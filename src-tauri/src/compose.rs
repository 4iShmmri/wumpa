// Page composer: builds real PDF pages, with selectable and searchable
// text, from a structured description of a scanned page.
mod fonts;
mod outline;
mod shape;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use serde::Deserialize;

use crate::common::{catalog_id, err, rgba_xobject, R};
use fonts::Fonts;
use shape::{shape_line, Dir, Line};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBlocks {
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub blocks: Vec<Block>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Block {
    Text(TextBlock),
    Rule(Rule),
    Rect(RectBlock),
    Image(ImageBlock),
    Table(Table),
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

fn black() -> [f64; 3] {
    [0.0, 0.0, 0.0]
}

fn one() -> f64 {
    1.0
}

fn half() -> f64 {
    0.5
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextBlock {
    pub bbox: [f64; 4],
    pub text: String,
    pub size: Option<f64>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub dir: Dir,
    #[serde(default = "black")]
    pub color: [f64; 3],
    // The box hugs the glyph ink: with a size, the ink top sits at the box top.
    #[serde(default)]
    pub tight: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    #[serde(default = "one")]
    pub width: f64,
    #[serde(default = "black")]
    pub color: [f64; 3],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RectBlock {
    pub bbox: [f64; 4],
    pub stroke: Option<f64>,
    pub fill: Option<[f64; 3]>,
    #[serde(default = "black")]
    pub color: [f64; 3],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageBlock {
    pub bbox: [f64; 4],
    pub png_b64: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub bbox: [f64; 4],
    pub rows: Vec<Vec<Cell>>,
    #[serde(default = "half")]
    pub border: f64,
    #[serde(default = "black")]
    pub color: [f64; 3],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub bbox: [f64; 4],
    pub text: String,
    pub size: Option<f64>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub dir: Dir,
    #[serde(default = "black")]
    pub color: [f64; 3],
}

// Where a shaped line lands on the page, in points.
struct Placement {
    size: f64,
    x: f64,
    y: f64,
    word_gap: f64,
}

// Colours are 0..1 floats; anything above 1 is taken as 0..255.
fn rgb(c: [f64; 3]) -> [f64; 3] {
    if c.iter().any(|&v| v > 1.0) {
        c.map(|v| (v / 255.0).clamp(0.0, 1.0))
    } else {
        c.map(|v| v.clamp(0.0, 1.0))
    }
}

// [left, bottom, right, top] regardless of the order the corners came in.
fn norm(b: [f64; 4]) -> [f64; 4] {
    [
        b[0].min(b[2]),
        b[1].min(b[3]),
        b[0].max(b[2]),
        b[1].max(b[3]),
    ]
}

fn op(name: &str, args: Vec<Object>) -> Operation {
    Operation::new(name, args)
}

fn nums(v: &[f64]) -> Vec<Object> {
    v.iter().map(|&n| Object::Real(n as f32)).collect()
}

const ITALIC_SKEW: f64 = 0.21;

// Picks a size that fits the box, then the baseline and start position.
// A given size keeps the font's ascent under the top edge, or the ink's
// top when the box is tight; a derived size makes the ink span the box
// exactly, as it did on the scan.
fn fit_line(
    line: &Line,
    size: Option<f64>,
    tight: bool,
    bbox: [f64; 4],
    top: f64,
    line_h: f64,
    align: Align,
) -> Placement {
    let [x1, _, x2, _] = bbox;
    let (ink_lo, ink_hi) = line.ink().unwrap_or((line.descent, line.ascent));
    let (mut size, ink_top) = match size {
        Some(s) if tight => (s, ink_hi),
        Some(s) => (s, line.ascent),
        None => (line_h * 1000.0 / (ink_hi - ink_lo).max(1.0), ink_hi),
    };
    let box_w = x2 - x1;
    let mut width = line.width() * size / 1000.0;
    if width > box_w && width > 0.0 {
        size *= box_w / width;
        width = box_w;
    }
    let slack = box_w - width;
    let spaces = line.spaces();
    let (x, word_gap) = match align {
        Align::Start => (x1, 0.0),
        Align::End => (x2 - width, 0.0),
        Align::Center => (x1 + slack / 2.0, 0.0),
        Align::Justify if spaces > 0 && slack > 0.0 => (x1, slack / spaces as f64),
        Align::Justify => (x1, 0.0),
    };
    Placement {
        size,
        x,
        y: top - ink_top * size / 1000.0,
        word_gap,
    }
}

struct TextSpec<'a> {
    bbox: [f64; 4],
    text: &'a str,
    size: Option<f64>,
    bold: bool,
    italic: bool,
    align: Align,
    dir: Dir,
    color: [f64; 3],
    tight: bool,
    // Centre the lines vertically in the box instead of starting at the top.
    middle: bool,
}

impl<'a> From<&'a TextBlock> for TextSpec<'a> {
    fn from(t: &'a TextBlock) -> Self {
        TextSpec {
            bbox: t.bbox,
            text: &t.text,
            size: t.size,
            bold: t.bold,
            italic: t.italic,
            align: t.align,
            dir: t.dir,
            color: t.color,
            tight: t.tight,
            middle: false,
        }
    }
}

#[derive(Default)]
pub struct Composer {
    fonts: Fonts,
    images: usize,
}

impl Composer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn page(&mut self, doc: &mut Document, page: &PageBlocks) -> R<ObjectId> {
        let mut ops = Vec::new();
        let mut xobjects = Dictionary::new();
        for block in &page.blocks {
            match block {
                Block::Text(t) => self.text(doc, &mut ops, &TextSpec::from(t))?,
                Block::Rule(r) => rule(&mut ops, r),
                Block::Rect(r) => rect(&mut ops, r),
                Block::Image(i) => self.image(doc, &mut ops, &mut xobjects, i)?,
                Block::Table(t) => self.table(doc, &mut ops, t)?,
            }
        }
        let content = Content { operations: ops }.encode().map_err(err)?;
        let content_id = doc.add_object(Stream::new(dictionary! {}, content));
        let mut font_dict = Dictionary::new();
        for (name, id) in self.fonts.resource_entries() {
            font_dict.set(name, Object::Reference(id));
        }
        let page_dict = dictionary! {
            "Type" => "Page",
            "MediaBox" => nums(&[0.0, 0.0, page.width, page.height]),
            "Contents" => content_id,
            "Resources" => dictionary! { "Font" => font_dict, "XObject" => xobjects },
        };
        append_page(doc, page_dict)
    }

    // Writes the fonts; must run once after the last page.
    pub fn finish(self, doc: &mut Document) -> R<()> {
        self.fonts.embed(doc)
    }

    fn text(&mut self, doc: &mut Document, ops: &mut Vec<Operation>, t: &TextSpec) -> R<()> {
        let bbox = norm(t.bbox);
        let lines: Vec<&str> = t
            .text
            .split('\n')
            .map(|l| l.trim_end_matches('\r'))
            .collect();
        let mut shaped: Vec<(usize, Line)> = Vec::new();
        for (i, raw) in lines.iter().enumerate() {
            let clean: String = raw
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect();
            if clean.trim().is_empty() {
                continue;
            }
            shaped.push((i, shape_line(&mut self.fonts, doc, &clean, t.bold, t.dir)?));
        }
        let n = lines.len() as f64;
        let mut line_h = (bbox[3] - bbox[1]) / n;
        // Centred lines: the ink of each line sits on the centre of its slot.
        let middle = match (t.middle, t.size) {
            (true, Some(size)) => {
                let extent = shaped
                    .iter()
                    .map(|(_, l)| l.ascent - l.descent)
                    .fold(0.0, f64::max);
                line_h = line_h.min(extent * size / 1000.0);
                Some(size)
            }
            _ => None,
        };
        let cy = (bbox[1] + bbox[3]) / 2.0;
        for (i, line) in &shaped {
            let top = match middle {
                Some(size) => {
                    let slot = cy + ((n - 1.0) / 2.0 - *i as f64) * line_h;
                    let (lo, hi) = line.ink().unwrap_or((line.descent, line.ascent));
                    slot + (line.ascent - (lo + hi) / 2.0) * size / 1000.0
                }
                None => bbox[3] - *i as f64 * line_h,
            };
            let place = fit_line(line, t.size, t.tight, bbox, top, line_h, t.align);
            self.emit_line(doc, ops, line, &place, t.italic, t.color)?;
        }
        Ok(())
    }

    // One text object per line. Spaces are folded into the text and width
    // of the glyph before them, each advancing CID carries the distance to
    // the next advancing glyph, and zero-width marks sit on a detour of
    // kerning numbers, so the pen never jumps where extractors would read
    // a word break. Glyphs without text are painted as outlines.
    fn emit_line(
        &mut self,
        doc: &mut Document,
        ops: &mut Vec<Operation>,
        line: &Line,
        p: &Placement,
        italic: bool,
        color: [f64; 3],
    ) -> R<()> {
        let skew = if italic { ITALIC_SKEW } else { 0.0 };
        let gap = p.word_gap * 1000.0 / p.size;
        let mut origins = Vec::with_capacity(line.glyphs.len());
        let mut pen = 0.0;
        for g in &line.glyphs {
            origins.push(pen + g.dx);
            pen += g.advance + if g.space { gap } else { 0.0 };
        }
        let texts = fold_spaces(line);
        let texted: Vec<usize> = (0..line.glyphs.len())
            .filter(|&i| !texts[i].is_empty())
            .collect();
        let [r, g, b] = rgb(color);
        ops.push(op("BT", vec![]));
        ops.push(op("rg", nums(&[r, g, b])));
        ops.push(op("Tm", nums(&[1.0, 0.0, skew, 1.0, p.x, p.y])));
        let mut cur = 0.0;
        let mut state: Option<(fonts::FontKey, f64)> = None;
        let mut array: Vec<Object> = Vec::new();
        for (n, &i) in texted.iter().enumerate() {
            let glyph = &line.glyphs[i];
            let width = if glyph.advance > 0.0 {
                let next = texted[n + 1..]
                    .iter()
                    .find(|&&j| line.glyphs[j].advance > 0.0);
                next.map(|&j| origins[j]).unwrap_or(pen) - origins[i]
            } else {
                0.0
            };
            let font = self.fonts.get(doc, glyph.font)?;
            let cid = font.cid(glyph.gid, &texts[i], width.max(0.0))?;
            if state != Some((glyph.font, glyph.dy)) {
                flush(ops, &mut array);
                if state.map(|s| s.0) != Some(glyph.font) {
                    ops.push(op(
                        "Tf",
                        vec![Object::Name(font.name.clone().into()), p.size.into()],
                    ));
                }
                if state.map(|s| s.1) != Some(glyph.dy) {
                    ops.push(op("Ts", nums(&[glyph.dy * p.size / 1000.0])));
                }
                state = Some((glyph.font, glyph.dy));
            }
            let adjust = cur - origins[i];
            if adjust.abs() > 0.5 {
                array.push(Object::Real(adjust as f32));
            }
            array.push(Object::String(
                cid.to_be_bytes().to_vec(),
                StringFormat::Hexadecimal,
            ));
            cur = origins[i] + font.width(cid);
        }
        flush(ops, &mut array);
        ops.push(op("ET", vec![]));
        let drawn: Vec<usize> = (0..line.glyphs.len())
            .filter(|&i| line.glyphs[i].text.is_empty() && !line.glyphs[i].space)
            .collect();
        if !drawn.is_empty() {
            ops.push(op("q", vec![]));
            ops.push(op("rg", nums(&[r, g, b])));
            for i in drawn {
                let glyph = &line.glyphs[i];
                let face = &self.fonts.get(doc, glyph.font)?.face;
                let s = p.size / face.hb.units_per_em() as f64;
                let (ox, oy) = (origins[i] * p.size / 1000.0, glyph.dy * p.size / 1000.0);
                let m = [s, 0.0, skew * s, s, p.x + ox + skew * oy, p.y + oy];
                ops.extend(outline::glyph_path(face, glyph.gid, m));
            }
            ops.push(op("Q", vec![]));
        }
        Ok(())
    }

    fn image(
        &mut self,
        doc: &mut Document,
        ops: &mut Vec<Operation>,
        xobjects: &mut Dictionary,
        i: &ImageBlock,
    ) -> R<()> {
        let bytes = B64.decode(i.png_b64.trim()).map_err(err)?;
        let img = image::load_from_memory(&bytes).map_err(err)?.to_rgba8();
        // Opaque photographs go in as JPEG, many times smaller than raw
        // pixels; anything with transparency or few colours stays exact.
        let opaque = img.pixels().all(|p| p[3] == 255);
        let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        let id = if opaque && img.width() * img.height() > 40_000 && crate::common::photo_like(&rgb)
        {
            crate::common::jpeg_xobject(
                doc,
                img.width(),
                img.height(),
                crate::common::jpeg_bytes(&rgb, 85)?,
            )
        } else {
            rgba_xobject(doc, img.width(), img.height(), img.as_raw())?
        };
        self.images += 1;
        let name = format!("Im{}", self.images);
        xobjects.set(name.as_bytes(), Object::Reference(id));
        let [x1, y1, x2, y2] = norm(i.bbox);
        ops.push(op("q", vec![]));
        ops.push(op("cm", nums(&[x2 - x1, 0.0, 0.0, y2 - y1, x1, y1])));
        ops.push(op("Do", vec![Object::Name(name.into())]));
        ops.push(op("Q", vec![]));
        Ok(())
    }

    fn table(&mut self, doc: &mut Document, ops: &mut Vec<Operation>, t: &Table) -> R<()> {
        if t.border > 0.0 {
            stroke_rect(ops, t.bbox, t.border, t.color);
        }
        for cell in t.rows.iter().flatten() {
            if t.border > 0.0 {
                stroke_rect(ops, cell.bbox, t.border, t.color);
            }
            let [x1, y1, x2, y2] = norm(cell.bbox);
            let pad_x = ((x2 - x1) * 0.1).min(2.0);
            let pad_y = (y2 - y1) * 0.15;
            let spec = TextSpec {
                bbox: [x1 + pad_x, y1 + pad_y, x2 - pad_x, y2 - pad_y],
                text: &cell.text,
                size: cell.size,
                bold: cell.bold,
                italic: false,
                align: cell.align,
                dir: cell.dir,
                color: cell.color,
                tight: false,
                middle: true,
            };
            self.text(doc, ops, &spec)?;
        }
        Ok(())
    }
}

// Each glyph's text with the spaces folded into a neighbour on the side
// that reads as "after the space" in logical order: the glyph before it
// in a left-to-right run, the glyph after it in a right-to-left run.
// Spaces with no such neighbour keep their advance but no text.
fn fold_spaces(line: &Line) -> Vec<String> {
    let mut texts: Vec<String> = line.glyphs.iter().map(|g| g.text.clone()).collect();
    let n = texts.len();
    let mut last: Option<usize> = None;
    for i in 0..n {
        if !line.glyphs[i].space {
            if !texts[i].is_empty() {
                last = Some(i);
            }
            continue;
        }
        let space = std::mem::take(&mut texts[i]);
        if line.glyphs[i].rtl {
            // The glyph to the right is logically before the space.
            let next = (i + 1..n).find(|&j| !line.glyphs[j].space && !texts[j].is_empty());
            match next {
                Some(j) => texts[j].insert_str(0, &space),
                None => {
                    if let Some(j) = last {
                        texts[j].push_str(&space);
                    }
                }
            }
        } else if let Some(j) = last {
            texts[j].push_str(&space);
        }
    }
    texts
}

fn flush(ops: &mut Vec<Operation>, array: &mut Vec<Object>) {
    if !array.is_empty() {
        ops.push(op("TJ", vec![Object::Array(std::mem::take(array))]));
    }
}

fn rule(ops: &mut Vec<Operation>, r: &Rule) {
    let [cr, cg, cb] = rgb(r.color);
    ops.push(op("q", vec![]));
    ops.push(op("RG", nums(&[cr, cg, cb])));
    ops.push(op("w", nums(&[r.width.max(0.1)])));
    ops.push(op("m", nums(&[r.x1, r.y1])));
    ops.push(op("l", nums(&[r.x2, r.y2])));
    ops.push(op("S", vec![]));
    ops.push(op("Q", vec![]));
}

fn stroke_rect(ops: &mut Vec<Operation>, bbox: [f64; 4], width: f64, color: [f64; 3]) {
    rect(
        ops,
        &RectBlock {
            bbox,
            stroke: Some(width),
            fill: None,
            color,
        },
    );
}

fn rect(ops: &mut Vec<Operation>, r: &RectBlock) {
    let [x1, y1, x2, y2] = norm(r.bbox);
    let paint = match (r.stroke, r.fill) {
        (Some(w), Some(_)) if w > 0.0 => "B",
        (_, Some(_)) => "f",
        (Some(w), None) if w > 0.0 => "S",
        _ => return,
    };
    let [sr, sg, sb] = rgb(r.color);
    ops.push(op("q", vec![]));
    ops.push(op("RG", nums(&[sr, sg, sb])));
    if let Some(fill) = r.fill {
        let [fr, fg, fb] = rgb(fill);
        ops.push(op("rg", nums(&[fr, fg, fb])));
    }
    ops.push(op("w", nums(&[r.stroke.unwrap_or(0.0).max(0.1)])));
    ops.push(op("re", nums(&[x1, y1, x2 - x1, y2 - y1])));
    ops.push(op(paint, vec![]));
    ops.push(op("Q", vec![]));
}

// The document's page tree root, created along with a catalog when the
// document is still empty.
fn pages_root(doc: &mut Document) -> R<ObjectId> {
    if let Ok(root) = catalog_id(doc) {
        return doc
            .get_object(root)
            .and_then(|o| o.as_dict())
            .and_then(|d| d.get(b"Pages"))
            .and_then(|o| o.as_reference())
            .map_err(err);
    }
    let pages_id = doc.add_object(dictionary! {
        "Type" => "Pages",
        "Kids" => Vec::<Object>::new(),
        "Count" => 0,
    });
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", root);
    Ok(pages_id)
}

fn append_page(doc: &mut Document, mut page: Dictionary) -> R<ObjectId> {
    let pages_id = pages_root(doc)?;
    page.set("Parent", Object::Reference(pages_id));
    let page_id = doc.add_object(page);
    let pages = doc
        .get_object_mut(pages_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    let count = pages.get(b"Count").and_then(|o| o.as_i64()).unwrap_or(0);
    match pages.get_mut(b"Kids").and_then(|o| o.as_array_mut()) {
        Ok(kids) => kids.push(Object::Reference(page_id)),
        Err(_) => pages.set("Kids", vec![Object::Reference(page_id)]),
    }
    pages.set("Count", count + 1);
    Ok(page_id)
}

// Single-page convenience; multi-page callers share fonts via a Composer.
#[allow(dead_code)]
pub fn compose_page(doc: &mut Document, page: &PageBlocks) -> R<ObjectId> {
    let mut composer = Composer::new();
    let id = composer.page(doc, page)?;
    composer.finish(doc)?;
    Ok(id)
}

pub fn compose_document(pages: &[PageBlocks]) -> R<Document> {
    let mut doc = Document::with_version("1.7");
    let mut composer = Composer::new();
    for page in pages {
        composer.page(&mut doc, page)?;
    }
    composer.finish(&mut doc)?;
    doc.compress();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::roundtrip;

    const ARABIC: &str = "السَّلامُ عَلَيْكُمْ ورحمة الله";
    const ENGLISH: &str = "Hello scanned world";

    fn text(bbox: [f64; 4], s: &str) -> Block {
        Block::Text(serde_json::from_value(serde_json::json!({ "bbox": bbox, "text": s })).unwrap())
    }

    fn page(blocks: Vec<Block>) -> PageBlocks {
        PageBlocks {
            width: 595.0,
            height: 842.0,
            blocks,
        }
    }

    fn font_count(doc: &Document) -> usize {
        let (_, pid) = doc.get_pages().into_iter().next().unwrap();
        let page = doc.get_object(pid).unwrap().as_dict().unwrap();
        let res = doc
            .get_dictionary(
                page.get(b"Resources")
                    .unwrap()
                    .as_reference()
                    .unwrap_or((0, 0)),
            )
            .ok()
            .cloned()
            .unwrap_or_else(|| page.get(b"Resources").unwrap().as_dict().unwrap().clone());
        res.get(b"Font").unwrap().as_dict().unwrap().len()
    }

    fn content_text(doc: &Document) -> String {
        let (_, pid) = doc.get_pages().into_iter().next().unwrap();
        String::from_utf8_lossy(&doc.get_page_content(pid)).to_string()
    }

    fn png_b64() -> String {
        let img = image::RgbaImage::from_fn(4, 4, |x, _| image::Rgba([x as u8 * 60, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        B64.encode(buf.into_inner())
    }

    #[test]
    fn mixed_scripts_embed_two_fonts_and_stay_extractable() {
        let mut doc = compose_document(&[page(vec![
            text([40.0, 760.0, 555.0, 790.0], ARABIC),
            text([40.0, 700.0, 555.0, 724.0], ENGLISH),
        ])])
        .unwrap();
        let doc = roundtrip(&mut doc);
        assert_eq!(doc.get_pages().len(), 1);
        assert_eq!(font_count(&doc), 2);
        // Read back through the export's own glyph walker: every glyph
        // must map to text, and spaces fold so the words come out whole.
        let page = *doc.get_pages().values().next().unwrap();
        let extracted: String = crate::export::glyphs::page_glyphs(&doc, page)
            .unwrap()
            .into_iter()
            .map(|g| g.text.expect("every glyph maps to text"))
            .collect();
        for word in ENGLISH.split(' ') {
            assert!(
                extracted.contains(word),
                "{word} missing from {extracted:?}"
            );
        }
        assert!(
            extracted.contains('ع'),
            "arabic letters missing from {extracted:?}"
        );
    }

    #[test]
    fn arabic_is_shaped_into_joined_forms() {
        let mut doc = Document::with_version("1.7");
        let mut fonts = Fonts::default();
        let cids = |line: &Line| {
            line.glyphs
                .iter()
                .map(|g| g.gid)
                .collect::<std::collections::BTreeSet<u16>>()
        };
        let joined = shape_line(&mut fonts, &mut doc, "ببب", false, Dir::Auto).unwrap();
        let isolated = shape_line(&mut fonts, &mut doc, "ب ب ب", false, Dir::Auto).unwrap();
        assert!(joined.glyphs.len() >= 3);
        assert!(
            cids(&joined).len() > 1,
            "initial, medial and final forms should differ"
        );
        assert_ne!(
            cids(&joined),
            cids(&isolated),
            "joined letters should not use isolated forms"
        );
        let sentence = shape_line(&mut fonts, &mut doc, ARABIC, false, Dir::Auto).unwrap();
        let marks = sentence.glyphs.iter().filter(|g| g.dy != 0.0).count();
        assert!(
            marks > 0,
            "diacritics should be positioned above or below their base"
        );
        let latin = shape_line(&mut fonts, &mut doc, ENGLISH, false, Dir::Auto).unwrap();
        assert_eq!(latin.glyphs.len(), ENGLISH.chars().count());
    }

    #[test]
    fn over_wide_line_shrinks_instead_of_overflowing() {
        let mut doc = Document::with_version("1.7");
        let mut fonts = Fonts::default();
        let line = shape_line(
            &mut fonts,
            &mut doc,
            "a very long line of text",
            false,
            Dir::Ltr,
        )
        .unwrap();
        let bbox = [10.0, 10.0, 60.0, 30.0];
        let place = fit_line(&line, Some(24.0), false, bbox, 30.0, 20.0, Align::Start);
        assert!(place.size < 24.0);
        assert!(line.width() * place.size / 1000.0 <= 50.0 + 1e-6);
        let roomy = fit_line(
            &line,
            Some(24.0),
            false,
            [10.0, 10.0, 1000.0, 30.0],
            30.0,
            20.0,
            Align::End,
        );
        assert_eq!(roomy.size, 24.0);
        assert!(roomy.x > 500.0);
        let derived = fit_line(
            &line,
            None,
            false,
            [10.0, 10.0, 1000.0, 30.0],
            30.0,
            20.0,
            Align::Start,
        );
        assert!(derived.size > 0.0 && derived.y < 30.0 && derived.y > 10.0);
        // A tight box puts the ink top, not the ascent, at the top edge.
        let tight = fit_line(
            &line,
            Some(24.0),
            true,
            [10.0, 10.0, 1000.0, 30.0],
            30.0,
            20.0,
            Align::Start,
        );
        assert!(tight.y > roomy.y);
    }

    #[test]
    fn table_rule_and_image_draw_on_one_page() {
        let cell = |bbox: [f64; 4], s: &str| {
            serde_json::from_value::<Cell>(
                serde_json::json!({ "bbox": bbox, "text": s, "bold": true }),
            )
            .unwrap()
        };
        let table = Table {
            bbox: [50.0, 500.0, 250.0, 560.0],
            rows: vec![
                vec![
                    cell([50.0, 530.0, 150.0, 560.0], "one"),
                    cell([150.0, 530.0, 250.0, 560.0], "اثنان"),
                ],
                vec![
                    cell([50.0, 500.0, 150.0, 530.0], "three"),
                    cell([150.0, 500.0, 250.0, 530.0], "four"),
                ],
            ],
            border: 0.5,
            color: black(),
        };
        let blocks = vec![
            Block::Table(table),
            Block::Rule(Rule {
                x1: 50.0,
                y1: 480.0,
                x2: 250.0,
                y2: 480.0,
                width: 1.0,
                color: [255.0, 0.0, 0.0],
            }),
            Block::Rect(RectBlock {
                bbox: [300.0, 500.0, 400.0, 560.0],
                stroke: Some(1.0),
                fill: Some([0.9, 0.9, 0.9]),
                color: black(),
            }),
            Block::Image(ImageBlock {
                bbox: [300.0, 600.0, 400.0, 700.0],
                png_b64: png_b64(),
            }),
        ];
        let mut doc = compose_document(&[page(blocks)]).unwrap();
        let doc = roundtrip(&mut doc);
        assert_eq!(font_count(&doc), 2);
        let content = content_text(&doc);
        assert_eq!(
            content.matches(" re").count(),
            6,
            "outer border, four cells, one rect"
        );
        assert!(content.contains(" l\nS") || content.contains(" l S") || content.contains("l\nS"));
        assert!(content.contains("/Im1 Do"));
        let images = doc
            .objects
            .values()
            .filter(|o| matches!(o, Object::Stream(s) if s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image".as_slice())))
            .count();
        assert_eq!(images, 2, "image plus its soft mask");
    }

    #[test]
    fn compose_page_appends_to_an_existing_document() {
        let mut doc = crate::common::testing::sample_doc(1);
        compose_page(
            &mut doc,
            &page(vec![text([40.0, 700.0, 300.0, 724.0], "appended")]),
        )
        .unwrap();
        let doc = roundtrip(&mut doc);
        assert_eq!(doc.get_pages().len(), 2);
    }

    #[test]
    fn json_shape_is_accepted() {
        let json = serde_json::json!({
            "width": 595, "height": 842,
            "blocks": [
                { "kind": "text", "bbox": [0, 0, 100, 20], "text": "x", "size": 12, "bold": true, "italic": true,
                  "align": "justify", "dir": "rtl", "color": [0, 0, 1] },
                { "kind": "rule", "x1": 0, "y1": 0, "x2": 10, "y2": 0 },
                { "kind": "rect", "bbox": [0, 0, 10, 10], "fill": [1, 0, 0] },
                { "kind": "image", "bbox": [0, 0, 10, 10], "pngB64": "" },
                { "kind": "table", "bbox": [0, 0, 10, 10], "rows": [[{ "bbox": [0, 0, 5, 5], "text": "c" }]] }
            ]
        });
        let page: PageBlocks = serde_json::from_value(json).unwrap();
        assert_eq!(page.blocks.len(), 5);
    }
}
