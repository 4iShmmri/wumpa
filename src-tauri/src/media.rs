use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn single_filter(dict: &Dictionary) -> Option<Vec<u8>> {
    match dict.get(b"Filter") {
        Ok(Object::Name(n)) => Some(n.clone()),
        Ok(Object::Array(a)) if a.len() == 1 => a[0].as_name().ok().map(|n| n.to_vec()),
        _ => None,
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Option<(Vec<u8>, u32, u32, Vec<u8>)> {
    let (w, h) = (img.width(), img.height());
    let mut buf = Vec::new();
    let gray = matches!(
        img.color(),
        image::ColorType::L8 | image::ColorType::La8 | image::ColorType::L16
    );
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    if gray {
        enc.encode_image(&img.to_luma8()).ok()?;
        Some((buf, w, h, b"DeviceGray".to_vec()))
    } else {
        enc.encode_image(&img.to_rgb8()).ok()?;
        Some((buf, w, h, b"DeviceRGB".to_vec()))
    }
}

fn fit(img: DynamicImage, max_dim: u32) -> DynamicImage {
    if img.width().max(img.height()) > max_dim {
        img.resize(max_dim, max_dim, FilterType::Lanczos3)
    } else {
        img
    }
}

fn plan_recompress(s: &Stream, quality: u8, max_dim: u32) -> Option<(Vec<u8>, u32, u32, Vec<u8>)> {
    if s.dict
        .get(b"ImageMask")
        .ok()
        .and_then(|o| o.as_bool().ok())
        .unwrap_or(false)
    {
        return None;
    }
    let w = s.dict.get(b"Width").ok()?.as_i64().ok()? as u32;
    let h = s.dict.get(b"Height").ok()?.as_i64().ok()? as u32;
    if w == 0 || h == 0 {
        return None;
    }
    match single_filter(&s.dict)?.as_slice() {
        b"DCTDecode" => {
            let cs_ok = match s.dict.get(b"ColorSpace") {
                Err(_) => true,
                Ok(Object::Name(n)) => {
                    matches!(n.as_slice(), b"DeviceRGB" | b"DeviceGray" | b"DeviceCMYK")
                }
                _ => false,
            };
            if !cs_ok {
                return None;
            }
            let img = image::load_from_memory_with_format(&s.content, ImageFormat::Jpeg).ok()?;
            encode_jpeg(&fit(img, max_dim), quality)
        }
        b"FlateDecode" => {
            if s.dict.has(b"SMask") || s.dict.has(b"Decode") || s.dict.has(b"DecodeParms") {
                return None;
            }
            if s.dict
                .get(b"BitsPerComponent")
                .ok()
                .and_then(|o| o.as_i64().ok())
                != Some(8)
            {
                return None;
            }
            let ch = match s.dict.get(b"ColorSpace") {
                Ok(Object::Name(n)) if n == b"DeviceRGB" => 3usize,
                Ok(Object::Name(n)) if n == b"DeviceGray" => 1usize,
                _ => return None,
            };
            let raw = s.decompressed_content().ok()?;
            if raw.len() != (w as usize) * (h as usize) * ch {
                return None;
            }
            let img = if ch == 3 {
                DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, raw)?)
            } else {
                DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, raw)?)
            };
            encode_jpeg(&fit(img, max_dim), quality)
        }
        _ => None,
    }
}

pub fn compress_document(doc: &mut Document, quality: u8, max_dim: u32) -> R<()> {
    let ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter_map(|(id, o)| match o {
            Object::Stream(s)
                if s.dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|v| v.as_name().ok())
                    .map(|n| n == b"Image")
                    .unwrap_or(false) =>
            {
                Some(*id)
            }
            _ => None,
        })
        .collect();

    for id in ids {
        let plan = match doc.get_object(id) {
            Ok(Object::Stream(s)) => plan_recompress(s, quality, max_dim),
            _ => None,
        };
        if let Some((bytes, w, h, cs)) = plan {
            if let Ok(Object::Stream(s)) = doc.get_object_mut(id) {
                if bytes.len() < s.content.len() {
                    s.dict.set("Width", w as i64);
                    s.dict.set("Height", h as i64);
                    s.dict.set("BitsPerComponent", 8);
                    s.dict.set("ColorSpace", Object::Name(cs));
                    s.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
                    s.dict.remove(b"DecodeParms");
                    s.dict.set("Length", bytes.len() as i64);
                    s.content = bytes;
                }
            }
        }
    }
    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    Ok(())
}

pub fn compress_file(src: &str, out: &str, quality: u8, max_dim: u32) -> R<[u64; 2]> {
    let orig = std::fs::metadata(src).map_err(err)?.len();
    let mut doc = Document::load(src).map_err(err)?;
    compress_document(&mut doc, quality.clamp(10, 95), max_dim.max(16))?;
    doc.save(out).map_err(err)?;
    let new = std::fs::metadata(out).map_err(err)?.len();
    Ok([orig, new])
}

pub fn blank_document(w: f64, h: f64, pages: u32) -> Document {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut kids: Vec<Object> = Vec::new();
    for _ in 0..pages {
        let content_id = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Resources" => Object::Dictionary(Dictionary::new()),
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

pub fn image_file_to_data_url(path: &str) -> R<String> {
    use base64::Engine;
    let bytes = std::fs::read(path).map_err(err)?;
    let fmt = image::guess_format(&bytes).map_err(|_| "unsupported image format".to_string())?;
    let (mime, payload) = match fmt {
        ImageFormat::Jpeg => {
            image::load_from_memory_with_format(&bytes, fmt).map_err(err)?;
            ("image/jpeg", bytes)
        }
        ImageFormat::Png => {
            image::load_from_memory_with_format(&bytes, fmt).map_err(err)?;
            ("image/png", bytes)
        }
        _ => {
            let img = image::load_from_memory(&bytes)
                .map_err(|_| "unsupported image format".to_string())?;
            let mut buf = std::io::Cursor::new(Vec::new());
            img.write_to(&mut buf, ImageFormat::Png).map_err(err)?;
            ("image/png", buf.into_inner())
        }
    };
    Ok(format!(
        "data:{};base64,{}",
        mime,
        base64::engine::general_purpose::STANDARD.encode(payload)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use std::io::Write;

    fn doc_with_flate_image(w: u32, h: u32) -> Document {
        let mut raw = Vec::with_capacity((w * h * 3) as usize);
        let mut x: u32 = 123456789;
        for _ in 0..w * h * 3 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            raw.push((x >> 16) as u8);
        }
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&raw).unwrap();
        let data = enc.finish().unwrap();

        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let img_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => w as i64,
                "Height" => h as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "FlateDecode",
            },
            data,
        ));
        let content = Content {
            operations: vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![200.into(), 0.into(), 0.into(), 200.into(), 50.into(), 500.into()],
                ),
                Operation::new("Do", vec![Object::Name(b"Im0".to_vec())]),
                Operation::new("Q", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let res_id = doc.add_object(dictionary! {
            "XObject" => dictionary! { "Im0" => img_id },
        });
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => res_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let cat = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", cat);
        doc
    }

    fn find_image(doc: &Document) -> &Stream {
        doc.objects
            .values()
            .find_map(|o| match o {
                Object::Stream(s)
                    if s.dict
                        .get(b"Subtype")
                        .ok()
                        .and_then(|v| v.as_name().ok())
                        .map(|n| n == b"Image")
                        .unwrap_or(false) =>
                {
                    Some(s)
                }
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn compress_shrinks_and_resizes() {
        let mut doc = doc_with_flate_image(64, 64);
        let dir = std::env::temp_dir();
        let src = dir.join(format!("wumpa-cmp-src-{}.pdf", std::process::id()));
        let out = dir.join(format!("wumpa-cmp-out-{}.pdf", std::process::id()));
        doc.save(&src).unwrap();
        let [o, n] = compress_file(
            src.to_str().unwrap(),
            out.to_str().unwrap(),
            60,
            32,
        )
        .unwrap();
        assert!(n < o, "expected {n} < {o}");
        let re = Document::load(&out).unwrap();
        let img = find_image(&re);
        assert_eq!(img.dict.get(b"Width").unwrap().as_i64().unwrap(), 32);
        assert_eq!(
            img.dict.get(b"Filter").unwrap().as_name().unwrap(),
            b"DCTDecode"
        );
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn blank_document_has_pages() {
        let mut doc = blank_document(300.0, 400.0, 3);
        let mut buf = Vec::new();
        doc.save_to(&mut buf).unwrap();
        let re = Document::load_mem(&buf).unwrap();
        let pages = re.get_pages();
        assert_eq!(pages.len(), 3);
        let p = re.get_object(pages[&2]).unwrap().as_dict().unwrap();
        let mb = p.get(b"MediaBox").unwrap().as_array().unwrap();
        assert_eq!(mb[2].as_float().unwrap() as i64, 300);
        assert_eq!(mb[3].as_float().unwrap() as i64, 400);
    }

    #[test]
    fn image_data_url_roundtrip_and_reject() {
        let dir = std::env::temp_dir();
        let png = dir.join(format!("wumpa-img-{}.png", std::process::id()));
        image::RgbImage::from_pixel(4, 4, image::Rgb([10, 200, 30]))
            .save(&png)
            .unwrap();
        let url = image_file_to_data_url(png.to_str().unwrap()).unwrap();
        assert!(url.starts_with("data:image/png;base64,"));
        let txt = dir.join(format!("wumpa-img-{}.txt", std::process::id()));
        std::fs::write(&txt, b"hello").unwrap();
        assert!(image_file_to_data_url(txt.to_str().unwrap()).is_err());
        let _ = std::fs::remove_file(&png);
        let _ = std::fs::remove_file(&txt);
    }
}
