// Markup annotations added on save: highlights, sticky notes and drawn
// shapes, each with an appearance stream so every viewer renders them.
use crate::common::{err, page_annots, R};
use crate::pages::PageIndex;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Highlight {
    pub src: i32,
    pub rects: Vec<[f64; 4]>,
    pub color: [f32; 3],
}

#[derive(Deserialize, Clone)]
pub struct Note {
    pub src: i32,
    pub x: f64, // top-left anchor in PDF user space
    pub y: f64,
    pub text: String,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub src: i32,
    pub kind: String,
    pub rect: [f64; 4],
    pub strokes: Vec<Vec<[f64; 2]>>,
    pub color: [f32; 3],
    pub width: f64,
    pub opacity: f32,
}

pub(crate) fn append_annot(doc: &mut Document, page_id: ObjectId, annot_id: ObjectId) -> R<()> {
    let mut arr = page_annots(doc, page_id);
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
                ops.push(Operation::new(
                    "m",
                    vec![stroke[0][0].into(), stroke[0][1].into()],
                ));
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
                        vec![
                            c1x.into(),
                            c1y.into(),
                            c2x.into(),
                            c2y.into(),
                            ex.into(),
                            ey.into(),
                        ],
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

// Attaches every annotation to its page, skipping pages that did not
// survive the rebuild.
pub fn apply(
    doc: &mut Document,
    index: &PageIndex,
    highlights: &[Highlight],
    notes: &[Note],
    shapes: &[Shape],
) -> R<()> {
    let target = |src: i32| index.page_for(src);
    for h in highlights {
        if let Some(oid) = target(h.src) {
            add_highlight(doc, oid, h)?;
        }
    }
    for n in notes {
        if let Some(oid) = target(n.src) {
            add_note(doc, oid, n)?;
        }
    }
    for s in shapes {
        if let Some(oid) = target(s.src) {
            add_shape(doc, oid, s)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};
    use crate::pages::{rebuild, PageSpec};
    use lopdf::Dictionary;

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

    fn first_page_annots(out: &Document) -> Vec<Dictionary> {
        let pages = out.get_pages();
        page_annots(out, pages[&1])
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
        let annots = first_page_annots(&out);
        assert_eq!(annots.len(), 2);
        for d in &annots {
            assert!(d.get(b"AP").is_ok(), "annotation missing /AP");
        }
    }

    #[test]
    fn annotations_on_dropped_pages_are_skipped() {
        let mut doc = sample_doc(2);
        let note = Note {
            src: 2,
            x: 10.0,
            y: 50.0,
            text: "gone".into(),
        };
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[note], &[]).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 1);
        assert!(first_page_annots(&out).is_empty());
    }

    #[test]
    fn shapes_all_kinds_have_appearance() {
        let mut doc = sample_doc(1);
        let line = vec![vec![[100.0, 100.0], [200.0, 160.0]]];
        let shapes = vec![
            shape(
                "ink",
                vec![vec![[100.0, 100.0], [150.0, 160.0], [200.0, 100.0]]],
            ),
            shape("rect", vec![]),
            shape("ellipse", vec![]),
            shape("line", line.clone()),
            shape("arrow", line),
        ];
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &shapes).unwrap();
        let out = roundtrip(&mut doc);
        let annots = first_page_annots(&out);
        assert_eq!(annots.len(), 5);
        let subtypes: Vec<&[u8]> = annots
            .iter()
            .map(|d| d.get(b"Subtype").unwrap().as_name().unwrap())
            .collect();
        assert_eq!(
            subtypes,
            vec![&b"Ink"[..], b"Square", b"Circle", b"Line", b"Line"]
        );
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
        let annots = first_page_annots(&out);
        let ink = annots[0].get(b"InkList").unwrap().as_array().unwrap();
        assert_eq!(ink.len(), 2);
    }

    #[test]
    fn arrow_has_closed_arrow_ending() {
        let mut doc = sample_doc(1);
        let s = shape("arrow", vec![vec![[100.0, 100.0], [200.0, 160.0]]]);
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &[s]).unwrap();
        let out = roundtrip(&mut doc);
        let annots = first_page_annots(&out);
        let le = annots[0].get(b"LE").unwrap().as_array().unwrap();
        assert_eq!(le[1].as_name().unwrap(), b"ClosedArrow");
    }
}
