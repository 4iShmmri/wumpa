// Walks a page's content stream and lists the objects drawn on it, in order,
// so the ids are stable between calls on the same content.
use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;

use super::fonts::{self, FALLBACK};
use super::mat::{compose, transform, translate, Mat, IDENT};
use super::{deref, num, opnum, Kind, PageObj};
use crate::common::{err, R};

fn page_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let mut cur = Some(page_id);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else {
            break;
        };
        if let Ok(res) = dict.get(b"Resources") {
            if let Ok(d) = deref(doc, res).as_dict() {
                return d.clone();
            }
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    Dictionary::new()
}

enum XKind {
    Image,
    Form { bbox: [f64; 4], matrix: Mat },
}

fn load_xobjects(doc: &Document, res: &Dictionary) -> HashMap<Vec<u8>, XKind> {
    let mut out = HashMap::new();
    let Some(xd) = res
        .get(b"XObject")
        .ok()
        .and_then(|o| deref(doc, o).as_dict().ok())
    else {
        return out;
    };
    for (name, val) in xd.iter() {
        let Ok(stream) = deref(doc, val).as_stream() else {
            continue;
        };
        let subtype = stream
            .dict
            .get(b"Subtype")
            .and_then(|o| o.as_name())
            .unwrap_or(b"");
        if subtype == b"Image" {
            out.insert(name.clone(), XKind::Image);
        } else if subtype == b"Form" {
            let bbox = stream
                .dict
                .get(b"BBox")
                .map(|o| deref(doc, o))
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    let g = |i: usize| a.get(i).and_then(num).unwrap_or(0.0);
                    [g(0), g(1), g(2), g(3)]
                })
                .unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let matrix = stream
                .dict
                .get(b"Matrix")
                .map(|o| deref(doc, o))
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| {
                    let g = |i: usize| a.get(i).and_then(num).unwrap_or(0.0);
                    [g(0), g(1), g(2), g(3), g(4), g(5)]
                })
                .unwrap_or(IDENT);
            out.insert(name.clone(), XKind::Form { bbox, matrix });
        }
    }
    out
}

fn bbox_of(pts: &[(f64, f64)]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for &(x, y) in pts {
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
    b
}

const PATH_PAINT: [&str; 9] = ["S", "s", "f", "F", "f*", "B", "B*", "b", "b*"];

pub fn enumerate(doc: &Document, page_id: ObjectId) -> R<Vec<PageObj>> {
    let data = doc.get_page_content(page_id);
    let content = Content::decode(&data).map_err(err)?;
    let res = page_resources(doc, page_id);
    let fonts = fonts::load_fonts(doc, &res);
    let xobjects = load_xobjects(doc, &res);

    let mut objs: Vec<PageObj> = Vec::new();
    let mut ctm = IDENT;
    let mut stack: Vec<Mat> = Vec::new();

    let mut tm = IDENT;
    let mut tlm = IDENT;
    let (mut tfs, mut th, mut tc, mut tw, mut tl, mut ts) = (0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
    let mut cur_font: Vec<u8> = Vec::new();
    let mut text_start = 0usize;
    let mut text_pts: Vec<(f64, f64)> = Vec::new();
    let mut text_ctm = IDENT;

    let mut path_start: Option<usize> = None;
    let mut path_pts: Vec<(f64, f64)> = Vec::new();
    let mut path_clip = false;
    let mut path_ctm = IDENT;

    let push = |objs: &mut Vec<PageObj>, kind, pts: &[(f64, f64)], range, ctm| {
        if pts.is_empty() {
            return;
        }
        let id = objs.len() as u32;
        objs.push(PageObj {
            id,
            kind,
            bbox: bbox_of(pts),
            op_range: range,
            ctm,
        });
    };

    for (i, op) in content.operations.iter().enumerate() {
        let o = op.operator.as_str();
        match o {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(IDENT),
            "cm" => {
                let m = [
                    opnum(op, 0),
                    opnum(op, 1),
                    opnum(op, 2),
                    opnum(op, 3),
                    opnum(op, 4),
                    opnum(op, 5),
                ];
                ctm = compose(m, ctm);
            }
            "BT" => {
                tm = IDENT;
                tlm = IDENT;
                text_start = i;
                text_pts.clear();
                text_ctm = ctm;
            }
            "ET" => push(&mut objs, Kind::Text, &text_pts, (text_start, i), text_ctm),
            "Tf" => {
                cur_font = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .unwrap_or(b"")
                    .to_vec();
                tfs = opnum(op, 1);
            }
            "Td" | "TD" => {
                let (tx, ty) = (opnum(op, 0), opnum(op, 1));
                if o == "TD" {
                    tl = -ty;
                }
                tlm = compose(translate(tx, ty), tlm);
                tm = tlm;
            }
            "Tm" => {
                tm = [
                    opnum(op, 0),
                    opnum(op, 1),
                    opnum(op, 2),
                    opnum(op, 3),
                    opnum(op, 4),
                    opnum(op, 5),
                ];
                tlm = tm;
            }
            "T*" => {
                tlm = compose(translate(0.0, -tl), tlm);
                tm = tlm;
            }
            "TL" => tl = opnum(op, 0),
            "Tc" => tc = opnum(op, 0),
            "Tw" => tw = opnum(op, 0),
            "Tz" => th = opnum(op, 0) / 100.0,
            "Ts" => ts = opnum(op, 0),
            "Tj" | "'" | "\"" | "TJ" => {
                let model = fonts.get(&cur_font).unwrap_or(&FALLBACK);
                if o == "\"" {
                    tw = opnum(op, 0);
                    tc = opnum(op, 1);
                }
                if o == "'" || o == "\"" {
                    tlm = compose(translate(0.0, -tl), tlm);
                    tm = tlm;
                }
                let mut show = |tm: &mut Mat, bytes: &[u8]| {
                    let lo = ts - 0.25 * tfs;
                    let hi = ts + 0.85 * tfs;
                    let m = compose(*tm, ctm);
                    text_pts.push(transform(m, (0.0, lo)));
                    text_pts.push(transform(m, (0.0, hi)));
                    for code in fonts::codes(bytes, model.two_byte()) {
                        let sp = if !model.two_byte() && code == 32 {
                            tw
                        } else {
                            0.0
                        };
                        let tx = ((model.advance(code) / 1000.0) * tfs + tc + sp) * th;
                        *tm = compose(translate(tx, 0.0), *tm);
                    }
                    let m = compose(*tm, ctm);
                    text_pts.push(transform(m, (0.0, lo)));
                    text_pts.push(transform(m, (0.0, hi)));
                };
                if o == "TJ" {
                    if let Some(Object::Array(arr)) = op.operands.first() {
                        for el in arr {
                            match el {
                                Object::String(bytes, _) => show(&mut tm, bytes),
                                _ => {
                                    if let Some(n) = num(el) {
                                        let tx = -n / 1000.0 * tfs * th;
                                        tm = compose(translate(tx, 0.0), tm);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    let idx = if o == "\"" { 2 } else { 0 };
                    if let Some(Object::String(bytes, _)) = op.operands.get(idx) {
                        show(&mut tm, bytes);
                    }
                }
            }
            "Do" => {
                let name = op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .unwrap_or(b"");
                match xobjects.get(name) {
                    Some(XKind::Image) => {
                        let pts: Vec<_> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                            .iter()
                            .map(|&p| transform(ctm, p))
                            .collect();
                        push(&mut objs, Kind::Image, &pts, (i, i), ctm);
                    }
                    Some(XKind::Form { bbox, matrix }) => {
                        let m = compose(*matrix, ctm);
                        let pts: Vec<_> = [
                            (bbox[0], bbox[1]),
                            (bbox[2], bbox[1]),
                            (bbox[0], bbox[3]),
                            (bbox[2], bbox[3]),
                        ]
                        .iter()
                        .map(|&p| transform(m, p))
                        .collect();
                        push(&mut objs, Kind::Form, &pts, (i, i), ctm);
                    }
                    None => {}
                }
            }
            "m" | "l" | "c" | "v" | "y" | "re" => {
                if path_start.is_none() {
                    path_start = Some(i);
                    path_pts.clear();
                    path_clip = false;
                    path_ctm = ctm;
                }
                let coords: Vec<f64> = op.operands.iter().filter_map(num).collect();
                if o == "re" && coords.len() == 4 {
                    let (x, y, w, h) = (coords[0], coords[1], coords[2], coords[3]);
                    for p in [(x, y), (x + w, y), (x, y + h), (x + w, y + h)] {
                        path_pts.push(transform(ctm, p));
                    }
                } else {
                    for ch in coords.chunks(2) {
                        if ch.len() == 2 {
                            path_pts.push(transform(ctm, (ch[0], ch[1])));
                        }
                    }
                }
            }
            "W" | "W*" => path_clip = true,
            "n" => {
                path_start = None;
                path_pts.clear();
                path_clip = false;
            }
            _ if PATH_PAINT.contains(&o) => {
                if let Some(s) = path_start {
                    if !path_clip {
                        push(&mut objs, Kind::Path, &path_pts, (s, i), path_ctm);
                    }
                }
                path_start = None;
                path_pts.clear();
                path_clip = false;
            }
            _ => {}
        }
    }
    Ok(objs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::{close, page1, sample_doc};

    #[test]
    fn enumerate_is_stable() {
        let doc = sample_doc();
        let a = enumerate(&doc, page1(&doc)).unwrap();
        let b = enumerate(&doc, page1(&doc)).unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.id, y.id);
            assert!(x.kind == y.kind);
            assert_eq!(x.bbox, y.bbox);
        }
        let img = a.iter().find(|o| o.kind == Kind::Image).unwrap();
        for (got, want) in img.bbox.iter().zip([100.0, 600.0, 200.0, 650.0]) {
            assert!(close(*got, want), "image bbox {:?}", img.bbox);
        }
    }
}
