use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::Deserialize;

use crate::common::{err, rgba_xobject, R};
use crate::pages::PageIndex;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    pub src: i32,
    pub rect: [f64; 4],
    pub w: u32,
    pub h: u32,
    pub rgba_b64: String,
}

fn resolved_dict(doc: &Document, obj: &Object) -> Dictionary {
    match obj {
        Object::Dictionary(d) => d.clone(),
        Object::Reference(r) => doc
            .get_object(*r)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default(),
        _ => Dictionary::new(),
    }
}

fn page_resources(doc: &Document, page_id: ObjectId) -> R<Dictionary> {
    let page = doc
        .get_object(page_id)
        .and_then(|o| o.as_dict())
        .map_err(err)?;
    if let Ok(obj) = page.get(b"Resources") {
        return Ok(resolved_dict(doc, &obj.clone()));
    }
    if let Some(obj) = crate::pages::find_inherited(doc, page, b"Resources") {
        return Ok(resolved_dict(doc, &obj));
    }
    Ok(Dictionary::new())
}

fn register_xobject(
    doc: &mut Document,
    page_id: ObjectId,
    img_id: ObjectId,
    counter: &mut usize,
) -> R<String> {
    let mut resources = page_resources(doc, page_id)?;
    let mut xobjects = resources
        .get(b"XObject")
        .map(|o| resolved_dict(doc, &o.clone()))
        .unwrap_or_default();
    let name = loop {
        let cand = format!("WImg{counter}");
        *counter += 1;
        if xobjects.get(cand.as_bytes()).is_err() {
            break cand;
        }
    };
    xobjects.set(name.as_bytes(), Object::Reference(img_id));
    resources.set("XObject", Object::Dictionary(xobjects));
    let page = doc
        .get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    page.set("Resources", Object::Dictionary(resources));
    Ok(name)
}

// Draws each stamp onto the page `index` resolves its `src` to; stamps on
// pages the index does not know are skipped.
pub fn apply_stamps(doc: &mut Document, stamps: &[Stamp], index: &PageIndex) -> R<()> {
    let mut counter = 0usize;
    for s in stamps {
        let Some(pid) = index.page_for(s.src) else {
            continue;
        };
        let raw = B64.decode(&s.rgba_b64).map_err(err)?;
        let img_id = rgba_xobject(doc, s.w, s.h, &raw)?;
        let name = register_xobject(doc, pid, img_id, &mut counter)?;
        let [x1, y1, x2, y2] = s.rect;
        // The existing content is wrapped in q/Q so an unbalanced state it
        // leaves behind cannot move or scale the stamp.
        crate::common::ensure_contents(doc, pid)?;
        let mut content = b"q\n".to_vec();
        content.extend_from_slice(&doc.get_page_content(pid));
        content.extend_from_slice(
            format!(
                "\nQ\nq {:.4} 0 0 {:.4} {:.4} {:.4} cm /{} Do Q\n",
                x2 - x1,
                y2 - y1,
                x1,
                y1,
                name
            )
            .as_bytes(),
        );
        doc.change_page_content(pid, content).map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};
    use crate::editor;
    use crate::pages::{rebuild, PageSpec};

    fn stamp_all(doc: &mut Document, stamps: &[Stamp]) -> R<()> {
        let index = PageIndex::of(doc);
        apply_stamps(doc, stamps, &index)
    }

    fn red_2x2() -> Stamp {
        let rgba: Vec<u8> = (0..4).flat_map(|_| [255u8, 0, 0, 255]).collect();
        Stamp {
            src: 1,
            rect: [10.0, 10.0, 110.0, 60.0],
            w: 2,
            h: 2,
            rgba_b64: B64.encode(&rgba),
        }
    }

    #[test]
    fn stamp_adds_image_with_smask() {
        let mut doc = sample_doc(1);
        stamp_all(&mut doc, &[red_2x2()]).unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();
        let pid = pages[&1];

        let objs = editor::enumerate(&out, pid).unwrap();
        let img = objs
            .iter()
            .find(|o| o.kind == editor::Kind::Image)
            .expect("no image object");
        let want = [10.0, 10.0, 110.0, 60.0];
        for (a, b) in img.bbox.iter().zip(want.iter()) {
            assert!((a - b).abs() < 0.5, "bbox {:?} != {:?}", img.bbox, want);
        }

        let page = out.get_object(pid).unwrap().as_dict().unwrap();
        let res = resolved_dict(&out, &page.get(b"Resources").unwrap().clone());
        let xobjs = resolved_dict(&out, &res.get(b"XObject").unwrap().clone());
        let img_ref = xobjs
            .get(b"WImg0")
            .expect("WImg0 missing")
            .as_reference()
            .unwrap();
        let stream = out.get_object(img_ref).unwrap().as_stream().unwrap();
        assert!(stream.dict.get(b"SMask").is_ok(), "image missing SMask");
    }

    #[test]
    fn shared_resources_stay_isolated() {
        let mut doc = sample_doc(2);
        stamp_all(&mut doc, &[red_2x2()]).unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();

        let p2 = out.get_object(pages[&2]).unwrap().as_dict().unwrap();
        let res2 = resolved_dict(&out, &p2.get(b"Resources").unwrap().clone());
        let has = res2
            .get(b"XObject")
            .map(|o| resolved_dict(&out, &o.clone()).get(b"WImg0").is_ok())
            .unwrap_or(false);
        assert!(!has, "stamp leaked into page 2 resources");

        let p1 = out.get_object(pages[&1]).unwrap().as_dict().unwrap();
        let res1 = resolved_dict(&out, &p1.get(b"Resources").unwrap().clone());
        assert!(res1.get(b"Font").is_ok(), "page 1 lost inherited fonts");
    }

    #[test]
    fn stamp_on_deleted_page_survives_rebuild() {
        let mut doc = sample_doc(2);
        let mut stamp = red_2x2();
        stamp.src = 2;
        stamp_all(&mut doc, &[stamp]).unwrap();
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &[]).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 1);
    }
}
