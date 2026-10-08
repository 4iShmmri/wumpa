use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::Deserialize;
use std::io::Write;

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
    pub src: u32,
    pub rect: [f64; 4],
    pub w: u32,
    pub h: u32,
    pub rgba_b64: String,
}

fn deflate(data: &[u8]) -> R<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(err)?;
    enc.finish().map_err(err)
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
    let page = doc.get_object(page_id).and_then(|o| o.as_dict()).map_err(err)?;
    if let Ok(obj) = page.get(b"Resources") {
        return Ok(resolved_dict(doc, &obj.clone()));
    }
    if let Some(obj) = crate::pdf_ops::find_inherited(doc, page, b"Resources") {
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
        let cand = format!("PES{counter}");
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

pub fn apply_stamps(doc: &mut Document, stamps: &[Stamp]) -> R<()> {
    if stamps.is_empty() {
        return Ok(());
    }
    let page_map = doc.get_pages();
    let mut counter = 0usize;
    for s in stamps {
        let Some(&pid) = page_map.get(&s.src) else {
            continue;
        };
        let raw = B64.decode(&s.rgba_b64).map_err(err)?;
        let expect = s.w as usize * s.h as usize * 4;
        if raw.len() != expect {
            return Err(format!("stamp buffer is {} bytes, expected {}", raw.len(), expect));
        }
        let mut rgb = Vec::with_capacity(expect / 4 * 3);
        let mut alpha = Vec::with_capacity(expect / 4);
        for px in raw.chunks_exact(4) {
            rgb.extend_from_slice(&px[..3]);
            alpha.push(px[3]);
        }
        let smask_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => s.w as i64,
                "Height" => s.h as i64,
                "ColorSpace" => "DeviceGray",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            deflate(&alpha)?,
        ));
        let img_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => s.w as i64,
                "Height" => s.h as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
                "SMask" => smask_id,
            },
            deflate(&rgb)?,
        ));
        let name = register_xobject(doc, pid, img_id, &mut counter)?;
        let [x1, y1, x2, y2] = s.rect;
        let mut content = doc.get_page_content(pid);
        content.extend_from_slice(
            format!(
                "\nq {:.4} 0 0 {:.4} {:.4} {:.4} cm /{} Do Q\n",
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
    use crate::editor;
    use crate::pdf_ops::{rebuild, PageSpec};
    use lopdf::content::{Content, Operation};

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
        apply_stamps(&mut doc, &[red_2x2()]).unwrap();
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
        let img_ref = xobjs.get(b"PES0").expect("PES0 missing").as_reference().unwrap();
        let stream = out.get_object(img_ref).unwrap().as_stream().unwrap();
        assert!(stream.dict.get(b"SMask").is_ok(), "image missing SMask");
    }

    #[test]
    fn shared_resources_stay_isolated() {
        let mut doc = sample_doc(2);
        apply_stamps(&mut doc, &[red_2x2()]).unwrap();
        let out = roundtrip(&mut doc);
        let pages = out.get_pages();

        let p2 = out.get_object(pages[&2]).unwrap().as_dict().unwrap();
        let res2 = resolved_dict(&out, &p2.get(b"Resources").unwrap().clone());
        let has = res2
            .get(b"XObject")
            .map(|o| resolved_dict(&out, &o.clone()).get(b"PES0").is_ok())
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
        apply_stamps(&mut doc, &[stamp]).unwrap();
        rebuild(&mut doc, &[PageSpec::page(1, 0)], &[], &[], &[]).unwrap();
        let out = roundtrip(&mut doc);
        assert_eq!(out.get_pages().len(), 1);
    }
}
