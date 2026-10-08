use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct PageSpec {
    pub src: u32, // 0 = blank page of size (w, h)
    #[serde(default)]
    pub rotation: i32,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
}

impl PageSpec {
    pub fn page(src: u32, rotation: i32) -> Self {
        Self { src, rotation, w: 0.0, h: 0.0 }
    }
}

#[derive(Deserialize, Clone)]
pub struct Highlight {
    pub src: u32,
    pub rects: Vec<[f64; 4]>,
    pub color: [f32; 3],
}

#[derive(Deserialize, Clone)]
pub struct Note {
    pub src: u32,
    pub x: f64, // top-left anchor in PDF user space
    pub y: f64,
    pub text: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub src: u32,
    pub kind: String,
    pub rect: [f64; 4],
    pub strokes: Vec<Vec<[f64; 2]>>,
    pub color: [f32; 3],
    pub width: f64,
    pub opacity: f32,
}

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn root_pages_id(doc: &Document) -> R<ObjectId> {
    let root_id = doc
        .trailer
        .get(b"Root")
        .and_then(|o| o.as_reference())
        .map_err(err)?;
    let catalog = doc.get_object(root_id).and_then(|o| o.as_dict()).map_err(err)?;
    catalog
        .get(b"Pages")
        .and_then(|o| o.as_reference())
        .map_err(err)
}

const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

pub(crate) fn find_inherited(doc: &Document, start: &Dictionary, key: &[u8]) -> Option<Object> {
    let mut current = start.get(b"Parent").ok()?.as_reference().ok();
    while let Some(pid) = current {
        let dict = doc.get_object(pid).ok()?.as_dict().ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(v.clone());
        }
        current = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

// Pages can inherit attributes from intermediate Pages nodes; copy them onto the
// page itself before we flatten the tree, or reparented pages lose them.
fn pull_down_inherited(doc: &mut Document, page_id: ObjectId) -> R<()> {
    let mut to_set: Vec<(Vec<u8>, Object)> = Vec::new();
    {
        let page = doc.get_object(page_id).and_then(|o| o.as_dict()).map_err(err)?;
        for key in INHERITABLE {
            if page.get(key).is_err() {
                if let Some(v) = find_inherited(doc, page, key) {
                    to_set.push((key.to_vec(), v));
                }
            }
        }
    }
    if !to_set.is_empty() {
        let page = doc
            .get_object_mut(page_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        for (k, v) in to_set {
            page.set(k, v);
        }
    }
    Ok(())
}

fn append_annot(doc: &mut Document, page_id: ObjectId, annot_id: ObjectId) -> R<()> {
    let existing: Vec<Object> = {
        let page = doc.get_object(page_id).and_then(|o| o.as_dict()).map_err(err)?;
        match page.get(b"Annots") {
            Ok(Object::Array(a)) => a.clone(),
            Ok(Object::Reference(r)) => doc
                .get_object(*r)
                .ok()
                .and_then(|o| o.as_array().ok())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    };
    let mut arr = existing;
    arr.push(Object::Reference(annot_id));
    let page = doc
        .get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    page.set("Annots", arr);
    Ok(())
}

fn add_highlight(doc: &mut Document, page_id: ObjectId, h: &Highlight) -> R<()> {
    if h.rects.is_empty() {
        return Ok(());
    }
    // Normalized rects (lx, ly, hx, hy), overall bounds, and quad points.
    let mut norm: Vec<(f64, f64, f64, f64)> = Vec::new();
    let (mut minx, mut miny, mut maxx, mut maxy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    let mut quads: Vec<Object> = Vec::new();
    for &[x1, y1, x2, y2] in &h.rects {
        let (lx, hx) = (x1.min(x2), x1.max(x2));
        let (ly, hy) = (y1.min(y2), y1.max(y2));
        minx = minx.min(lx);
        miny = miny.min(ly);
        maxx = maxx.max(hx);
        maxy = maxy.max(hy);
        for v in [lx, hy, hx, hy, lx, ly, hx, ly] {
            quads.push(v.into());
        }
        norm.push((lx, ly, hx, hy));
    }

    // Appearance stream so the highlight renders in every viewer. A Form
    // XObject's coordinates are the page's, clipped to BBox.
    let mut ops = vec![
        Operation::new("gs", vec!["G0".into()]),
        Operation::new(
            "rg",
            vec![h.color[0].into(), h.color[1].into(), h.color[2].into()],
        ),
    ];
    for &(lx, ly, hx, hy) in &norm {
        ops.push(Operation::new(
            "re",
            vec![lx.into(), ly.into(), (hx - lx).into(), (hy - ly).into()],
        ));
    }
    ops.push(Operation::new("f", vec![]));
    let content = Content { operations: ops };
    let ap = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![minx.into(), miny.into(), maxx.into(), maxy.into()],
            "Group" => dictionary! { "S" => "Transparency" },
            "Resources" => dictionary! {
                "ExtGState" => dictionary! {
                    "G0" => dictionary! {
                        "Type" => "ExtGState",
                        "BM" => "Multiply",
                        "CA" => Object::Real(0.45),
                        "ca" => Object::Real(0.45),
                    },
                },
            },
        },
        content.encode().map_err(err)?,
    );
    let ap_id = doc.add_object(ap);

    let annot = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Highlight",
        "Rect" => vec![minx.into(), miny.into(), maxx.into(), maxy.into()],
        "QuadPoints" => quads,
        "C" => vec![Object::Real(h.color[0]), Object::Real(h.color[1]), Object::Real(h.color[2])],
        "CA" => Object::Real(0.45),
        "F" => 4,
        "AP" => dictionary! { "N" => ap_id },
    };
    let annot_id = doc.add_object(annot);
    append_annot(doc, page_id, annot_id)
}

fn add_note(doc: &mut Document, page_id: ObjectId, note: &Note) -> R<()> {
    // 18x18 sticky-note glyph as the appearance stream.
    let content = Content {
        operations: vec![
            Operation::new("rg", vec![1.into(), 0.8.into(), 0.into()]),
            Operation::new("re", vec![0.into(), 0.into(), 18.into(), 18.into()]),
            Operation::new("f", vec![]),
            Operation::new("RG", vec![0.6.into(), 0.45.into(), 0.into()]),
            Operation::new("w", vec![0.75.into()]),
            Operation::new("re", vec![0.4.into(), 0.4.into(), 17.2.into(), 17.2.into()]),
            Operation::new("S", vec![]),
            Operation::new("RG", vec![0.45.into(), 0.33.into(), 0.into()]),
            Operation::new("m", vec![3.into(), 11.5.into()]),
            Operation::new("l", vec![15.into(), 11.5.into()]),
            Operation::new("S", vec![]),
            Operation::new("m", vec![3.into(), 7.into()]),
            Operation::new("l", vec![11.into(), 7.into()]),
            Operation::new("S", vec![]),
        ],
    };
    let ap = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 18.into(), 18.into()],
        },
        content.encode().map_err(err)?,
    );
    let ap_id = doc.add_object(ap);

    let annot = dictionary! {
        "Type" => "Annot",
        "Subtype" => "Text",
        "Rect" => vec![
            note.x.into(),
            (note.y - 18.0).into(),
            (note.x + 18.0).into(),
            note.y.into(),
        ],
        "Contents" => Object::string_literal(note.text.clone()),
        "Name" => "Comment",
        "C" => vec![Object::Real(1.0), Object::Real(0.78), Object::Real(0.1)],
        "F" => 4,
        "T" => Object::string_literal("Note"),
        "AP" => dictionary! { "N" => ap_id },
    };
    let annot_id = doc.add_object(annot);
    append_annot(doc, page_id, annot_id)
}

fn add_shape(doc: &mut Document, page_id: ObjectId, s: &Shape) -> R<()> {
    let [x1, y1, x2, y2] = s.rect;
    let (lx, hx) = (x1.min(x2), x1.max(x2));
    let (ly, hy) = (y1.min(y2), y1.max(y2));
    let w = s.width;
    let bbox = [lx - w, ly - w, hx + w, hy + w];
    let (r, g, b) = (s.color[0], s.color[1], s.color[2]);

    let mut ops = vec![
        Operation::new("gs", vec!["G0".into()]),
        Operation::new("RG", vec![r.into(), g.into(), b.into()]),
        Operation::new("w", vec![w.into()]),
        Operation::new("J", vec![1.into()]),
        Operation::new("j", vec![1.into()]),
    ];

    let subtype;
    let mut extra: Vec<(&str, Object)> = Vec::new();

    match s.kind.as_str() {
        "ink" => {
            subtype = "Ink";
            let mut list: Vec<Object> = Vec::new();
            for stroke in &s.strokes {
                if stroke.len() < 2 {
                    continue;
                }
                ops.push(Operation::new("m", vec![stroke[0][0].into(), stroke[0][1].into()]));
                for p in &stroke[1..] {
                    ops.push(Operation::new("l", vec![p[0].into(), p[1].into()]));
                }
                ops.push(Operation::new("S", vec![]));
                let flat: Vec<Object> = stroke
                    .iter()
                    .flat_map(|p| [p[0].into(), p[1].into()])
                    .collect();
                list.push(Object::Array(flat));
            }
            extra.push(("InkList", Object::Array(list)));
        }
        "rect" | "ellipse" => {
            let ix = lx + w / 2.0;
            let iy = ly + w / 2.0;
            let iw = (hx - lx - w).max(0.0);
            let ih = (hy - ly - w).max(0.0);
            if s.kind == "rect" {
                subtype = "Square";
                ops.push(Operation::new(
                    "re",
                    vec![ix.into(), iy.into(), iw.into(), ih.into()],
                ));
            } else {
                subtype = "Circle";
                let (cx, cy) = (ix + iw / 2.0, iy + ih / 2.0);
                let (rx, ry) = (iw / 2.0, ih / 2.0);
                let k = 0.5523;
                ops.push(Operation::new("m", vec![(cx + rx).into(), cy.into()]));
                for [c1x, c1y, c2x, c2y, ex, ey] in [
                    [cx + rx, cy + k * ry, cx + k * rx, cy + ry, cx, cy + ry],
                    [cx - k * rx, cy + ry, cx - rx, cy + k * ry, cx - rx, cy],
                    [cx - rx, cy - k * ry, cx - k * rx, cy - ry, cx, cy - ry],
                    [cx + k * rx, cy - ry, cx + rx, cy - k * ry, cx + rx, cy],
                ] {
                    ops.push(Operation::new(
                        "c",
                        vec![c1x.into(), c1y.into(), c2x.into(), c2y.into(), ex.into(), ey.into()],
                    ));
                }
            }
            ops.push(Operation::new("S", vec![]));
        }
        "line" | "arrow" => {
            subtype = "Line";
            let pts = s
                .strokes
                .first()
                .filter(|p| p.len() >= 2)
                .ok_or_else(|| format!("{} shape needs 2 points", s.kind))?;
            let (p1, p2) = (pts[0], pts[1]);
            extra.push((
                "L",
                Object::Array(vec![p1[0].into(), p1[1].into(), p2[0].into(), p2[1].into()]),
            ));
            if s.kind == "line" {
                ops.push(Operation::new("m", vec![p1[0].into(), p1[1].into()]));
                ops.push(Operation::new("l", vec![p2[0].into(), p2[1].into()]));
                ops.push(Operation::new("S", vec![]));
            } else {
                extra.push((
                    "LE",
                    Object::Array(vec![
                        Object::Name(b"None".to_vec()),
                        Object::Name(b"ClosedArrow".to_vec()),
                    ]),
                ));
                let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 1e-6 {
                    let head = (4.5 * w).max(9.0);
                    let (ux, uy) = (dx / len, dy / len);
                    let (ex, ey) = (p2[0] - ux * head, p2[1] - uy * head);
                    ops.push(Operation::new("m", vec![p1[0].into(), p1[1].into()]));
                    ops.push(Operation::new("l", vec![ex.into(), ey.into()]));
                    ops.push(Operation::new("S", vec![]));
                    let (nx, ny) = (-uy, ux);
                    let hb = head / 2.0;
                    ops.push(Operation::new("rg", vec![r.into(), g.into(), b.into()]));
                    ops.push(Operation::new("m", vec![p2[0].into(), p2[1].into()]));
                    ops.push(Operation::new(
                        "l",
                        vec![(ex + nx * hb).into(), (ey + ny * hb).into()],
                    ));
                    ops.push(Operation::new(
                        "l",
                        vec![(ex - nx * hb).into(), (ey - ny * hb).into()],
                    ));
                    ops.push(Operation::new("f", vec![]));
                }
            }
        }
        other => return Err(format!("unknown shape kind {other}")),
    }

    let content = Content { operations: ops };
    let ap = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => bbox.iter().map(|&v| v.into()).collect::<Vec<Object>>(),
            "Resources" => dictionary! {
                "ExtGState" => dictionary! {
                    "G0" => dictionary! {
                        "Type" => "ExtGState",
                        "CA" => Object::Real(s.opacity),
                        "ca" => Object::Real(s.opacity),
                    },
                },
            },
        },
        content.encode().map_err(err)?,
    );
    let ap_id = doc.add_object(ap);

    let mut annot = dictionary! {
        "Type" => "Annot",
        "Subtype" => subtype,
        "Rect" => bbox.iter().map(|&v| v.into()).collect::<Vec<Object>>(),
        "C" => vec![Object::Real(r), Object::Real(g), Object::Real(b)],
        "CA" => Object::Real(s.opacity),
        "F" => 4,
        "BS" => dictionary! { "W" => s.width },
        "AP" => dictionary! { "N" => ap_id },
    };
    for (k, v) in extra {
        annot.set(k, v);
    }
    let annot_id = doc.add_object(annot);
    append_annot(doc, page_id, annot_id)
}

/// Rebuild the document's page tree: keep only the pages in `specs`, in that
/// order, applying per-page rotation deltas, then attach annotations.
pub fn rebuild(
    doc: &mut Document,
    specs: &[PageSpec],
    highlights: &[Highlight],
    notes: &[Note],
    shapes: &[Shape],
) -> R<()> {
    let page_map = doc.get_pages();
    let pages_root = root_pages_id(doc)?;
    let mut kids: Vec<ObjectId> = Vec::new();

    let mut base_rot: std::collections::HashMap<u32, i64> = std::collections::HashMap::new();
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for spec in specs {
        let new_rot = |base: i64| ((base + spec.rotation as i64) % 360 + 360) % 360;

        if spec.src == 0 {
            let w = if spec.w > 0.0 { spec.w } else { 612.0 };
            let h = if spec.h > 0.0 { spec.h } else { 792.0 };
            let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
            let mut page = dictionary! {
                "Type" => "Page",
                "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
                "Resources" => Object::Dictionary(Dictionary::new()),
                "Contents" => content_id,
                "Parent" => Object::Reference(pages_root),
            };
            let rot = new_rot(0);
            if rot != 0 {
                page.set("Rotate", rot);
            }
            kids.push(doc.add_object(page));
            continue;
        }

        let oid = *page_map
            .get(&spec.src)
            .ok_or_else(|| format!("page {} not found", spec.src))?;

        if seen.insert(spec.src) {
            pull_down_inherited(doc, oid)?;
            let page = doc
                .get_object_mut(oid)
                .and_then(|o| o.as_dict_mut())
                .map_err(err)?;
            let existing_rot = page.get(b"Rotate").and_then(|o| o.as_i64()).unwrap_or(0);
            base_rot.insert(spec.src, existing_rot);
            let rot = new_rot(existing_rot);
            if rot != 0 {
                page.set("Rotate", rot);
            } else {
                page.remove(b"Rotate");
            }
            page.set("Parent", Object::Reference(pages_root));
            kids.push(oid);
        } else {
            // Later occurrence of the same source page: clone the dict so Kids
            // never repeats an ObjectId; Contents/Resources stay shared.
            let mut dict = doc
                .get_object(oid)
                .and_then(|o| o.as_dict())
                .map_err(err)?
                .clone();
            let rot = new_rot(base_rot[&spec.src]);
            if rot != 0 {
                dict.set("Rotate", rot);
            } else {
                dict.remove(b"Rotate");
            }
            dict.set("Parent", Object::Reference(pages_root));
            kids.push(doc.add_object(Object::Dictionary(dict)));
        }
    }

    for h in highlights {
        if let Some(&oid) = page_map.get(&h.src) {
            if kids.contains(&oid) {
                add_highlight(doc, oid, h)?;
            }
        }
    }

    for n in notes {
        if let Some(&oid) = page_map.get(&n.src) {
            if kids.contains(&oid) {
                add_note(doc, oid, n)?;
            }
        }
    }

    for s in shapes {
        if let Some(&oid) = page_map.get(&s.src) {
            if kids.contains(&oid) {
                add_shape(doc, oid, s)?;
            }
        }
    }

    let pages = doc
        .get_object_mut(pages_root)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    pages.set(
        "Kids",
        kids.iter().map(|&k| Object::Reference(k)).collect::<Vec<_>>(),
    );
    pages.set("Count", kids.len() as i64);
    for key in INHERITABLE {
        pages.remove(key);
    }

    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    Ok(())
}

/// Merge multiple PDFs into one document, preserving page order.
pub fn merge(paths: &[String]) -> R<Document> {
    if paths.is_empty() {
        return Err("no files to merge".into());
    }
    let mut max_id = 1u32;
    let mut all_pages: Vec<(ObjectId, Object)> = Vec::new();
    let mut all_objects: Vec<(ObjectId, Object)> = Vec::new();
    let mut merged = Document::with_version("1.7");

    for path in paths {
        let mut doc = Document::load(path).map_err(|e| format!("{path}: {e}"))?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for (_, oid) in doc.get_pages() {
            pull_down_inherited(&mut doc, oid)?;
            let obj = doc.get_object(oid).map_err(err)?.to_owned();
            all_pages.push((oid, obj));
        }
        all_objects.extend(doc.objects);
    }

    let mut catalog_id: Option<ObjectId> = None;
    let mut pages_id: Option<ObjectId> = None;
    let page_ids: std::collections::BTreeSet<ObjectId> =
        all_pages.iter().map(|(id, _)| *id).collect();

    for (oid, obj) in all_objects {
        let type_name = obj.type_name().map(|n| n.to_vec()).unwrap_or_default();
        match type_name.as_slice() {
            b"Catalog" => {
                if catalog_id.is_none() {
                    catalog_id = Some(oid);
                    merged.objects.insert(oid, obj);
                }
            }
            b"Pages" => {
                if pages_id.is_none() {
                    pages_id = Some(oid);
                }
            }
            b"Outlines" | b"Outline" => {}
            _ => {
                if !page_ids.contains(&oid) {
                    merged.objects.insert(oid, obj);
                }
            }
        }
    }

    let pages_id = pages_id.ok_or("no Pages node found")?;
    let catalog_id = catalog_id.ok_or("no Catalog found")?;

    for (oid, obj) in &all_pages {
        let mut dict = obj.as_dict().map_err(err)?.clone();
        dict.set("Parent", Object::Reference(pages_id));
        merged.objects.insert(*oid, Object::Dictionary(dict));
    }

    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => all_pages.len() as i64,
            "Kids" => all_pages.iter().map(|(id, _)| Object::Reference(*id)).collect::<Vec<_>>(),
        }),
    );

    {
        let catalog = merged
            .get_object_mut(catalog_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        catalog.set("Pages", Object::Reference(pages_id));
        catalog.remove(b"Outlines");
        catalog.remove(b"Names");
        catalog.remove(b"OpenAction");
        catalog.remove(b"AcroForm");
    }

    merged.trailer.set("Root", Object::Reference(catalog_id));
    merged.max_id = max_id;
    merged.prune_objects();
    merged.renumber_objects();
    merged.compress();
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc(pages: usize) -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut kids: Vec<Object> = Vec::new();
        for n in 1..=pages {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![72.into(), 700.into()]),
                    Operation::new("Tj", vec![Object::string_literal(format!("Page {n}"))]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id =
                doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => resources_id,
            });
            kids.push(Object::Reference(page_id));
        }
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    fn roundtrip(doc: &mut Document) -> Document {
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        Document::load_mem(&buf).unwrap()
    }

    #[test]
    fn rebuild_reorders_and_rotates() {
        let mut doc = sample_doc(4);
        rebuild(
            &mut doc,
            &[
                PageSpec::page(3, 90),
                PageSpec::page(1, 0),
            ],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();
        assert_eq!(pages.len(), 2);
        let first = out.get_object(pages[&1]).unwrap().as_dict().unwrap();
        assert_eq!(first.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        let second = out.get_object(pages[&2]).unwrap().as_dict().unwrap();
        assert!(second.get(b"Rotate").is_err());
    }

    #[test]
    fn merge_concatenates() {
        let dir = std::env::temp_dir();
        let p1 = dir.join("pdfeditor_test_merge_a.pdf");
        let p2 = dir.join("pdfeditor_test_merge_b.pdf");
        sample_doc(2).save(&p1).unwrap();
        sample_doc(3).save(&p2).unwrap();
        let mut merged = merge(&[
            p1.to_string_lossy().to_string(),
            p2.to_string_lossy().to_string(),
        ])
        .unwrap();
        let out = roundtrip(&mut merged);
        assert_eq!(out.get_pages().len(), 5);
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }

    #[test]
    fn annotations_have_appearance() {
        let mut doc = sample_doc(2);
        let specs: Vec<PageSpec> = (1..=2).map(|i| PageSpec::page(i, 0)).collect();
        let hl = Highlight {
            src: 1,
            rects: vec![[72.0, 690.0, 200.0, 710.0]],
            color: [1.0, 0.82, 0.0],
        };
        let note = Note {
            src: 1,
            x: 300.0,
            y: 750.0,
            text: "hello note".into(),
        };
        rebuild(&mut doc, &specs, &[hl], &[note], &[]).unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();
        let page = out.get_object(pages[&1]).unwrap().as_dict().unwrap();
        let annots = page.get(b"Annots").unwrap().as_array().unwrap();
        assert_eq!(annots.len(), 2);
        for a in annots {
            let id = a.as_reference().unwrap();
            let d = out.get_object(id).unwrap().as_dict().unwrap();
            assert!(d.get(b"AP").is_ok(), "annotation missing /AP");
        }
    }

    fn shape(kind: &str, strokes: Vec<Vec<[f64; 2]>>) -> Shape {
        Shape {
            src: 1,
            kind: kind.into(),
            rect: [100.0, 100.0, 200.0, 160.0],
            strokes,
            color: [0.8, 0.1, 0.1],
            width: 2.0,
            opacity: 0.9,
        }
    }

    fn page_annots(out: &Document) -> Vec<Dictionary> {
        let pages = out.get_pages();
        let page = out.get_object(pages[&1]).unwrap().as_dict().unwrap();
        page.get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                out.get_object(a.as_reference().unwrap())
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .clone()
            })
            .collect()
    }

    #[test]
    fn shapes_all_kinds_have_appearance() {
        let mut doc = sample_doc(1);
        let line = vec![vec![[100.0, 100.0], [200.0, 160.0]]];
        let shapes = vec![
            shape("ink", vec![vec![[100.0, 100.0], [150.0, 160.0], [200.0, 100.0]]]),
            shape("rect", vec![]),
            shape("ellipse", vec![]),
            shape("line", line.clone()),
            shape("arrow", line),
        ];
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &shapes).unwrap();
        let out = roundtrip(&mut doc);
        let annots = page_annots(&out);
        assert_eq!(annots.len(), 5);
        let subtypes: Vec<&[u8]> = annots
            .iter()
            .map(|d| d.get(b"Subtype").unwrap().as_name().unwrap())
            .collect();
        assert_eq!(subtypes, vec![&b"Ink"[..], b"Square", b"Circle", b"Line", b"Line"]);
        for d in &annots {
            assert!(d.get(b"AP").is_ok(), "shape missing /AP");
        }
    }

    #[test]
    fn ink_strokes_in_inklist() {
        let mut doc = sample_doc(1);
        let s = shape(
            "ink",
            vec![
                vec![[100.0, 100.0], [150.0, 150.0]],
                vec![[120.0, 100.0], [170.0, 150.0]],
            ],
        );
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &[s]).unwrap();
        let out = roundtrip(&mut doc);
        let annots = page_annots(&out);
        let ink = annots[0].get(b"InkList").unwrap().as_array().unwrap();
        assert_eq!(ink.len(), 2);
    }

    #[test]
    fn arrow_has_closed_arrow_ending() {
        let mut doc = sample_doc(1);
        let s = shape("arrow", vec![vec![[100.0, 100.0], [200.0, 160.0]]]);
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &[s]).unwrap();
        let out = roundtrip(&mut doc);
        let annots = page_annots(&out);
        let le = annots[0].get(b"LE").unwrap().as_array().unwrap();
        assert_eq!(le[1].as_name().unwrap(), b"ClosedArrow");
    }

    fn num(o: &Object) -> f64 {
        match o {
            Object::Integer(i) => *i as f64,
            Object::Real(r) => *r as f64,
            _ => panic!("not a number"),
        }
    }

    #[test]
    fn blank_page_inserted() {
        let mut doc = sample_doc(2);
        rebuild(
            &mut doc,
            &[
                PageSpec::page(1, 0),
                PageSpec { src: 0, rotation: 0, w: 300.0, h: 400.0 },
                PageSpec::page(2, 0),
            ],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();
        assert_eq!(pages.len(), 3);
        let mid = out.get_object(pages[&2]).unwrap().as_dict().unwrap();
        let mb = mid.get(b"MediaBox").unwrap().as_array().unwrap();
        assert_eq!(num(&mb[2]), 300.0);
        assert_eq!(num(&mb[3]), 400.0);
    }

    #[test]
    fn duplicate_pages_are_distinct_objects() {
        let mut doc = sample_doc(2);
        rebuild(
            &mut doc,
            &[PageSpec::page(1, 0), PageSpec::page(1, 90), PageSpec::page(2, 0)],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();
        assert_eq!(pages.len(), 3);
        assert_ne!(pages[&1], pages[&2]);
        let p1 = out.get_object(pages[&1]).unwrap().as_dict().unwrap();
        let p2 = out.get_object(pages[&2]).unwrap().as_dict().unwrap();
        assert!(p1.get(b"Rotate").is_err());
        assert_eq!(p2.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
        assert_eq!(
            p1.get(b"Contents").unwrap().as_reference().unwrap(),
            p2.get(b"Contents").unwrap().as_reference().unwrap()
        );
    }

    #[test]
    fn blank_page_with_stamp_saves() {
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine;
        let mut doc = sample_doc(2);
        let data: Vec<u8> = (0..4).flat_map(|_| [255u8, 0, 0, 255]).collect();
        let stamp = crate::stamps::Stamp {
            src: 1,
            rect: [10.0, 10.0, 110.0, 60.0],
            w: 2,
            h: 2,
            rgba_b64: B64.encode(&data),
        };
        crate::stamps::apply_stamps(&mut doc, &[stamp]).unwrap();
        rebuild(
            &mut doc,
            &[PageSpec::page(1, 0), PageSpec { src: 0, rotation: 0, w: 0.0, h: 0.0 }],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 2);
    }
}
