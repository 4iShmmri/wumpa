// Glyph advance widths, enough to measure a text run's extent.
use lopdf::{Dictionary, Document, Object};
use std::collections::HashMap;

use super::{deref, num};

pub enum WidthModel {
    Simple {
        first: u32,
        widths: Vec<f64>,
        missing: f64,
    },
    Cid {
        dw: f64,
        map: HashMap<u32, f64>,
    },
    Flat(f64),
}

pub static FALLBACK: WidthModel = WidthModel::Flat(500.0);

impl WidthModel {
    pub fn two_byte(&self) -> bool {
        matches!(self, WidthModel::Cid { .. })
    }

    pub fn advance(&self, code: u32) -> f64 {
        match self {
            WidthModel::Simple {
                first,
                widths,
                missing,
            } => code
                .checked_sub(*first)
                .and_then(|i| widths.get(i as usize))
                .copied()
                .unwrap_or(*missing),
            WidthModel::Cid { dw, map } => map.get(&code).copied().unwrap_or(*dw),
            WidthModel::Flat(w) => *w,
        }
    }
}

pub fn width_model(doc: &Document, font: &Dictionary) -> WidthModel {
    let subtype = font
        .get(b"Subtype")
        .and_then(|o| o.as_name())
        .unwrap_or(b"");
    if subtype == b"Type0" {
        let desc = font
            .get(b"DescendantFonts")
            .map(|o| deref(doc, o))
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first())
            .map(|o| deref(doc, o))
            .and_then(|o| o.as_dict().ok());
        let Some(desc) = desc else {
            return WidthModel::Flat(500.0);
        };
        let dw = desc.get(b"DW").ok().and_then(num).unwrap_or(1000.0);
        let mut map = HashMap::new();
        if let Ok(w) = desc.get(b"W").map(|o| deref(doc, o)) {
            if let Ok(arr) = w.as_array() {
                let mut i = 0;
                while i < arr.len() {
                    let Some(c1) = num(deref(doc, &arr[i])) else {
                        break;
                    };
                    match arr.get(i + 1).map(|o| deref(doc, o)) {
                        Some(Object::Array(ws)) => {
                            for (k, wv) in ws.iter().enumerate() {
                                if let Some(w) = num(deref(doc, wv)) {
                                    map.insert(c1 as u32 + k as u32, w);
                                }
                            }
                            i += 2;
                        }
                        Some(o2) => {
                            let (Some(c2), Some(w)) =
                                (num(o2), arr.get(i + 2).and_then(|o| num(deref(doc, o))))
                            else {
                                break;
                            };
                            for c in c1 as u32..=c2 as u32 {
                                map.insert(c, w);
                            }
                            i += 3;
                        }
                        None => break,
                    }
                }
            }
        }
        return WidthModel::Cid { dw, map };
    }
    let first = font.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as u32;
    let widths: Vec<f64> = font
        .get(b"Widths")
        .map(|o| deref(doc, o))
        .ok()
        .and_then(|o| o.as_array().ok())
        .map(|a| {
            a.iter()
                .map(|w| num(deref(doc, w)).unwrap_or(500.0))
                .collect()
        })
        .unwrap_or_default();
    let missing = font
        .get(b"FontDescriptor")
        .map(|o| deref(doc, o))
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"MissingWidth").ok().and_then(num))
        .unwrap_or(500.0);
    if widths.is_empty() {
        WidthModel::Flat(missing)
    } else {
        WidthModel::Simple {
            first,
            widths,
            missing,
        }
    }
}

pub fn load_fonts(doc: &Document, res: &Dictionary) -> HashMap<Vec<u8>, WidthModel> {
    let mut out = HashMap::new();
    let Some(fd) = res
        .get(b"Font")
        .ok()
        .and_then(|o| deref(doc, o).as_dict().ok())
    else {
        return out;
    };
    for (name, val) in fd.iter() {
        let model = deref(doc, val)
            .as_dict()
            .map(|d| width_model(doc, d))
            .unwrap_or(WidthModel::Flat(500.0));
        out.insert(name.clone(), model);
    }
    out
}

// Splits a string operand into character codes; CID fonts use two bytes.
pub fn codes(bytes: &[u8], two_byte: bool) -> Vec<u32> {
    if two_byte {
        bytes
            .chunks(2)
            .map(|c| {
                if c.len() == 2 {
                    ((c[0] as u32) << 8) | c[1] as u32
                } else {
                    c[0] as u32
                }
            })
            .collect()
    } else {
        bytes.iter().map(|&b| b as u32).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn cid_width_ranges_and_lists() {
        let doc = Document::with_version("1.7");
        let desc = dictionary! {
            "DW" => 700,
            "W" => vec![
                Object::Integer(1), Object::Array(vec![100.into(), 200.into()]),
                Object::Integer(10), Object::Integer(12), Object::Integer(300),
            ],
        };
        let font = dictionary! {
            "Subtype" => "Type0",
            "DescendantFonts" => vec![Object::Dictionary(desc)],
        };
        let m = width_model(&doc, &font);
        assert!(m.two_byte());
        assert_eq!(m.advance(1), 100.0);
        assert_eq!(m.advance(2), 200.0);
        assert_eq!(m.advance(11), 300.0);
        assert_eq!(m.advance(99), 700.0);
        assert_eq!(codes(&[1, 2, 3], true), vec![0x0102, 3]);
    }

    #[test]
    fn simple_widths_fall_back_to_missing_width() {
        let doc = Document::with_version("1.7");
        let font = dictionary! {
            "Subtype" => "TrueType",
            "FirstChar" => 65,
            "Widths" => vec![600.into(), 650.into()],
            "FontDescriptor" => dictionary! { "MissingWidth" => 250 },
        };
        let m = width_model(&doc, &font);
        assert!(!m.two_byte());
        assert_eq!(m.advance(66), 650.0);
        assert_eq!(m.advance(32), 250.0);
    }
}
