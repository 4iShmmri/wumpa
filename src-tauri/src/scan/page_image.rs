// Finds pages whose only content is one big image (a scan) and decodes it.
use image::{DynamicImage, GrayImage, ImageFormat, RgbImage};
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::common::{err, R};

pub struct PageImage {
    pub image: RgbImage,
    // Page size in points, as displayed (after /Rotate).
    pub width_pt: f64,
    pub height_pt: f64,
}

// The image must cover at least this share of the page.
const MIN_COVERAGE: f64 = 0.8;

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => doc.get_object(*r).unwrap_or(o),
        _ => o,
    }
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

// A page attribute, following the /Parent chain for inherited ones.
fn inherited<'a>(doc: &'a Document, page_id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut cur = Some(page_id);
    for _ in 0..64 {
        let dict = doc.get_object(cur?).ok()?.as_dict().ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(deref(doc, v));
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

fn media_box(doc: &Document, page_id: ObjectId) -> [f64; 4] {
    let b = inherited(doc, page_id, b"MediaBox")
        .and_then(|o| o.as_array().ok())
        .map(|a| {
            let g = |i: usize| a.get(i).map(|o| deref(doc, o)).and_then(num).unwrap_or(0.0);
            [g(0), g(1), g(2), g(3)]
        })
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    [
        b[0].min(b[2]),
        b[1].min(b[3]),
        b[0].max(b[2]),
        b[1].max(b[3]),
    ]
}

fn rotation(doc: &Document, page_id: ObjectId) -> i64 {
    crate::common::rotation_of(doc, inherited(doc, page_id, b"Rotate")).rem_euclid(360)
}

fn image_xobjects(doc: &Document, page_id: ObjectId) -> Vec<(Vec<u8>, ObjectId)> {
    let Some(res) = inherited(doc, page_id, b"Resources").and_then(|o| o.as_dict().ok()) else {
        return Vec::new();
    };
    let Some(xd) = res
        .get(b"XObject")
        .ok()
        .and_then(|o| deref(doc, o).as_dict().ok())
    else {
        return Vec::new();
    };
    xd.iter()
        .filter_map(|(name, val)| {
            let id = val.as_reference().ok()?;
            let stream = doc.get_object(id).ok()?.as_stream().ok()?;
            let subtype = stream.dict.get(b"Subtype").and_then(|o| o.as_name()).ok()?;
            (subtype == b"Image").then(|| (name.clone(), id))
        })
        .collect()
}

type Mat = [f64; 6];

fn mul(a: Mat, b: Mat) -> Mat {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn unit_square_bbox(m: Mat) -> [f64; 4] {
    let pts = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for (x, y) in pts {
        let px = m[0] * x + m[2] * y + m[4];
        let py = m[1] * x + m[3] * y + m[5];
        b[0] = b[0].min(px);
        b[1] = b[1].min(py);
        b[2] = b[2].max(px);
        b[3] = b[3].max(py);
    }
    b
}

// The single image XObject drawn on the page when the page draws nothing
// else of substance: no text, no other images, and the image covers most
// of the MediaBox.
fn locate(doc: &Document, page_id: ObjectId) -> Option<ObjectId> {
    let images = image_xobjects(doc, page_id);
    if images.is_empty() {
        return None;
    }
    let data = doc.get_page_content(page_id);
    let content = Content::decode(&data).ok()?;
    let mbox = media_box(doc, page_id);
    let page_area = (mbox[2] - mbox[0]) * (mbox[3] - mbox[1]);
    let mut ctm: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut stack = Vec::new();
    let mut found: Option<ObjectId> = None;
    for op in &content.operations {
        match op.operator.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(ctm),
            "cm" => {
                let g = |i: usize| op.operands.get(i).and_then(num).unwrap_or(0.0);
                ctm = mul([g(0), g(1), g(2), g(3), g(4), g(5)], ctm);
            }
            "Tj" | "TJ" | "'" | "\"" => return None,
            "Do" => {
                let name = op.operands.first().and_then(|o| o.as_name().ok())?;
                let Some((_, id)) = images.iter().find(|(n, _)| n == name) else {
                    // A form XObject: too opaque to call the page image-only.
                    return None;
                };
                if found.is_some() {
                    return None;
                }
                let b = unit_square_bbox(ctm);
                let ix1 = b[0].max(mbox[0]);
                let iy1 = b[1].max(mbox[1]);
                let ix2 = b[2].min(mbox[2]);
                let iy2 = b[3].min(mbox[3]);
                let area = (ix2 - ix1).max(0.0) * (iy2 - iy1).max(0.0);
                if page_area <= 0.0 || area < MIN_COVERAGE * page_area {
                    return None;
                }
                found = Some(*id);
            }
            _ => {}
        }
    }
    found
}

fn single_filter(dict: &Dictionary) -> Option<Vec<u8>> {
    match dict.get(b"Filter") {
        Err(_) => Some(Vec::new()),
        Ok(Object::Name(n)) => Some(n.clone()),
        Ok(Object::Array(a)) if a.is_empty() => Some(Vec::new()),
        Ok(Object::Array(a)) if a.len() == 1 => a[0].as_name().ok().map(|n| n.to_vec()),
        _ => None,
    }
}

fn dims(s: &Stream) -> Option<(u32, u32)> {
    let w = u32::try_from(s.dict.get(b"Width").ok()?.as_i64().ok()?).ok()?;
    let h = u32::try_from(s.dict.get(b"Height").ok()?.as_i64().ok()?).ok()?;
    (w > 0 && h > 0).then_some((w, h))
}

fn channels(doc: &Document, s: &Stream) -> Option<usize> {
    match s.dict.get(b"ColorSpace").map(|o| deref(doc, o)) {
        Ok(Object::Name(n)) if n == b"DeviceRGB" => Some(3),
        Ok(Object::Name(n)) if n == b"DeviceGray" => Some(1),
        _ => None,
    }
}

fn decode_image(doc: &Document, s: &Stream) -> Option<RgbImage> {
    if s.dict.has(b"ImageMask") || s.dict.has(b"Decode") {
        return None;
    }
    let (w, h) = dims(s)?;
    match single_filter(&s.dict)?.as_slice() {
        b"DCTDecode" => image::load_from_memory_with_format(&s.content, ImageFormat::Jpeg)
            .ok()
            .map(|i| i.to_rgb8()),
        b"FlateDecode" | b"" => {
            if s.dict
                .get(b"BitsPerComponent")
                .ok()
                .and_then(|o| o.as_i64().ok())
                != Some(8)
            {
                return None;
            }
            let ch = channels(doc, s)?;
            let raw = s.decompressed_content().ok()?;
            let expect = (w as usize).checked_mul(h as usize)?.checked_mul(ch)?;
            if raw.len() < expect {
                return None;
            }
            let raw = raw[..expect].to_vec();
            if ch == 3 {
                RgbImage::from_raw(w, h, raw)
            } else {
                GrayImage::from_raw(w, h, raw).map(|g| DynamicImage::ImageLuma8(g).to_rgb8())
            }
        }
        _ => None,
    }
}

fn decodable(doc: &Document, s: &Stream) -> bool {
    if s.dict.has(b"ImageMask") || s.dict.has(b"Decode") || dims(s).is_none() {
        return false;
    }
    match single_filter(&s.dict).as_deref() {
        Some(b"DCTDecode") => true,
        Some(b"FlateDecode") | Some(b"") => {
            s.dict
                .get(b"BitsPerComponent")
                .ok()
                .and_then(|o| o.as_i64().ok())
                == Some(8)
                && channels(doc, s).is_some()
        }
        _ => false,
    }
}

// Applies the page's /Rotate so the image is oriented as displayed.
fn oriented(img: RgbImage, rotate: i64) -> RgbImage {
    match rotate {
        90 => image::imageops::rotate90(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate270(&img),
        _ => img,
    }
}

pub fn page_image(doc: &Document, page_id: ObjectId) -> R<Option<PageImage>> {
    let Some(id) = locate(doc, page_id) else {
        return Ok(None);
    };
    let stream = doc
        .get_object(id)
        .and_then(|o| o.as_stream())
        .map_err(err)?;
    let Some(img) = decode_image(doc, stream) else {
        return Ok(None);
    };
    let rot = rotation(doc, page_id);
    let mbox = media_box(doc, page_id);
    let (mut w, mut h) = (mbox[2] - mbox[0], mbox[3] - mbox[1]);
    if rot == 90 || rot == 270 {
        std::mem::swap(&mut w, &mut h);
    }
    Ok(Some(PageImage {
        image: oriented(img, rot),
        width_pt: w,
        height_pt: h,
    }))
}

// 1-based numbers of the pages page_image would accept.
pub fn image_only_pages(doc: &Document) -> Vec<u32> {
    doc.get_pages()
        .iter()
        .filter(|(_, &id)| {
            locate(doc, id)
                .and_then(|img| doc.get_object(img).ok()?.as_stream().ok())
                .map(|s| decodable(doc, s))
                .unwrap_or(false)
        })
        .map(|(&n, _)| n)
        .collect()
}

// One-page documents whose page draws a single full-page image, for the
// module tests.
#[cfg(test)]
pub mod testing {
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, Stream};

    pub fn image_doc(stream: Stream, page_w: f64, page_h: f64, with_text: bool) -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let img_id = doc.add_object(stream);
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => img_id },
            "Font" => dictionary! { "F1" => font_id },
        });
        let mut operations = vec![
            Operation::new("q", vec![]),
            Operation::new(
                "cm",
                vec![
                    page_w.into(),
                    0.into(),
                    0.into(),
                    page_h.into(),
                    0.into(),
                    0.into(),
                ],
            ),
            Operation::new("Do", vec!["Im0".into()]),
            Operation::new("Q", vec![]),
        ];
        if with_text {
            operations.extend([
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Tj", vec![Object::string_literal("x")]),
                Operation::new("ET", vec![]),
            ]);
        }
        let content = Content { operations };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), page_w.into(), page_h.into()],
            "Resources" => resources_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    pub fn rgb_stream(img: &image::RgbImage) -> Stream {
        let mut s = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => img.width() as i64,
                "Height" => img.height() as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            img.as_raw().clone(),
        );
        s.compress().unwrap();
        s
    }

    pub fn jpeg_stream(jpeg: &[u8]) -> Stream {
        let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).unwrap();
        Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => img.width() as i64,
                "Height" => img.height() as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg.to_vec(),
        )
        .with_compression(false)
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{image_doc, rgb_stream};
    use super::*;

    fn first_page(doc: &Document) -> ObjectId {
        *doc.get_pages().values().next().unwrap()
    }

    #[test]
    fn decodes_flate_rgb_scan() {
        let img = RgbImage::from_pixel(40, 20, image::Rgb([200, 100, 50]));
        let doc = image_doc(rgb_stream(&img), 400.0, 200.0, false);
        assert_eq!(image_only_pages(&doc), vec![1]);
        let got = page_image(&doc, first_page(&doc)).unwrap().unwrap();
        assert_eq!(got.image.dimensions(), (40, 20));
        assert_eq!(got.image.get_pixel(3, 3).0, [200, 100, 50]);
        assert_eq!((got.width_pt, got.height_pt), (400.0, 200.0));
    }

    #[test]
    fn refuses_pages_with_text() {
        let img = RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0]));
        let doc = image_doc(rgb_stream(&img), 100.0, 100.0, true);
        assert!(image_only_pages(&doc).is_empty());
        assert!(page_image(&doc, first_page(&doc)).unwrap().is_none());
    }

    #[test]
    fn refuses_small_images() {
        let img = RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0]));
        // Drawn at page size 100x100 but the page is 400x400.
        let mut doc = image_doc(rgb_stream(&img), 100.0, 100.0, false);
        let page_id = first_page(&doc);
        let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
        page.set("MediaBox", vec![0.into(), 0.into(), 400.into(), 400.into()]);
        assert!(page_image(&doc, page_id).unwrap().is_none());
    }
}
