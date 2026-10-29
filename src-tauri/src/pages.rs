// Page tree rebuilding: page order, rotation, inherited attributes and the
// signature-widget cleanup every rewrite needs.
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::annots::{self, Highlight, Note, Shape};
use crate::common::{catalog_id, err, page_annots, rotation_of, R};
use crate::stamps::{self, Stamp};

#[derive(Deserialize, Clone)]
pub struct PageSpec {
    // > 0: 1-based source page. <= 0: a blank page of size (w, h); negative
    // values identify it so annotations can target it, 0 is anonymous.
    pub src: i32,
    #[serde(default)]
    pub rotation: i32,
    #[serde(default)]
    pub w: f64,
    #[serde(default)]
    pub h: f64,
}

impl PageSpec {
    pub fn page(src: i32, rotation: i32) -> Self {
        Self {
            src,
            rotation,
            w: 0.0,
            h: 0.0,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub struct Extras<'a> {
    pub outline: Option<&'a [crate::outline::OutlineNode]>,
    pub links: &'a [crate::links::LinkSpec],
    pub link_removals: &'a [crate::links::LinkRemoval],
    pub stamps: &'a [Stamp],
}

// Resolves a `src` page number to the page object it refers to in the
// output: source pages by their 1-based number, blank pages by the negative
// id the frontend gave them. Pages dropped by the rebuild resolve to None.
pub struct PageIndex {
    sources: BTreeMap<u32, ObjectId>,
    blanks: HashMap<i32, ObjectId>,
    kept: HashSet<ObjectId>,
}

impl PageIndex {
    // Every page of `doc` as it stands, with no blanks.
    #[cfg(test)]
    pub fn of(doc: &Document) -> Self {
        let sources = doc.get_pages();
        let kept = sources.values().copied().collect();
        Self {
            sources,
            blanks: HashMap::new(),
            kept,
        }
    }

    pub fn page_for(&self, src: i32) -> Option<ObjectId> {
        let oid = if src > 0 {
            self.sources.get(&(src as u32)).copied()?
        } else {
            *self.blanks.get(&src)?
        };
        self.kept.contains(&oid).then_some(oid)
    }
}

fn root_pages_id(doc: &Document) -> R<ObjectId> {
    let root_id = catalog_id(doc)?;
    let catalog = doc
        .get_object(root_id)
        .and_then(|o| o.as_dict())
        .map_err(err)?;
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
pub(crate) fn pull_down_inherited(doc: &mut Document, page_id: ObjectId) -> R<()> {
    let mut to_set: Vec<(Vec<u8>, Object)> = Vec::new();
    {
        let page = doc
            .get_object(page_id)
            .and_then(|o| o.as_dict())
            .map_err(err)?;
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

fn is_signature_widget(doc: &Document, annot: &Object) -> bool {
    let Some(d) = annot
        .as_reference()
        .ok()
        .and_then(|r| doc.get_object(r).ok())
        .and_then(|o| o.as_dict().ok())
    else {
        return false;
    };
    let ft_is_sig = |d: &Dictionary| {
        d.get(b"FT")
            .and_then(Object::as_name)
            .map(|n| n == b"Sig")
            .unwrap_or(false)
    };
    if ft_is_sig(d) {
        return true;
    }
    d.get(b"Parent")
        .ok()
        .and_then(|p| p.as_reference().ok())
        .and_then(|r| doc.get_object(r).ok())
        .and_then(|o| o.as_dict().ok())
        .map(ft_is_sig)
        .unwrap_or(false)
}

// A rewrite breaks any certificate signature the file carried, so its widgets
// must not survive as misleading stamps.
pub(crate) fn strip_signature_widgets(doc: &mut Document) {
    let pages: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in pages {
        let annots = page_annots(doc, page_id);
        if annots.is_empty() {
            continue;
        }
        let kept: Vec<Object> = annots
            .iter()
            .filter(|a| !is_signature_widget(doc, a))
            .cloned()
            .collect();
        if kept.len() == annots.len() {
            continue;
        }
        if let Ok(page) = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()) {
            if kept.is_empty() {
                page.remove(b"Annots");
            } else {
                page.set("Annots", Object::Array(kept));
            }
        }
    }
    // Drop the signature fields from the interactive form as well.
    let Ok(catalog_id) = catalog_id(doc) else {
        return;
    };
    let acro = doc
        .get_object(catalog_id)
        .ok()
        .and_then(|c| c.as_dict().ok())
        .and_then(|c| c.get(b"AcroForm").ok().cloned());
    let (acro_dict, acro_ref) = match acro {
        Some(Object::Reference(r)) => (
            doc.get_object(r)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .cloned(),
            Some(r),
        ),
        Some(Object::Dictionary(d)) => (Some(d), None),
        _ => (None, None),
    };
    let Some(mut acro_dict) = acro_dict else {
        return;
    };
    let fields: Vec<Object> = match acro_dict.get(b"Fields") {
        Ok(Object::Array(a)) => a.clone(),
        Ok(Object::Reference(r)) => doc
            .get_object(*r)
            .ok()
            .and_then(|o| o.as_array().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let kept: Vec<Object> = fields
        .iter()
        .filter(|f| !is_signature_widget(doc, f))
        .cloned()
        .collect();
    if kept.len() == fields.len() && acro_dict.get(b"SigFlags").is_err() {
        return;
    }
    acro_dict.set("Fields", Object::Array(kept));
    acro_dict.remove(b"SigFlags");
    match acro_ref {
        Some(r) => {
            doc.set_object(r, Object::Dictionary(acro_dict));
        }
        None => {
            if let Ok(cat) = doc.get_object_mut(catalog_id).and_then(|o| o.as_dict_mut()) {
                cat.set("AcroForm", Object::Dictionary(acro_dict));
            }
        }
    }
}

pub fn rebuild(
    doc: &mut Document,
    specs: &[PageSpec],
    highlights: &[Highlight],
    notes: &[Note],
    shapes: &[Shape],
) -> R<()> {
    rebuild_with(doc, specs, highlights, notes, shapes, &Extras::default())
}

fn rotation_after(base: i64, delta: i32) -> i64 {
    ((base + delta as i64) % 360 + 360) % 360
}

// Rebuilds the document's page tree: keeps only the pages in `specs`, in that
// order, applying per-page rotation deltas, then attaches stamps,
// annotations, links and the outline against the final page order.
pub fn rebuild_with(
    doc: &mut Document,
    specs: &[PageSpec],
    highlights: &[Highlight],
    notes: &[Note],
    shapes: &[Shape],
    extras: &Extras,
) -> R<()> {
    strip_signature_widgets(doc);
    let page_map = doc.get_pages();
    let pages_root = root_pages_id(doc)?;
    // Output slots in order; a repeated source page is cloned only after the
    // first copy has received its stamps and annotations.
    let mut kids: Vec<Option<ObjectId>> = Vec::new();
    let mut index = PageIndex {
        sources: page_map,
        blanks: HashMap::new(),
        kept: HashSet::new(),
    };

    let mut base_rot: HashMap<i32, i64> = HashMap::new();

    for spec in specs {
        let new_rot = |base: i64| rotation_after(base, spec.rotation);

        if spec.src <= 0 {
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
            let oid = doc.add_object(page);
            if spec.src < 0 {
                index.blanks.entry(spec.src).or_insert(oid);
            }
            index.kept.insert(oid);
            kids.push(Some(oid));
            continue;
        }

        let oid = *index
            .sources
            .get(&(spec.src as u32))
            .ok_or_else(|| format!("page {} not found", spec.src))?;

        if index.kept.insert(oid) {
            pull_down_inherited(doc, oid)?;
            let existing_rot = doc
                .get_object(oid)
                .ok()
                .and_then(|o| o.as_dict().ok())
                .map(|d| rotation_of(doc, d.get(b"Rotate").ok()))
                .unwrap_or(0);
            let page = doc
                .get_object_mut(oid)
                .and_then(|o| o.as_dict_mut())
                .map_err(err)?;
            base_rot.insert(spec.src, existing_rot);
            let rot = new_rot(existing_rot);
            if rot != 0 {
                page.set("Rotate", rot);
            } else {
                page.remove(b"Rotate");
            }
            page.set("Parent", Object::Reference(pages_root));
            kids.push(Some(oid));
        } else {
            kids.push(None);
        }
    }

    stamps::apply_stamps(doc, extras.stamps, &index)?;
    annots::apply(doc, &index, highlights, notes, shapes)?;

    // Later occurrences of the same source page: clone the dict so Kids never
    // repeats an ObjectId; Contents/Resources stay shared, while annotations
    // belong to the first copy only.
    let kids: Vec<ObjectId> = kids
        .into_iter()
        .zip(specs)
        .map(|(slot, spec)| match slot {
            Some(oid) => Ok(oid),
            None => {
                let oid = index.sources[&(spec.src as u32)];
                let mut dict = doc
                    .get_object(oid)
                    .and_then(|o| o.as_dict())
                    .map_err(err)?
                    .clone();
                dict.remove(b"Annots");
                let rot = rotation_after(base_rot[&spec.src], spec.rotation);
                if rot != 0 {
                    dict.set("Rotate", rot);
                } else {
                    dict.remove(b"Rotate");
                }
                dict.set("Parent", Object::Reference(pages_root));
                Ok(doc.add_object(Object::Dictionary(dict)))
            }
        })
        .collect::<R<_>>()?;

    let pages = doc
        .get_object_mut(pages_root)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    pages.set(
        "Kids",
        kids.iter()
            .map(|&k| Object::Reference(k))
            .collect::<Vec<_>>(),
    );
    pages.set("Count", kids.len() as i64);
    for key in INHERITABLE {
        pages.remove(key);
    }

    crate::links::apply(doc, &kids, extras.links, extras.link_removals)?;
    crate::outline::apply(doc, &kids, extras.outline)?;

    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};

    fn num(o: &Object) -> f64 {
        match o {
            Object::Integer(i) => *i as f64,
            Object::Real(r) => *r as f64,
            _ => panic!("not a number"),
        }
    }

    #[test]
    fn rebuild_reorders_and_rotates() {
        let mut doc = sample_doc(4);
        rebuild(
            &mut doc,
            &[PageSpec::page(3, 90), PageSpec::page(1, 0)],
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
    fn blank_page_inserted() {
        let mut doc = sample_doc(2);
        rebuild(
            &mut doc,
            &[
                PageSpec::page(1, 0),
                PageSpec {
                    src: 0,
                    rotation: 0,
                    w: 300.0,
                    h: 400.0,
                },
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
            &[
                PageSpec::page(1, 0),
                PageSpec::page(1, 90),
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

    fn stamp(src: i32) -> Stamp {
        use base64::engine::general_purpose::STANDARD as B64;
        use base64::Engine;
        let data: Vec<u8> = (0..4).flat_map(|_| [255u8, 0, 0, 255]).collect();
        Stamp {
            src,
            rect: [10.0, 10.0, 110.0, 60.0],
            w: 2,
            h: 2,
            rgba_b64: B64.encode(&data),
        }
    }

    fn note(src: i32, text: &str) -> Note {
        Note {
            src,
            x: 300.0,
            y: 750.0,
            text: text.into(),
        }
    }

    fn note_texts(doc: &Document, page: u32) -> Vec<String> {
        page_annots(doc, doc.get_pages()[&page])
            .iter()
            .map(|a| {
                let d = doc
                    .get_object(a.as_reference().unwrap())
                    .unwrap()
                    .as_dict()
                    .unwrap();
                String::from_utf8_lossy(d.get(b"Contents").unwrap().as_str().unwrap()).to_string()
            })
            .collect()
    }

    fn has_stamp(doc: &Document, page: u32) -> bool {
        let content = doc.get_page_content(doc.get_pages()[&page]);
        String::from_utf8_lossy(&content).contains("/WImg")
    }

    #[test]
    fn blank_page_with_stamp_saves() {
        let mut doc = sample_doc(2);
        let index = PageIndex::of(&doc);
        crate::stamps::apply_stamps(&mut doc, &[stamp(1)], &index).unwrap();
        rebuild(
            &mut doc,
            &[
                PageSpec::page(1, 0),
                PageSpec {
                    src: 0,
                    rotation: 0,
                    w: 0.0,
                    h: 0.0,
                },
            ],
            &[],
            &[],
            &[],
        )
        .unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 2);
    }

    #[test]
    fn blank_page_keeps_its_note_and_stamp() {
        let mut doc = sample_doc(1);
        let specs = [
            PageSpec::page(1, 0),
            PageSpec {
                src: -1,
                rotation: 0,
                w: 0.0,
                h: 0.0,
            },
        ];
        let stamps = [stamp(-1), stamp(1)];
        let extras = Extras {
            stamps: &stamps,
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs, &[], &[note(-1, "on blank")], &[], &extras).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(note_texts(&out, 2), vec!["on blank".to_string()]);
        assert!(note_texts(&out, 1).is_empty());
        assert!(has_stamp(&out, 1) && has_stamp(&out, 2));
    }

    #[test]
    fn blank_pages_are_told_apart_by_src() {
        let mut doc = sample_doc(1);
        let blank = |src| PageSpec {
            src,
            rotation: 0,
            w: 0.0,
            h: 0.0,
        };
        // -2 is listed twice: its note lands on the first copy only.
        let specs = [blank(-2), PageSpec::page(1, 0), blank(-1), blank(-2)];
        let notes = [note(-1, "first"), note(-2, "second")];
        rebuild_with(&mut doc, &specs, &[], &notes, &[], &Extras::default()).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 4);
        assert_eq!(note_texts(&out, 1), vec!["second".to_string()]);
        assert!(note_texts(&out, 2).is_empty());
        assert_eq!(note_texts(&out, 3), vec!["first".to_string()]);
        assert!(note_texts(&out, 4).is_empty());
    }

    #[test]
    fn legacy_src_zero_is_an_anonymous_blank() {
        let mut doc = sample_doc(1);
        let blank = PageSpec {
            src: 0,
            rotation: 0,
            w: 0.0,
            h: 0.0,
        };
        let specs = [blank.clone(), PageSpec::page(1, 0), blank];
        let stamps = [stamp(0)];
        let extras = Extras {
            stamps: &stamps,
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs, &[], &[note(0, "lost")], &[], &extras).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 3);
        for p in 1..=3 {
            assert!(note_texts(&out, p).is_empty());
            assert!(!has_stamp(&out, p));
        }
    }
}
