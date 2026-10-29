// Writes the document as an Excel workbook (SpreadsheetML). Every page
// becomes a sheet laid out like the page itself: a grid whose columns and
// rows follow the edges of the paragraphs, tables and pictures, with
// each block in its cells and pictures anchored where they were. Every
// table also gets a clean sheet of its own with typed numbers.
use std::collections::{HashMap, HashSet};

use super::model::{Item, Page, TableItem};
use super::xml::{emu, esc, package, picture_file};
use crate::common::R;
use crate::scan::blocks::{Align, Dir};

const SHEET_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XDR_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const REL_SHEET: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_DRAWING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

// Points of column width per Excel character unit.
const PT_PER_CHAR: f64 = 5.5;
// Page sheets are drawn a little wider than the page, and wrapped text a
// little smaller, since Excel's fonts are not the page's fonts.
const PAGE_PT_PER_CHAR: f64 = 4.6;
const WRAP_SHRINK: f64 = 0.95;
// Edges closer than this (points) share one grid line.
const EDGE_TOL: f64 = 2.5;
// Excel's limits for a row's height, in points.
const ROW_MIN: f64 = 2.0;
const ROW_MAX: f64 = 409.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    // A number with its display format.
    Number(f64, NumFmt),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NumFmt {
    Plain,
    Grouped(u8),
    Percent(u8),
}

fn ascii_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => char::from_u32('0' as u32 + (c as u32 - 0x0660)).unwrap(),
            '\u{06F0}'..='\u{06F9}' => char::from_u32('0' as u32 + (c as u32 - 0x06F0)).unwrap(),
            '\u{066B}' => '.',
            '\u{066C}' | '\u{202F}' => ',',
            '\u{2212}' => '-',
            c => c,
        })
        .collect()
}

// Reads "1,234.50", "-12", "(300)", "15%" or "%15" as a number.
pub fn parse_value(text: &str) -> Value {
    let raw = ascii_digits(text.trim());
    let mut s = raw.as_str();
    let percent = s.starts_with('%') || s.ends_with('%');
    s = s.trim_matches('%').trim();
    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = &s[1..s.len() - 1];
    }
    if let Some(rest) = s.strip_prefix('-') {
        negative = !negative;
        s = rest;
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest;
    }
    s = s.trim();
    let (int_part, frac) = match s.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (s, None),
    };
    let grouped = int_part.contains(',');
    let digits_ok = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let int_ok = if grouped {
        let groups: Vec<&str> = int_part.split(',').collect();
        groups.len() >= 2
            && digits_ok(groups[0])
            && groups[0].len() <= 3
            && groups[1..].iter().all(|g| g.len() == 3 && digits_ok(g))
    } else {
        digits_ok(int_part)
    };
    let frac_ok = frac.is_none_or(digits_ok);
    if !int_ok || !frac_ok || (int_part.is_empty() && frac.is_none()) {
        return Value::Text(text.trim().to_string());
    }
    let clean: String = int_part.chars().filter(|c| *c != ',').collect();
    let mut n: f64 = match frac {
        Some(f) => format!("{clean}.{f}").parse().unwrap_or(f64::NAN),
        None => clean.parse().unwrap_or(f64::NAN),
    };
    if !n.is_finite() {
        return Value::Text(text.trim().to_string());
    }
    if negative {
        n = -n;
    }
    let decimals = frac.map_or(0, |f| f.len().min(9) as u8);
    if percent {
        return Value::Number(n / 100.0, NumFmt::Percent(decimals));
    }
    if grouped || decimals > 0 {
        Value::Number(n, NumFmt::Grouped(decimals))
    } else {
        Value::Number(n, NumFmt::Plain)
    }
}

fn format_code(f: NumFmt) -> String {
    let dec = |d: u8| {
        if d == 0 {
            String::new()
        } else {
            format!(".{}", "0".repeat(d as usize))
        }
    };
    match f {
        NumFmt::Plain => "0".into(),
        NumFmt::Grouped(d) => format!("#,##0{}", dec(d)),
        NumFmt::Percent(d) => format!("0{}%", dec(d)),
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct StyleKey {
    bold: bool,
    italic: bool,
    // Font size in half-points.
    size: u16,
    align: Align,
    rtl: bool,
    wrap: bool,
    border: bool,
    top: bool,
    fmt: Option<NumFmt>,
}

fn half_points(size: f64) -> u16 {
    ((size.clamp(5.0, 72.0) * 2.0).round()) as u16
}

#[derive(Default)]
struct Styles {
    keys: Vec<StyleKey>,
    index: HashMap<StyleKey, usize>,
}

impl Styles {
    fn id(&mut self, key: StyleKey) -> usize {
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        self.keys.push(key.clone());
        let i = self.keys.len();
        self.index.insert(key, i);
        i
    }

    fn xml(&self) -> String {
        let mut fmts: Vec<NumFmt> = self.keys.iter().filter_map(|k| k.fmt).collect();
        fmts.sort_by_key(|f| format_code(*f));
        fmts.dedup();
        let fmt_id = |f: NumFmt| 164 + fmts.iter().position(|x| *x == f).unwrap();
        let mut fonts: Vec<(bool, bool, u16)> = vec![(false, false, 22)];
        for k in &self.keys {
            if !fonts.contains(&(k.bold, k.italic, k.size)) {
                fonts.push((k.bold, k.italic, k.size));
            }
        }
        let mut s = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><styleSheet xmlns=\"{SHEET_NS}\">"
        );
        s.push_str(&format!("<numFmts count=\"{}\">", fmts.len()));
        for f in &fmts {
            s.push_str(&format!(
                "<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>",
                fmt_id(*f),
                esc(&format_code(*f))
            ));
        }
        s.push_str(&format!("</numFmts><fonts count=\"{}\">", fonts.len()));
        for (b, i, size) in &fonts {
            s.push_str("<font>");
            if *b {
                s.push_str("<b/>");
            }
            if *i {
                s.push_str("<i/>");
            }
            s.push_str(&format!(
                "<sz val=\"{}\"/><name val=\"Calibri\"/></font>",
                *size as f64 / 2.0
            ));
        }
        s.push_str("</fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills>");
        s.push_str("<borders count=\"2\"><border><left/><right/><top/><bottom/><diagonal/></border><border>");
        for side in ["left", "right", "top", "bottom"] {
            s.push_str(&format!(
                "<{side} style=\"thin\"><color auto=\"1\"/></{side}>"
            ));
        }
        s.push_str("<diagonal/></border></borders>");
        s.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
        s.push_str(&format!("<cellXfs count=\"{}\">", self.keys.len() + 1));
        s.push_str("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>");
        for k in &self.keys {
            let font = fonts
                .iter()
                .position(|f| *f == (k.bold, k.italic, k.size))
                .unwrap_or(0);
            let num = k.fmt.map_or(0, fmt_id);
            let horizontal = match k.align {
                Align::Start => "left",
                Align::Center => "center",
                Align::End => "right",
                Align::Justify => "justify",
            };
            s.push_str(&format!(
                "<xf numFmtId=\"{num}\" fontId=\"{font}\" fillId=\"0\" borderId=\"{}\" xfId=\"0\" applyFont=\"1\" applyNumberFormat=\"1\" applyBorder=\"1\" applyAlignment=\"1\"><alignment horizontal=\"{horizontal}\" vertical=\"{}\"{}{}/></xf>",
                u8::from(k.border),
                if k.top { "top" } else { "center" },
                if k.wrap { " wrapText=\"1\"" } else { "" },
                if k.rtl { " readingOrder=\"2\"" } else { "" }
            ));
        }
        s.push_str("</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>");
        s
    }
}

pub fn column_name(mut n: usize) -> String {
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    s
}

fn cell_ref(r: usize, c: usize) -> String {
    format!("{}{}", column_name(c), r + 1)
}

fn cell_xml(r: usize, c: usize, value: &Value, style: usize) -> String {
    let name = cell_ref(r, c);
    match value {
        Value::Text(t) if t.is_empty() => format!("<c r=\"{name}\" s=\"{style}\"/>"),
        Value::Text(t) => format!(
            "<c r=\"{name}\" s=\"{style}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            esc(t)
        ),
        Value::Number(n, _) => format!("<c r=\"{name}\" s=\"{style}\"><v>{n}</v></c>"),
    }
}

// Numbers are only typed when their column holds mostly numbers, so a
// reference like "2024" in a text column stays text.
fn typed(t: &TableItem) -> Vec<Vec<Value>> {
    let cols = t.cols.len().max(1);
    let mut numeric = vec![0usize; cols];
    let mut filled = vec![0usize; cols];
    let parsed: Vec<Vec<(usize, &str, Value)>> = t
        .rows
        .iter()
        .map(|row| {
            let mut col = 0;
            row.iter()
                .map(|c| {
                    let v = parse_value(&c.text);
                    if !c.text.trim().is_empty() && col < cols {
                        filled[col] += 1;
                        if matches!(v, Value::Number(..)) {
                            numeric[col] += 1;
                        }
                    }
                    let at = col;
                    col += c.span.max(1);
                    (at, c.text.as_str(), v)
                })
                .collect()
        })
        .collect();
    parsed
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(col, text, v)| match v {
                    Value::Number(..) if col < cols && numeric[col] * 2 < filled[col] => {
                        Value::Text(text.trim().to_string())
                    }
                    v => v,
                })
                .collect()
        })
        .collect()
}

fn sheet_head(rtl: bool, gridlines: bool) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"{SHEET_NS}\" xmlns:r=\"{R_NS}\"><sheetViews><sheetView workbookViewId=\"0\"{}{}/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>",
        if rtl { " rightToLeft=\"1\"" } else { "" },
        if gridlines { "" } else { " showGridLines=\"0\"" }
    )
}

fn merges_xml(merges: &[String]) -> String {
    if merges.is_empty() {
        return String::new();
    }
    let mut s = format!("<mergeCells count=\"{}\">", merges.len());
    for m in merges {
        s.push_str(&format!("<mergeCell ref=\"{m}\"/>"));
    }
    s.push_str("</mergeCells>");
    s
}

// One table on its own sheet: the clean data.
fn table_sheet_xml(t: &TableItem, styles: &mut Styles) -> String {
    let mut s = sheet_head(t.rtl, true);
    s.push_str("<cols>");
    for (i, w) in t.cols.iter().enumerate() {
        s.push_str(&format!(
            "<col min=\"{}\" max=\"{}\" width=\"{:.2}\" customWidth=\"1\"/>",
            i + 1,
            i + 1,
            (w / PT_PER_CHAR).clamp(4.0, 80.0)
        ));
    }
    s.push_str("</cols><sheetData>");
    let values = typed(t);
    let mut merges: Vec<String> = Vec::new();
    for (r, (row, vals)) in t.rows.iter().zip(&values).enumerate() {
        let lines = row
            .iter()
            .map(|c| c.text.split('\n').count())
            .max()
            .unwrap_or(1);
        s.push_str(&format!("<row r=\"{}\"", r + 1));
        if lines > 1 {
            s.push_str(&format!(" ht=\"{}\" customHeight=\"1\"", 15 * lines));
        }
        s.push('>');
        let mut col = 0;
        for (c, v) in row.iter().zip(vals) {
            let span = c.span.max(1);
            let style = styles.id(StyleKey {
                bold: c.bold,
                italic: c.italic,
                size: 22,
                align: c.align,
                rtl: c.dir == Dir::Rtl,
                wrap: c.text.contains('\n'),
                border: t.ruled,
                top: false,
                fmt: match v {
                    Value::Number(_, f) => Some(*f),
                    Value::Text(_) => None,
                },
            });
            // The value goes in the first cell of a span; the rest exist so
            // their borders are drawn.
            s.push_str(&cell_xml(r, col, v, style));
            for k in 1..span {
                s.push_str(&cell_xml(r, col + k, &Value::Text(String::new()), style));
            }
            if span > 1 {
                merges.push(format!(
                    "{}:{}",
                    cell_ref(r, col),
                    cell_ref(r, col + span - 1)
                ));
            }
            col += span;
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");
    s.push_str(&merges_xml(&merges));
    s.push_str("</worksheet>");
    s
}

// ---- The page laid out on a grid ----

// Grid lines of a page sheet, in points from the page's top-left corner.
struct Grid {
    xs: Vec<f64>,
    ys: Vec<f64>,
}

fn cluster(mut v: Vec<f64>, tol: f64) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut out: Vec<(f64, usize)> = Vec::new();
    for x in v {
        match out.last_mut() {
            Some((m, n)) if (x - *m).abs() <= tol => {
                *m = (*m * *n as f64 + x) / (*n as f64 + 1.0);
                *n += 1;
            }
            _ => out.push((x, 1)),
        }
    }
    out.into_iter().map(|(m, _)| m).collect()
}

fn nearest(bounds: &[f64], v: f64) -> usize {
    bounds
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - v).abs().partial_cmp(&(b.1 - v).abs()).unwrap())
        .map(|(i, _)| i)
        .unwrap_or(0)
}

// The band holding `v`: the last grid line at or before it.
fn band(bounds: &[f64], v: f64) -> usize {
    let n = bounds.len().saturating_sub(2);
    bounds
        .iter()
        .rposition(|b| *b <= v + 1e-6)
        .unwrap_or(0)
        .min(n)
}

impl Grid {
    fn build(page: &Page) -> Grid {
        let mut xs = vec![0.0, page.width];
        let mut ys = vec![0.0, page.height];
        for b in &page.blocks {
            match &b.item {
                Item::Table(t) if !t.col_edges.is_empty() && !t.row_edges.is_empty() => {
                    xs.extend(t.col_edges.iter().copied());
                    ys.extend(t.row_edges.iter().copied());
                }
                _ => {
                    xs.extend([b.bbox[0], b.bbox[2]]);
                    ys.extend([b.bbox[1], b.bbox[3]]);
                }
            }
        }
        Grid {
            xs: cluster(xs, EDGE_TOL),
            ys: cluster(ys, EDGE_TOL),
        }
    }

    // Cell range covering a box: (first row, last row, first col, last col).
    fn span(&self, b: &[f64; 4]) -> (usize, usize, usize, usize) {
        let c0 = nearest(&self.xs, b[0]).min(self.xs.len() - 2);
        let c1 = nearest(&self.xs, b[2]).saturating_sub(1).max(c0);
        let r0 = nearest(&self.ys, b[1]).min(self.ys.len() - 2);
        let r1 = nearest(&self.ys, b[3]).saturating_sub(1).max(r0);
        (r0, r1, c0, c1)
    }
}

struct Placed {
    r0: usize,
    r1: usize,
    c0: usize,
    c1: usize,
    value: Value,
    style: usize,
}

struct Media {
    // (part name, bytes) of every picture in the workbook.
    files: Vec<(String, Vec<u8>)>,
}

// A sheet's drawing part: its XML and the (id, target) image relationships.
type Drawing = (String, Vec<(String, String)>);

// Places a block's cells, shrinking to the first cell when its range is
// already taken, so nothing overwrites anything.
fn place(
    taken: &mut HashSet<(usize, usize)>,
    out: &mut Vec<Placed>,
    (r0, r1, c0, c1): (usize, usize, usize, usize),
    value: Value,
    style: usize,
) {
    let free = |taken: &HashSet<(usize, usize)>, r0: usize, r1: usize, c0: usize, c1: usize| {
        (r0..=r1).all(|r| (c0..=c1).all(|c| !taken.contains(&(r, c))))
    };
    let (r1, c1) = if free(taken, r0, r1, c0, c1) {
        (r1, c1)
    } else if free(taken, r0, r0, c0, c0) {
        (r0, c0)
    } else {
        return;
    };
    for r in r0..=r1 {
        for c in c0..=c1 {
            taken.insert((r, c));
        }
    }
    out.push(Placed {
        r0,
        r1,
        c0,
        c1,
        value,
        style,
    });
}

fn drawing_xml(anchors: &[String]) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><xdr:wsDr xmlns:xdr=\"{XDR_NS}\" xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\">{}</xdr:wsDr>",
        anchors.concat()
    )
}

// The page as a sheet. Returns the sheet XML and, when pictures exist,
// the drawing XML with the relationship ids it uses.
fn page_sheet_xml(
    page: &Page,
    styles: &mut Styles,
    media: &mut Media,
) -> (String, Option<Drawing>) {
    let grid = Grid::build(page);
    let rtl = page.rtl();
    let mut taken: HashSet<(usize, usize)> = HashSet::new();
    let mut cells: Vec<Placed> = Vec::new();
    let mut anchors: Vec<String> = Vec::new();
    let mut drawing_rels: Vec<(String, String)> = Vec::new();
    for b in &page.blocks {
        match &b.item {
            Item::Para(p) => {
                // A single line overflows into empty neighbours rather than
                // wrapping inside a box that Excel's font may not fit.
                let wrap = p.text.contains('\n') || (b.bbox[3] - b.bbox[1]) > 1.6 * p.size;
                let style = styles.id(StyleKey {
                    bold: p.bold,
                    italic: p.italic,
                    size: half_points(if wrap { p.size * WRAP_SHRINK } else { p.size }),
                    align: p.align,
                    rtl: p.dir == Dir::Rtl,
                    wrap,
                    border: false,
                    top: true,
                    fmt: None,
                });
                place(
                    &mut taken,
                    &mut cells,
                    grid.span(&b.bbox),
                    Value::Text(p.text.clone()),
                    style,
                );
            }
            Item::Table(t) => {
                let values = typed(t);
                let (cols, rows) = if t.col_edges.len() == t.cols.len() + 1
                    && t.row_edges.len() == t.rows.len() + 1
                {
                    (t.col_edges.clone(), t.row_edges.clone())
                } else {
                    // Edges missing: spread the table evenly over its box.
                    let (w, h) = (b.bbox[2] - b.bbox[0], b.bbox[3] - b.bbox[1]);
                    let nc = t.cols.len().max(1) as f64;
                    let nr = t.rows.len().max(1) as f64;
                    (
                        (0..=t.cols.len())
                            .map(|i| b.bbox[0] + w * i as f64 / nc)
                            .collect(),
                        (0..=t.rows.len())
                            .map(|i| b.bbox[1] + h * i as f64 / nr)
                            .collect(),
                    )
                };
                for (ri, (row, vals)) in t.rows.iter().zip(&values).enumerate() {
                    let mut col = 0;
                    for (c, v) in row.iter().zip(vals) {
                        let span = c.span.max(1);
                        let x0 = cols[col.min(cols.len() - 1)];
                        let x1 = cols[(col + span).min(cols.len() - 1)];
                        let y0 = rows[ri.min(rows.len() - 1)];
                        let y1 = rows[(ri + 1).min(rows.len() - 1)];
                        let style = styles.id(StyleKey {
                            bold: c.bold,
                            italic: c.italic,
                            size: half_points(c.size),
                            align: c.align,
                            rtl: c.dir == Dir::Rtl,
                            wrap: c.text.contains('\n'),
                            border: t.ruled,
                            top: false,
                            fmt: match v {
                                Value::Number(_, f) => Some(*f),
                                Value::Text(_) => None,
                            },
                        });
                        place(
                            &mut taken,
                            &mut cells,
                            grid.span(&[x0, y0, x1, y1]),
                            v.clone(),
                            style,
                        );
                        col += span;
                    }
                }
            }
            Item::Picture(p) => {
                let Some((ext, bytes)) = picture_file(&p.png_b64) else {
                    continue;
                };
                let n = media.files.len() + 1;
                media
                    .files
                    .push((format!("xl/media/image{n}.{ext}"), bytes));
                let rid = format!("rIdImg{n}");
                drawing_rels.push((rid.clone(), format!("../media/image{n}.{ext}")));
                let anchor = |x: f64, y: f64| {
                    let c = band(&grid.xs, x);
                    let r = band(&grid.ys, y);
                    format!(
                        "<xdr:col>{c}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{r}</xdr:row><xdr:rowOff>{}</xdr:rowOff>",
                        emu((x - grid.xs[c]).max(0.0)),
                        emu((y - grid.ys[r]).max(0.0))
                    )
                };
                let k = anchors.len() + 1;
                anchors.push(format!(
                    concat!(
                        "<xdr:twoCellAnchor editAs=\"oneCell\"><xdr:from>{from}</xdr:from><xdr:to>{to}</xdr:to>",
                        "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{k}\" name=\"Picture {k}\"/><xdr:cNvPicPr/></xdr:nvPicPr>",
                        "<xdr:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>",
                        "<xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
                        "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:twoCellAnchor>"
                    ),
                    from = anchor(b.bbox[0], b.bbox[1]),
                    to = anchor(b.bbox[2], b.bbox[3]),
                    k = k,
                    rid = rid,
                    cx = emu(b.bbox[2] - b.bbox[0]),
                    cy = emu(b.bbox[3] - b.bbox[1]),
                ));
            }
        }
    }

    let mut s = sheet_head(rtl, false);
    s.push_str("<cols>");
    for (i, w) in grid.xs.windows(2).enumerate() {
        s.push_str(&format!(
            "<col min=\"{}\" max=\"{}\" width=\"{:.2}\" customWidth=\"1\"/>",
            i + 1,
            i + 1,
            ((w[1] - w[0]) / PAGE_PT_PER_CHAR).clamp(0.5, 200.0)
        ));
    }
    s.push_str("</cols><sheetData>");
    let mut by_row: HashMap<usize, Vec<&Placed>> = HashMap::new();
    for p in &cells {
        by_row.entry(p.r0).or_default().push(p);
    }
    let mut merges: Vec<String> = Vec::new();
    for (r, h) in grid.ys.windows(2).enumerate() {
        let height = (h[1] - h[0]).clamp(ROW_MIN, ROW_MAX);
        s.push_str(&format!(
            "<row r=\"{}\" ht=\"{height:.2}\" customHeight=\"1\">",
            r + 1
        ));
        if let Some(list) = by_row.get_mut(&r) {
            list.sort_by_key(|p| p.c0);
            for p in list.iter() {
                s.push_str(&cell_xml(p.r0, p.c0, &p.value, p.style));
                if p.r1 > p.r0 || p.c1 > p.c0 {
                    merges.push(format!("{}:{}", cell_ref(p.r0, p.c0), cell_ref(p.r1, p.c1)));
                }
            }
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");
    s.push_str(&merges_xml(&merges));
    if !anchors.is_empty() {
        s.push_str("<drawing r:id=\"rIdDrawing\"/>");
    }
    s.push_str("</worksheet>");
    let drawing = (!anchors.is_empty()).then(|| (drawing_xml(&anchors), drawing_rels));
    (s, drawing)
}

fn sheet_name(base: &str, used: &mut Vec<String>) -> String {
    let clean: String = base
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(28)
        .collect();
    let mut name = clean.clone();
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
        name = format!("{clean} {n}");
        n += 1;
    }
    used.push(name.clone());
    name
}

struct Sheet {
    name: String,
    xml: String,
    drawing: Option<Drawing>,
}

// `lang` picks the sheet names: "ar" or anything else for English.
pub fn write_xlsx(pages: &[Page], lang: &str) -> R<Vec<u8>> {
    let ar = lang == "ar";
    let mut styles = Styles::default();
    let mut media = Media { files: Vec::new() };
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut used = Vec::new();
    for page in pages {
        let base = if ar {
            format!("صفحة {}", page.number)
        } else {
            format!("Page {}", page.number)
        };
        let (xml, drawing) = page_sheet_xml(page, &mut styles, &mut media);
        sheets.push(Sheet {
            name: sheet_name(&base, &mut used),
            xml,
            drawing,
        });
    }
    for page in pages {
        let mut n = 0;
        for b in &page.blocks {
            if let Item::Table(t) = &b.item {
                if t.rows.is_empty() || t.cols.is_empty() {
                    continue;
                }
                n += 1;
                let base = if ar {
                    format!("صفحة {} - جدول {n}", page.number)
                } else {
                    format!("Page {} - Table {n}", page.number)
                };
                sheets.push(Sheet {
                    name: sheet_name(&base, &mut used),
                    xml: table_sheet_xml(t, &mut styles),
                    drawing: None,
                });
            }
        }
    }

    let mut workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><workbook xmlns=\"{SHEET_NS}\" xmlns:r=\"{R_NS}\"><sheets>"
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    let mut types = String::from(concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        "<Default Extension=\"png\" ContentType=\"image/png\"/>",
        "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>",
        "<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>",
        "<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>"
    ));
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, sheet) in sheets.iter().enumerate() {
        let id = i + 1;
        workbook.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{id}\" r:id=\"rId{id}\"/>",
            esc(&sheet.name)
        ));
        rels.push_str(&format!(
            "<Relationship Id=\"rId{id}\" Type=\"{REL_SHEET}\" Target=\"worksheets/sheet{id}.xml\"/>"
        ));
        types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{id}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ));
        parts.push((
            format!("xl/worksheets/sheet{id}.xml"),
            sheet.xml.clone().into_bytes(),
        ));
        if let Some((drawing, images)) = &sheet.drawing {
            parts.push((
                format!("xl/worksheets/_rels/sheet{id}.xml.rels"),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rIdDrawing\" Type=\"{REL_DRAWING}\" Target=\"../drawings/drawing{id}.xml\"/></Relationships>"
                )
                .into_bytes(),
            ));
            parts.push((
                format!("xl/drawings/drawing{id}.xml"),
                drawing.clone().into_bytes(),
            ));
            let mut drels = String::from(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            );
            for (rid, target) in images {
                drels.push_str(&format!(
                    "<Relationship Id=\"{rid}\" Type=\"{REL_IMAGE}\" Target=\"{target}\"/>"
                ));
            }
            drels.push_str("</Relationships>");
            parts.push((
                format!("xl/drawings/_rels/drawing{id}.xml.rels"),
                drels.into_bytes(),
            ));
            types.push_str(&format!(
                "<Override PartName=\"/xl/drawings/drawing{id}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"
            ));
        }
    }
    workbook.push_str("</sheets></workbook>");
    rels.push_str(&format!(
        "<Relationship Id=\"rIdStyles\" Type=\"{REL_STYLES}\" Target=\"styles.xml\"/></Relationships>"
    ));
    types.push_str("</Types>");
    let root_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_DOC}\" Target=\"xl/workbook.xml\"/></Relationships>"
    );
    let mut all: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", types.into_bytes()),
        ("_rels/.rels", root_rels.into_bytes()),
        ("xl/workbook.xml", workbook.into_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.into_bytes()),
        ("xl/styles.xml", styles.xml().into_bytes()),
    ];
    for (name, bytes) in &parts {
        all.push((name.as_str(), bytes.clone()));
    }
    for (name, bytes) in &media.files {
        all.push((name.as_str(), bytes.clone()));
    }
    package(&all)
}

#[cfg(test)]
mod tests {
    use super::super::model::{Block, CellItem, Para, Picture};
    use super::super::xml::unpack;
    use super::*;
    use base64::Engine;

    #[test]
    fn parses_numbers_in_their_common_forms() {
        assert_eq!(
            parse_value("4,800.00"),
            Value::Number(4800.0, NumFmt::Grouped(2))
        );
        assert_eq!(parse_value("-12"), Value::Number(-12.0, NumFmt::Plain));
        assert_eq!(parse_value("(300)"), Value::Number(-300.0, NumFmt::Plain));
        assert_eq!(parse_value("15%"), Value::Number(0.15, NumFmt::Percent(0)));
        assert_eq!(parse_value("%15"), Value::Number(0.15, NumFmt::Percent(0)));
        assert_eq!(
            parse_value("٣٬٥٠٠"),
            Value::Number(3500.0, NumFmt::Grouped(0))
        );
        assert_eq!(parse_value("1,23"), Value::Text("1,23".into()));
        assert_eq!(parse_value("INV-2024"), Value::Text("INV-2024".into()));
        assert_eq!(
            parse_value("7,000.00 ريال"),
            Value::Text("7,000.00 ريال".into())
        );
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(27), "AB");
    }

    fn cell(text: &str, span: usize) -> CellItem {
        CellItem {
            text: text.into(),
            span,
            align: Align::Center,
            dir: Dir::Auto,
            bold: false,
            italic: false,
            size: 10.0,
        }
    }

    fn para(text: &str, bbox: [f64; 4], dir: Dir) -> Block {
        Block {
            bbox,
            item: Item::Para(Para {
                text: text.into(),
                size: 12.0,
                bold: false,
                italic: false,
                color: [0.0; 3],
                align: if dir == Dir::Rtl {
                    Align::End
                } else {
                    Align::Start
                },
                dir,
            }),
        }
    }

    #[test]
    fn writes_page_and_table_sheets_with_typed_cells_and_merges() {
        let table = TableItem {
            cols: vec![120.0, 60.0, 60.0],
            rows: vec![
                vec![cell("الوصف", 1), cell("الكمية", 1), cell("السعر", 1)],
                vec![cell("طاولة", 1), cell("1", 1), cell("4,800.00", 1)],
                vec![cell("الإجمالي", 2), cell("4,800.00", 1)],
            ],
            ruled: true,
            rtl: true,
            col_edges: vec![100.0, 220.0, 280.0, 340.0],
            row_edges: vec![200.0, 220.0, 240.0, 260.0],
        };
        let png = {
            let mut buf = std::io::Cursor::new(Vec::new());
            image::RgbImage::from_pixel(2, 2, image::Rgb([1, 2, 3]))
                .write_to(&mut buf, image::ImageFormat::Png)
                .unwrap();
            base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
        };
        let pages = vec![Page {
            number: 3,
            width: 595.0,
            height: 842.0,
            blocks: vec![
                para("عنوان الصفحة", [300.0, 60.0, 540.0, 74.0], Dir::Rtl),
                Block {
                    bbox: [100.0, 200.0, 340.0, 260.0],
                    item: Item::Table(table),
                },
                para("نص حر\nسطر ثان", [100.0, 300.0, 540.0, 340.0], Dir::Rtl),
                Block {
                    bbox: [100.0, 400.0, 200.0, 450.0],
                    item: Item::Picture(Picture {
                        png_b64: png,
                        width: 100.0,
                        height: 50.0,
                    }),
                },
            ],
        }];
        let bytes = write_xlsx(&pages, "ar").unwrap();
        let wb = unpack(&bytes, "xl/workbook.xml");
        assert!(wb.contains("name=\"صفحة 3\""), "{wb}");
        assert!(wb.contains("صفحة 3 - جدول 1"), "{wb}");
        // The page sheet: the title in merged cells, the table with its
        // numbers, the picture anchored.
        let page = unpack(&bytes, "xl/worksheets/sheet1.xml");
        assert!(page.contains("rightToLeft=\"1\""));
        assert!(page.contains("showGridLines=\"0\""));
        assert!(page.contains("عنوان الصفحة"));
        assert!(page.contains("<v>4800</v>"));
        assert!(page.contains("<mergeCell ref="));
        assert!(page.contains("<drawing r:id=\"rIdDrawing\"/>"));
        assert!(page.contains("customHeight=\"1\""));
        let drawing = unpack(&bytes, "xl/drawings/drawing1.xml");
        assert!(drawing.contains("<xdr:twoCellAnchor"));
        assert!(drawing.contains("r:embed=\"rIdImg1\""));
        assert!(
            unpack(&bytes, "xl/drawings/_rels/drawing1.xml.rels").contains("../media/image1.png")
        );
        assert!(unpack(&bytes, "[Content_Types].xml").contains("drawing+xml"));
        // The table's own sheet.
        let sheet = unpack(&bytes, "xl/worksheets/sheet2.xml");
        assert!(sheet.contains("<c r=\"B2\" s=\""), "{sheet}");
        assert!(sheet.contains("<v>4800</v>"));
        assert!(sheet.contains("<mergeCell ref=\"A3:B3\"/>"));
        assert!(sheet.contains("<t xml:space=\"preserve\">طاولة</t>"));
        let styles = unpack(&bytes, "xl/styles.xml");
        assert!(styles.contains("formatCode=\"#,##0.00\""));
        assert!(styles.contains("style=\"thin\""));
        // The one-line title keeps its size; the wrapped paragraph is a
        // touch smaller so Excel's font still fits the box.
        assert!(styles.contains("<sz val=\"12\"/>"));
        assert!(styles.contains("<sz val=\"11.5\"/>"));
    }

    #[test]
    fn a_text_column_keeps_its_numbers_as_text() {
        let table = TableItem {
            cols: vec![100.0, 50.0],
            rows: vec![
                vec![cell("Order 2024", 1), cell("10", 1)],
                vec![cell("2025", 1), cell("20", 1)],
                vec![cell("Ref A", 1), cell("30", 1)],
            ],
            ruled: false,
            rtl: false,
            col_edges: vec![],
            row_edges: vec![],
        };
        let v = typed(&table);
        assert_eq!(v[1][0], Value::Text("2025".into()));
        assert_eq!(v[1][1], Value::Number(20.0, NumFmt::Plain));
    }

    #[test]
    fn grid_lines_come_from_block_edges() {
        let page = Page {
            number: 1,
            width: 200.0,
            height: 100.0,
            blocks: vec![
                para("a", [10.0, 10.0, 90.0, 30.0], Dir::Ltr),
                para("b", [11.0, 40.0, 150.0, 60.0], Dir::Ltr),
            ],
        };
        let g = Grid::build(&page);
        assert_eq!(g.xs.len(), 5, "{:?}", g.xs);
        assert_eq!(g.ys.len(), 6, "{:?}", g.ys);
        assert_eq!(g.span(&[10.0, 10.0, 90.0, 30.0]), (1, 1, 1, 1));
        assert_eq!(g.span(&[11.0, 40.0, 150.0, 60.0]), (3, 3, 1, 2));
        assert_eq!(band(&g.xs, 95.0), 2);
    }
}
