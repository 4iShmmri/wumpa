use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

type R<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct ObjDesc {
    pub id: u32,
    pub kind: &'static str,
    pub bbox: [f64; 4],
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize, Clone)]
pub struct ObjectEdit {
    pub src: u32,
    pub id: u32,
    pub action: String,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    #[serde(default = "one")]
    pub sx: f64,
    #[serde(default = "one")]
    pub sy: f64,
    #[serde(default)]
    pub ax: f64,
    #[serde(default)]
    pub ay: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Text,
    Image,
    Form,
    Path,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Form => "form",
            Kind::Path => "path",
        }
    }
}

pub struct PageObj {
    pub id: u32,
    pub kind: Kind,
    pub bbox: [f64; 4],
    pub op_range: (usize, usize),
    pub ctm: Mat,
}

impl From<&PageObj> for ObjDesc {
    fn from(o: &PageObj) -> Self {
        ObjDesc { id: o.id, kind: o.kind.as_str(), bbox: o.bbox }
    }
}

pub type Mat = [f64; 6];

const IDENT: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn compose(first: Mat, second: Mat) -> Mat {
    let [a1, b1, c1, d1, e1, f1] = first;
    let [a2, b2, c2, d2, e2, f2] = second;
    [
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    ]
}

fn apply(m: Mat, p: (f64, f64)) -> (f64, f64) {
    (p.0 * m[0] + p.1 * m[2] + m[4], p.0 * m[1] + p.1 * m[3] + m[5])
}

fn translate(tx: f64, ty: f64) -> Mat {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

fn scale_about(sx: f64, sy: f64, ax: f64, ay: f64) -> Mat {
    compose(
        compose(translate(-ax, -ay), [sx, 0.0, 0.0, sy, 0.0, 0.0]),
        translate(ax, ay),
    )
}

fn invert(m: Mat) -> Option<Mat> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b, c, d, e, f) = (m[0], m[1], m[2], m[3], m[4], m[5]);
    Some([
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ])
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

fn opnum(op: &Operation, i: usize) -> f64 {
    op.operands.get(i).and_then(num).unwrap_or(0.0)
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => doc.get_object(*r).unwrap_or(o),
        _ => o,
    }
}

enum WidthModel {
    Simple { first: u32, widths: Vec<f64>, missing: f64 },
    Cid { dw: f64, map: HashMap<u32, f64> },
    Flat(f64),
}

impl WidthModel {
    fn two_byte(&self) -> bool {
        matches!(self, WidthModel::Cid { .. })
    }

    fn advance(&self, code: u32) -> f64 {
        match self {
            WidthModel::Simple { first, widths, missing } => code
                .checked_sub(*first)
                .and_then(|i| widths.get(i as usize))
                .copied()
                .unwrap_or(*missing),
            WidthModel::Cid { dw, map } => map.get(&code).copied().unwrap_or(*dw),
            WidthModel::Flat(w) => *w,
        }
    }
}

fn font_width_model(doc: &Document, font: &Dictionary) -> WidthModel {
    let subtype = font.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"");
    if subtype == b"Type0" {
        let desc = font
            .get(b"DescendantFonts")
            .map(|o| deref(doc, o))
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first())
            .map(|o| deref(doc, o))
            .and_then(|o| o.as_dict().ok());
        let Some(desc) = desc else { return WidthModel::Flat(500.0) };
        let dw = desc.get(b"DW").ok().and_then(num).unwrap_or(1000.0);
        let mut map = HashMap::new();
        if let Ok(w) = desc.get(b"W").map(|o| deref(doc, o)) {
            if let Ok(arr) = w.as_array() {
                let mut i = 0;
                while i < arr.len() {
                    let Some(c1) = num(deref(doc, &arr[i])) else { break };
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
        .map(|a| a.iter().map(|w| num(deref(doc, w)).unwrap_or(500.0)).collect())
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
        WidthModel::Simple { first, widths, missing }
    }
}

fn resolve_dict(doc: &Document, o: &Object) -> Option<Dictionary> {
    deref(doc, o).as_dict().ok().cloned()
}

fn page_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let mut cur = Some(page_id);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else { break };
        if let Ok(res) = dict.get(b"Resources") {
            if let Some(d) = resolve_dict(doc, res) {
                return d;
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
    let Some(xd) = res.get(b"XObject").ok().and_then(|o| resolve_dict(doc, o)) else {
        return out;
    };
    for (name, val) in xd.iter() {
        let Ok(stream) = deref(doc, val).as_stream() else { continue };
        let subtype = stream.dict.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"");
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

fn load_fonts(doc: &Document, res: &Dictionary) -> HashMap<Vec<u8>, WidthModel> {
    let mut out = HashMap::new();
    let Some(fd) = res.get(b"Font").ok().and_then(|o| resolve_dict(doc, o)) else {
        return out;
    };
    for (name, val) in fd.iter() {
        let model = resolve_dict(doc, val)
            .map(|d| font_width_model(doc, &d))
            .unwrap_or(WidthModel::Flat(500.0));
        out.insert(name.clone(), model);
    }
    out
}

fn codes(bytes: &[u8], two_byte: bool) -> Vec<u32> {
    if two_byte {
        bytes.chunks(2).map(|c| {
            if c.len() == 2 { ((c[0] as u32) << 8) | c[1] as u32 } else { c[0] as u32 }
        }).collect()
    } else {
        bytes.iter().map(|&b| b as u32).collect()
    }
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
// Text/graphics state set inside a removed or wrapped range persists afterwards
// per spec, so those operators are re-emitted to keep trailing content intact.
const STICKY_STATE: [&str; 26] = [
    "Tf", "Tc", "Tw", "Tz", "TL", "Ts", "Tr", "gs", "w", "J", "j", "M", "d", "ri", "i",
    "g", "G", "rg", "RG", "k", "K", "cs", "CS", "sc", "scn", "SC",
];

pub fn enumerate(doc: &Document, page_id: ObjectId) -> R<Vec<PageObj>> {
    let data = doc.get_page_content(page_id);
    let content = Content::decode(&data).map_err(err)?;
    let res = page_resources(doc, page_id);
    let fonts = load_fonts(doc, &res);
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
        objs.push(PageObj { id, kind, bbox: bbox_of(pts), op_range: range, ctm });
    };

    for (i, op) in content.operations.iter().enumerate() {
        let o = op.operator.as_str();
        match o {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(IDENT),
            "cm" => {
                let m = [opnum(op, 0), opnum(op, 1), opnum(op, 2), opnum(op, 3), opnum(op, 4), opnum(op, 5)];
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
                cur_font = op.operands.first().and_then(|o| o.as_name().ok()).unwrap_or(b"").to_vec();
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
                tm = [opnum(op, 0), opnum(op, 1), opnum(op, 2), opnum(op, 3), opnum(op, 4), opnum(op, 5)];
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
                static FALLBACK: WidthModel = WidthModel::Flat(500.0);
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
                    text_pts.push(apply(m, (0.0, lo)));
                    text_pts.push(apply(m, (0.0, hi)));
                    for code in codes(bytes, model.two_byte()) {
                        let sp = if !model.two_byte() && code == 32 { tw } else { 0.0 };
                        let tx = ((model.advance(code) / 1000.0) * tfs + tc + sp) * th;
                        *tm = compose(translate(tx, 0.0), *tm);
                    }
                    let m = compose(*tm, ctm);
                    text_pts.push(apply(m, (0.0, lo)));
                    text_pts.push(apply(m, (0.0, hi)));
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
                        let b = bytes.clone();
                        show(&mut tm, &b);
                    }
                }
            }
            "Do" => {
                let name = op.operands.first().and_then(|o| o.as_name().ok()).unwrap_or(b"");
                match xobjects.get(name) {
                    Some(XKind::Image) => {
                        let pts: Vec<_> = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]
                            .iter()
                            .map(|&p| apply(ctm, p))
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
                        .map(|&p| apply(m, p))
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
                        path_pts.push(apply(ctm, p));
                    }
                } else {
                    for ch in coords.chunks(2) {
                        if ch.len() == 2 {
                            path_pts.push(apply(ctm, (ch[0], ch[1])));
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

struct Action {
    delete: bool,
    a: Mat,
    transformed: bool,
}

pub fn apply_edits(doc: &mut Document, edits: &[ObjectEdit]) -> R<()> {
    if edits.is_empty() {
        return Ok(());
    }
    let pages = doc.get_pages();
    let mut by_page: BTreeMap<u32, Vec<&ObjectEdit>> = BTreeMap::new();
    for e in edits {
        by_page.entry(e.src).or_default().push(e);
    }

    for (src, page_edits) in by_page {
        let &pid = pages.get(&src).ok_or_else(|| format!("page {src} not found"))?;
        let objs = enumerate(doc, pid)?;
        let by_id: HashMap<u32, &PageObj> = objs.iter().map(|o| (o.id, o)).collect();

        let mut acts: HashMap<u32, Action> = HashMap::new();
        for e in page_edits {
            let obj = by_id
                .get(&e.id)
                .ok_or_else(|| format!("object {} not found on page {src}", e.id))?;
            let act = acts
                .entry(obj.id)
                .or_insert(Action { delete: false, a: IDENT, transformed: false });
            match e.action.as_str() {
                "delete" => act.delete = true,
                "translate" => {
                    act.a = compose(act.a, translate(e.dx, e.dy));
                    act.transformed = true;
                }
                "scale" => {
                    act.a = compose(act.a, scale_about(e.sx, e.sy, e.ax, e.ay));
                    act.transformed = true;
                }
                other => return Err(format!("unknown action {other}")),
            }
        }

        let data = doc.get_page_content(pid);
        let content = Content::decode(&data).map_err(err)?;
        let n_ops = content.operations.len();
        if n_ops == 0 {
            continue;
        }

        let mut skip = vec![false; n_ops];
        let mut keep_state = vec![false; n_ops];
        let mut starts: HashMap<usize, Mat> = HashMap::new();
        let mut ends: std::collections::HashSet<usize> = Default::default();

        for (id, act) in &acts {
            let obj = by_id[id];
            let (s, e) = obj.op_range;
            if act.delete {
                for i in s..=e.min(n_ops - 1) {
                    skip[i] = true;
                    if obj.kind == Kind::Text {
                        keep_state[i] = true;
                    }
                }
            } else if act.transformed {
                let inv = invert(obj.ctm).ok_or("object transform not invertible")?;
                starts.insert(s, compose(compose(obj.ctm, act.a), inv));
                ends.insert(e);
            }
        }

        let mut out: Vec<Operation> = Vec::with_capacity(n_ops + acts.len() * 3);
        let mut restate: Vec<Operation> = Vec::new();
        let mut in_wrapped = false;
        for (i, op) in content.operations.into_iter().enumerate() {
            if let Some(m) = starts.get(&i) {
                out.push(Operation::new("q", vec![]));
                out.push(Operation::new(
                    "cm",
                    m.iter().map(|&v| Object::Real(v as f32)).collect(),
                ));
                in_wrapped = true;
                restate.clear();
            }
            let is_end = ends.contains(&i);
            if skip[i] {
                if keep_state[i] && STICKY_STATE.contains(&op.operator.as_str()) {
                    out.push(op);
                }
            } else {
                if in_wrapped && STICKY_STATE.contains(&op.operator.as_str()) {
                    restate.push(op.clone());
                }
                out.push(op);
            }
            if is_end {
                out.push(Operation::new("Q", vec![]));
                out.extend(restate.drain(..));
                in_wrapped = false;
            }
        }

        let encoded = Content { operations: out }.encode().map_err(err)?;
        doc.change_page_content(pid, encoded).map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    fn sample_doc() -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let img_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            vec![255, 0, 0],
        ));
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "Im0" => img_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal("Hello world")]),
                Operation::new("ET", vec![]),
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![100.into(), 0.into(), 0.into(), 50.into(), 100.into(), 600.into()],
                ),
                Operation::new("Do", vec!["Im0".into()]),
                Operation::new("Q", vec![]),
                Operation::new("re", vec![50.into(), 50.into(), 200.into(), 30.into()]),
                Operation::new("f", vec![]),
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
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
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

    fn page1(doc: &Document) -> ObjectId {
        doc.get_pages()[&1]
    }

    fn edit(id: u32, action: &str) -> ObjectEdit {
        ObjectEdit {
            src: 1,
            id,
            action: action.into(),
            dx: 0.0,
            dy: 0.0,
            sx: 1.0,
            sy: 1.0,
            ax: 0.0,
            ay: 0.0,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1.0
    }

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

    #[test]
    fn delete_removes_image() {
        let mut doc = sample_doc();
        let img_id = enumerate(&doc, page1(&doc))
            .unwrap()
            .iter()
            .find(|o| o.kind == Kind::Image)
            .unwrap()
            .id;
        apply_edits(&mut doc, &[edit(img_id, "delete")]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        assert!(objs.iter().all(|o| o.kind != Kind::Image));
        assert_eq!(objs.len(), 2);
    }

    #[test]
    fn translate_moves_text() {
        let mut doc = sample_doc();
        let before = enumerate(&doc, page1(&doc)).unwrap();
        let t = before.iter().find(|o| o.kind == Kind::Text).unwrap();
        let (tid, bb) = (t.id, t.bbox);
        let mut e = edit(tid, "translate");
        e.dx = 10.0;
        e.dy = 20.0;
        apply_edits(&mut doc, &[e]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        let t2 = objs.iter().find(|o| o.kind == Kind::Text).unwrap();
        for k in 0..4 {
            let want = bb[k] + if k % 2 == 0 { 10.0 } else { 20.0 };
            assert!(close(t2.bbox[k], want), "{:?} vs {:?}", t2.bbox, bb);
        }
    }

    #[test]
    fn scale_image_about_corner() {
        let mut doc = sample_doc();
        let img_id = enumerate(&doc, page1(&doc))
            .unwrap()
            .iter()
            .find(|o| o.kind == Kind::Image)
            .unwrap()
            .id;
        let mut e = edit(img_id, "scale");
        e.sx = 2.0;
        e.sy = 1.0;
        e.ax = 100.0;
        e.ay = 600.0;
        apply_edits(&mut doc, &[e]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        let img = objs.iter().find(|o| o.kind == Kind::Image).unwrap();
        for (got, want) in img.bbox.iter().zip([100.0, 600.0, 300.0, 650.0]) {
            assert!(close(*got, want), "scaled bbox {:?}", img.bbox);
        }
    }
}
