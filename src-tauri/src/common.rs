// Helpers shared by every core module.
use flate2::write::ZlibEncoder;
use flate2::Compression;
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use std::io::Write;

pub type R<T> = Result<T, String>;

pub fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub const PROTECTED: &str = "file is password-protected; remove its password first";

// Loads a document for rewriting. lopdf silently returns an object-less
// document for files it cannot decrypt and auto-decrypts owner-only files,
// so both cases are refused here instead of producing a broken or
// silently unprotected output.
pub fn load_plain(path: &str) -> R<Document> {
    let doc = Document::load(path).map_err(|e| format!("{path}: {e}"))?;
    if doc.is_encrypted() || doc.encryption_state.is_some() {
        return Err(PROTECTED.into());
    }
    Ok(doc)
}

pub fn catalog_id(doc: &Document) -> R<ObjectId> {
    doc.trailer
        .get(b"Root")
        .and_then(|o| o.as_reference())
        .map_err(err)
}

// The page's /Annots as a plain array, following an indirect reference.
pub fn page_annots(doc: &Document, page_id: ObjectId) -> Vec<Object> {
    let Ok(page) = doc.get_object(page_id).and_then(|o| o.as_dict()) else {
        return Vec::new();
    };
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
}

// /Rotate may be an integer, a real or a reference to either.
pub fn rotation_of(doc: &Document, obj: Option<&Object>) -> i64 {
    let Some(obj) = obj else { return 0 };
    let value = match doc.dereference(obj) {
        Ok((_, o)) => o,
        Err(_) => obj,
    };
    value
        .as_i64()
        .or_else(|_| value.as_float().map(|f| f.round() as i64))
        .unwrap_or(0)
}

// Makes sure the page has a /Contents stream so content can be appended.
pub fn ensure_contents(doc: &mut Document, page_id: ObjectId) -> R<()> {
    let has = doc
        .get_object(page_id)
        .and_then(|o| o.as_dict())
        .map(|d| d.get(b"Contents").is_ok())
        .map_err(err)?;
    if !has {
        let stream = doc.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
        let page = doc
            .get_object_mut(page_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        page.set("Contents", Object::Reference(stream));
    }
    Ok(())
}

pub fn deflate(data: &[u8]) -> R<Vec<u8>> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(data).map_err(err)?;
    enc.finish().map_err(err)
}

// A photograph has many colours; line art, logos and scans of plain text
// have few. Sampled, so it is cheap on big images.
pub fn photo_like(rgb: &image::RgbImage) -> bool {
    let mut colours = std::collections::HashSet::new();
    for p in rgb.pixels().step_by(97) {
        colours.insert((p[0] >> 3, p[1] >> 3, p[2] >> 3));
        if colours.len() > 512 {
            return true;
        }
    }
    false
}

// JPEG bytes of an RGB image, for photographs.
pub fn jpeg_bytes(rgb: &image::RgbImage, quality: u8) -> R<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode_image(rgb).map_err(err)?;
    Ok(out.into_inner())
}

// An RGB image XObject holding JPEG data as it is.
pub fn jpeg_xobject(doc: &mut Document, w: u32, h: u32, jpeg: Vec<u8>) -> ObjectId {
    doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "DCTDecode",
        },
        jpeg,
    ))
}

// An RGB image XObject with a soft mask built from the alpha channel.
pub fn rgba_xobject(doc: &mut Document, w: u32, h: u32, rgba: &[u8]) -> R<ObjectId> {
    let expect = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("image too large")?;
    if rgba.len() != expect {
        return Err(format!(
            "image buffer is {} bytes, expected {}",
            rgba.len(),
            expect
        ));
    }
    let mut rgb = Vec::with_capacity(expect / 4 * 3);
    let mut alpha = Vec::with_capacity(expect / 4);
    for px in rgba.as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
        alpha.push(px[3]);
    }
    let smask_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
        },
        deflate(&alpha)?,
    ));
    Ok(doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => w as i64,
            "Height" => h as i64,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "Filter" => "FlateDecode",
            "SMask" => smask_id,
        },
        deflate(&rgb)?,
    )))
}

// Fixtures shared by the module tests: a minimal n-page document with a
// single Helvetica font and a save/load round trip.
#[cfg(test)]
pub mod testing {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    pub fn sample_doc(pages: usize) -> Document {
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
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
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

    pub fn roundtrip(doc: &mut Document) -> Document {
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        Document::load_mem(&buf).unwrap()
    }
}
