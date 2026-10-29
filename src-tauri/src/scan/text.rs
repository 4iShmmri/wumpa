// Turns OCR lines into table cell text and free text lines with alignment,
// direction, size, weight and colour. Free lines are grouped into
// paragraphs, which decide the alignment and even out the sizes.
use image::{GrayImage, RgbImage};

use super::blocks::{Align, Dir};
use super::tables::Table;
use crate::ocr::OcrLine;

#[derive(Debug, Clone, PartialEq)]
pub struct TextLine {
    // Spans the paragraph's width so justified and centred lines share a measure.
    pub bbox: [f64; 4],
    // Width of the line's own ink.
    pub extent: f64,
    pub text: String,
    pub size_px: f64,
    pub bold: bool,
    pub italic: bool,
    pub align: Align,
    pub dir: Dir,
    pub color: [f64; 3],
    pub para: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct CellText {
    pub text: String,
    pub align: Align,
    pub dir: Dir,
    pub bold: bool,
    pub italic: bool,
    // Median font size of the cell's lines in pixels; 0 when empty.
    pub size_px: f64,
    // Median ink density of the cell's lines; 0 when empty.
    pub density: f64,
}

pub struct Layout {
    pub lines: Vec<TextLine>,
    // cells[table][row][col], parallel to the tables' rows.
    pub cells: Vec<Vec<Vec<CellText>>>,
    // Tables found from column alignment alone, with their cell text.
    pub column_tables: Vec<(Table, Vec<Vec<CellText>>)>,
}

// A line of a text PDF: the glyph boxes and font facts are exact.
pub struct NativeLine {
    pub line: OcrLine,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
}

// Lines closer than this share of the median line height form a paragraph.
const PARA_GAP: f64 = 0.8;
// Lines whose size ratio falls outside this window never share a paragraph.
const PARA_SIZE_RATIO: f64 = 1.4;
// Lines of one paragraph keep their size inside this window around the median.
const LINE_SIZE_WINDOW: f64 = 0.15;
// A cell holding this many rows of text is a boxed region (a form section,
// a paragraph in a frame): its lines are laid out as free text instead.
const MAX_CELL_ROWS: usize = 3;
// A line edge this close to the paragraph's edge, as a share of the
// paragraph width, is taken to be flush with it.
const EDGE_TOL: f64 = 0.06;
// Rows of a line box with at least this share of the fullest row carry
// glyph ink; sparser rows are specks.
const INK_ROW: f64 = 0.02;
// A row inked across this share of the box width is a rule, not glyphs.
const RULE_ROW: f64 = 0.9;
// Ink density relative to the page median that counts as bold.
const BOLD_RATIO: f64 = 1.35;

fn is_arabic(c: char) -> bool {
    matches!(c as u32,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

pub fn script_dir(text: &str) -> Dir {
    let mut rtl = 0;
    let mut ltr = 0;
    for c in text.chars() {
        if is_arabic(c) {
            rtl += 1;
        } else if c.is_ascii_alphabetic() || (c.is_alphabetic() && c as u32 >= 0x00C0) {
            ltr += 1;
        }
    }
    if rtl > 0 && rtl >= ltr {
        Dir::Rtl
    } else if ltr > 0 {
        Dir::Ltr
    } else {
        Dir::Auto
    }
}

fn clamp_box(b: &[f64; 4], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let x0 = b[0].floor().max(0.0) as u32;
    let y0 = b[1].floor().max(0.0) as u32;
    let x1 = (b[2].ceil().max(0.0) as u32).min(w);
    let y1 = (b[3].ceil().max(0.0) as u32).min(h);
    (x0, y0, x1.max(x0), y1.max(y0))
}

pub(super) fn ink_density(ink: &GrayImage, b: &[f64; 4]) -> f64 {
    let (x0, y0, x1, y1) = clamp_box(b, ink.width(), ink.height());
    let area = ((x1 - x0) * (y1 - y0)) as f64;
    if area == 0.0 {
        return 0.0;
    }
    let mut n = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            if ink.get_pixel(x, y).0[0] > 0 {
                n += 1.0;
            }
        }
    }
    n / area
}

fn ink_rows(ink: &GrayImage, b: &[f64; 4]) -> (u32, Vec<f64>) {
    let (x0, y0, x1, y1) = clamp_box(b, ink.width(), ink.height());
    let rows = (y0..y1)
        .map(|y| (x0..x1).filter(|&x| ink.get_pixel(x, y).0[0] > 0).count() as f64)
        .collect();
    (y0, rows)
}

// Vertical extent of the glyph ink in the box, in pixels: the first and
// one past the last row with real ink, skipping specks and ruling lines.
fn ink_band(ink: &GrayImage, b: &[f64; 4]) -> Option<(f64, f64)> {
    let (y0, rows) = ink_rows(ink, b);
    let width = b[2] - b[0];
    let peak = rows
        .iter()
        .filter(|&&r| r < RULE_ROW * width)
        .cloned()
        .fold(0.0, f64::max);
    let min = (INK_ROW * peak).max(2.0);
    let inked = |r: &f64| *r >= min && *r < RULE_ROW * width;
    let lo = rows.iter().position(inked)?;
    let hi = rows.iter().rposition(inked)?;
    Some(((y0 + lo as u32) as f64, (y0 + hi as u32 + 1) as f64))
}

// Height of the band holding the central 90% of the box's ink rows; a
// proxy for the glyph body that ignores stray diacritics and descenders.
fn body_height(ink: &GrayImage, b: &[f64; 4]) -> Option<f64> {
    let (_, rows) = ink_rows(ink, b);
    let total: f64 = rows.iter().sum();
    if total < 1.0 {
        return None;
    }
    let mut acc = 0.0;
    let (mut lo, mut hi) = (None, 0usize);
    for (i, r) in rows.iter().enumerate() {
        acc += r;
        if lo.is_none() && acc >= 0.05 * total {
            lo = Some(i);
        }
        if acc <= 0.95 * total {
            hi = i;
        }
    }
    Some((hi.max(lo?) - lo? + 1) as f64)
}

// Font size in pixels. The ink band is roughly the cap/x height for
// Latin and the full letter body for Arabic, so the divisor differs.
fn font_size(ink: &GrayImage, b: &[f64; 4], dir: Dir) -> f64 {
    let h = b[3] - b[1];
    let divisor = if dir == Dir::Rtl { 0.85 } else { 0.68 };
    match body_height(ink, b) {
        Some(band) => (band / divisor).clamp(0.45 * h, 0.95 * h),
        None => 0.7 * h,
    }
}

pub fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

// Median colour of the ink pixels; black unless clearly coloured.
fn ink_color(color: &RgbImage, ink: &GrayImage, b: &[f64; 4]) -> [f64; 3] {
    let (x0, y0, x1, y1) = clamp_box(b, ink.width(), ink.height());
    let mut ch: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for y in y0..y1 {
        for x in x0..x1 {
            if ink.get_pixel(x, y).0[0] > 0 {
                let p = color.get_pixel(x, y).0;
                for k in 0..3 {
                    ch[k].push(p[k] as f64);
                }
            }
        }
    }
    if ch[0].len() < 20 {
        return [0.0; 3];
    }
    let m = [median(&mut ch[0]), median(&mut ch[1]), median(&mut ch[2])];
    let spread = m.iter().cloned().fold(0.0, f64::max) - m.iter().cloned().fold(255.0, f64::min);
    if spread < 48.0 {
        [0.0; 3]
    } else {
        [m[0] / 255.0, m[1] / 255.0, m[2] / 255.0]
    }
}

#[derive(Debug, Clone)]
pub(super) struct Info {
    pub(super) bbox: [f64; 4],
    // The box narrowed to the rows that carry glyph ink.
    tight: [f64; 4],
    text: String,
    dir: Dir,
    pub(super) size: f64,
    bold: bool,
    italic: bool,
    density: f64,
    color: [f64; 3],
}

fn measure(lines: &[OcrLine], ink: &GrayImage, color: &RgbImage) -> Vec<Info> {
    let mut dens: Vec<f64> = lines.iter().map(|l| ink_density(ink, &l.bbox)).collect();
    let page_median = median(&mut dens.clone());
    lines
        .iter()
        .zip(dens.drain(..))
        .map(|(l, d)| {
            let dir = script_dir(&l.text);
            let tight = match ink_band(ink, &l.bbox) {
                Some((lo, hi)) => [l.bbox[0], lo, l.bbox[2], hi],
                None => l.bbox,
            };
            Info {
                bbox: l.bbox,
                tight,
                text: l.text.trim().to_string(),
                dir,
                size: font_size(ink, &l.bbox, dir),
                bold: page_median > 0.0 && d >= BOLD_RATIO * page_median,
                italic: false,
                density: d,
                color: ink_color(color, ink, &l.bbox),
            }
        })
        .collect()
}

#[cfg(test)]
impl Info {
    // A line with nothing measured: for tests of the geometry alone.
    pub(super) fn plain(l: &OcrLine) -> Info {
        Info {
            bbox: l.bbox,
            tight: l.bbox,
            text: l.text.clone(),
            dir: script_dir(&l.text),
            size: l.bbox[3] - l.bbox[1],
            bold: false,
            italic: false,
            density: 0.0,
            color: [0.0; 3],
        }
    }
}

// Native lines keep their font facts; only the colour comes from the raster.
fn measure_native(lines: &[NativeLine], ink: &GrayImage, color: &RgbImage) -> Vec<Info> {
    lines
        .iter()
        .map(|n| {
            let l = &n.line;
            Info {
                bbox: l.bbox,
                tight: l.bbox,
                text: l.text.trim().to_string(),
                dir: script_dir(&l.text),
                size: n.size,
                bold: n.bold,
                italic: n.italic,
                density: ink_density(ink, &l.bbox),
                color: ink_color(color, ink, &l.bbox),
            }
        })
        .collect()
}

pub(super) fn center(b: &[f64; 4]) -> (f64, f64) {
    ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
}

pub(super) fn contains(b: &[f64; 4], p: (f64, f64)) -> bool {
    p.0 >= b[0] && p.0 <= b[2] && p.1 >= b[1] && p.1 <= b[3]
}

pub fn union(a: &[f64; 4], b: &[f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

// (table, row, col) of the cell containing the point.
fn cell_at(tables: &[Table], p: (f64, f64)) -> Option<(usize, usize, usize)> {
    tables.iter().enumerate().find_map(|(t, table)| {
        if !contains(&table.bbox, p) {
            return None;
        }
        table.rows.iter().enumerate().find_map(|(r, row)| {
            row.iter()
                .position(|c| contains(&c.bbox, p))
                .map(|c| (t, r, c))
        })
    })
}

// How text sits inside its container: flush, centred or filling it.
fn placement(text: &[f64; 4], outer: &[f64; 4]) -> Align {
    let w = outer[2] - outer[0];
    let l = text[0] - outer[0];
    let r = outer[2] - text[2];
    if w <= 0.0 || (l < 0.05 * w && r < 0.05 * w) {
        return Align::Start;
    }
    if (l - r).abs() <= 0.06 * w {
        return Align::Center;
    }
    if l < r {
        Align::Start
    } else {
        Align::End
    }
}

struct Part {
    y: f64,
    x: f64,
    h: f64,
    text: String,
}

struct CellAcc {
    parts: Vec<Part>,
    // Indices of the lines added whole.
    whole: Vec<usize>,
    bbox: Option<[f64; 4]>,
    bold: bool,
    italic: bool,
    sizes: Vec<f64>,
    densities: Vec<f64>,
    rtl: usize,
    ltr: usize,
}

fn add_part(acc: &mut CellAcc, bbox: &[f64; 4], text: &str, info: &Info) {
    if text.trim().is_empty() {
        return;
    }
    acc.parts.push(Part {
        y: bbox[1],
        x: bbox[0],
        h: bbox[3] - bbox[1],
        text: text.trim().to_string(),
    });
    acc.bbox = Some(acc.bbox.map_or(*bbox, |b| union(&b, bbox)));
    acc.bold |= info.bold;
    acc.italic |= info.italic;
    acc.sizes.push(info.size);
    acc.densities.push(info.density);
    match script_dir(text) {
        Dir::Rtl => acc.rtl += 1,
        Dir::Ltr => acc.ltr += 1,
        Dir::Auto => {}
    }
}

// Assigns lines to table cells; lines in no cell are returned as free.
fn assign_cells(
    lines: &[OcrLine],
    infos: &[Info],
    tables: &[Table],
) -> (Vec<usize>, Vec<Vec<Vec<CellText>>>) {
    let mut accs: Vec<Vec<Vec<CellAcc>>> = tables
        .iter()
        .map(|t| {
            t.rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|_| CellAcc {
                            parts: Vec::new(),
                            whole: Vec::new(),
                            bbox: None,
                            bold: false,
                            italic: false,
                            sizes: Vec::new(),
                            densities: Vec::new(),
                            rtl: 0,
                            ltr: 0,
                        })
                        .collect()
                })
                .collect()
        })
        .collect();
    let mut free = Vec::new();
    for (i, (line, info)) in lines.iter().zip(infos).enumerate() {
        let Some(home) = cell_at(tables, center(&line.bbox)) else {
            free.push(i);
            continue;
        };
        let split = line
            .words
            .iter()
            .any(|w| matches!(cell_at(tables, center(&w.bbox)), Some(c) if c != home));
        if !split {
            let acc = &mut accs[home.0][home.1][home.2];
            add_part(acc, &line.bbox, &line.text, info);
            acc.whole.push(i);
            continue;
        }
        for w in &line.words {
            let (t, r, c) = cell_at(tables, center(&w.bbox)).unwrap_or(home);
            add_part(&mut accs[t][r][c], &w.bbox, &w.text, info);
        }
    }
    for acc in accs.iter_mut().flatten().flatten() {
        acc.parts.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
        if rows_of(&acc.parts).len() >= MAX_CELL_ROWS {
            free.append(&mut acc.whole);
            *acc = CellAcc {
                parts: Vec::new(),
                whole: Vec::new(),
                bbox: None,
                bold: false,
                italic: false,
                sizes: Vec::new(),
                densities: Vec::new(),
                rtl: 0,
                ltr: 0,
            };
        }
    }
    free.sort_unstable();
    let cells = accs
        .into_iter()
        .zip(tables)
        .map(|(rows, table)| {
            rows.into_iter()
                .zip(&table.rows)
                .map(|(row, boxes)| {
                    row.into_iter()
                        .zip(boxes)
                        .map(|(acc, cb)| finish_cell(acc, &cb.bbox))
                        .collect()
                })
                .collect()
        })
        .collect();
    (free, cells)
}

// Parts sorted by y, grouped into rows.
fn rows_of(parts: &[Part]) -> Vec<Vec<&Part>> {
    let mut rows: Vec<Vec<&Part>> = Vec::new();
    for p in parts {
        match rows.last_mut() {
            Some(row) if p.y - row[0].y <= 0.5 * row[0].h => row.push(p),
            _ => rows.push(vec![p]),
        }
    }
    rows
}

fn finish_cell(mut acc: CellAcc, cell: &[f64; 4]) -> CellText {
    let dir = if acc.rtl > 0 && acc.rtl >= acc.ltr {
        Dir::Rtl
    } else if acc.ltr > 0 {
        Dir::Ltr
    } else {
        Dir::Auto
    };
    // Reading order: rows top to bottom, then along the line direction;
    // parts on one row are joined by spaces, rows by line breaks.
    acc.parts.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());
    let mut rows = rows_of(&acc.parts);
    for row in &mut rows {
        row.sort_by(|a, b| {
            if dir == Dir::Rtl {
                b.x.partial_cmp(&a.x).unwrap()
            } else {
                a.x.partial_cmp(&b.x).unwrap()
            }
        });
    }
    let text = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|p| p.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let align = acc.bbox.map_or(Align::Start, |b| placement(&b, cell));
    CellText {
        text,
        align,
        dir,
        bold: acc.bold,
        italic: acc.italic,
        size_px: median(&mut acc.sizes),
        density: median(&mut acc.densities),
    }
}

struct Para {
    lines: Vec<usize>,
    bbox: [f64; 4],
}

pub(super) fn h_overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let o = a[2].min(b[2]) - a[0].max(b[0]);
    let narrow = (a[2] - a[0]).min(b[2] - b[0]);
    if narrow <= 0.0 {
        0.0
    } else {
        o / narrow
    }
}

fn group(infos: &[Info], free: &[usize]) -> Vec<Para> {
    let mut order: Vec<usize> = free.to_vec();
    order.sort_by(|&a, &b| infos[a].bbox[1].partial_cmp(&infos[b].bbox[1]).unwrap());
    let mut heights: Vec<f64> = order
        .iter()
        .map(|&i| infos[i].bbox[3] - infos[i].bbox[1])
        .collect();
    let gap = PARA_GAP * median(&mut heights);
    let mut paras: Vec<Para> = Vec::new();
    for i in order {
        let line = &infos[i];
        let best = paras
            .iter_mut()
            .filter(|p| {
                let last = &infos[*p.lines.last().unwrap()];
                let same_dir =
                    last.dir == line.dir || last.dir == Dir::Auto || line.dir == Dir::Auto;
                let ratio = line.size / last.size.max(1.0);
                // The line must sit below the last one, not beside it.
                let below = line.bbox[1] > (last.bbox[1] + last.bbox[3]) / 2.0;
                same_dir
                    && below
                    && (1.0 / PARA_SIZE_RATIO..=PARA_SIZE_RATIO).contains(&ratio)
                    && line.bbox[1] - last.bbox[3] < gap
                    && h_overlap(&line.bbox, &p.bbox) >= 0.3
            })
            .max_by(|a, b| {
                h_overlap(&line.bbox, &a.bbox)
                    .partial_cmp(&h_overlap(&line.bbox, &b.bbox))
                    .unwrap()
            });
        match best {
            Some(p) => {
                p.bbox = union(&p.bbox, &line.bbox);
                p.lines.push(i);
            }
            None => paras.push(Para {
                lines: vec![i],
                bbox: line.bbox,
            }),
        }
    }
    paras
}

fn para_dir(lines: &[&Info]) -> Dir {
    let rtl = lines.iter().filter(|l| l.dir == Dir::Rtl).count();
    let ltr = lines.iter().filter(|l| l.dir == Dir::Ltr).count();
    if rtl > 0 && rtl >= ltr {
        Dir::Rtl
    } else if ltr > 0 {
        Dir::Ltr
    } else {
        Dir::Auto
    }
}

fn para_align(lines: &[&Info], bbox: &[f64; 4], page_w: f64) -> Align {
    if lines.len() == 1 {
        return placement(&lines[0].bbox, &[0.0, bbox[1], page_w, bbox[3]]);
    }
    let w = bbox[2] - bbox[0];
    let n = lines.len();
    let full = lines[..n - 1]
        .iter()
        .all(|l| (l.bbox[2] - l.bbox[0]) >= 0.95 * w);
    if n >= 3 && full {
        return Align::Justify;
    }
    let margins: Vec<(f64, f64)> = lines
        .iter()
        .map(|l| (l.bbox[0] - bbox[0], bbox[2] - l.bbox[2]))
        .collect();
    let max_diff = margins
        .iter()
        .map(|(l, r)| (l - r).abs())
        .fold(0.0, f64::max);
    let max_sum = margins.iter().map(|(l, r)| l + r).fold(0.0, f64::max);
    if max_diff <= 0.1 * w && max_sum > 0.05 * w {
        return Align::Center;
    }
    let left: f64 = margins.iter().map(|m| m.0).sum();
    let right: f64 = margins.iter().map(|m| m.1).sum();
    if left <= right {
        Align::Start
    } else {
        Align::End
    }
}

// A line's box takes the paragraph's edge on the side it aligns to when
// it sits close to that edge; a line that starts elsewhere (a form field
// beside a label) keeps its own box. The vertical extent is the ink's.
fn line_box(l: &Info, p: &Para, align: Align) -> [f64; 4] {
    let tol = EDGE_TOL * (p.bbox[2] - p.bbox[0]);
    let (x0, x2) = (l.tight[0], l.tight[2]);
    let near_left = (x0 - p.bbox[0]).abs() <= tol;
    let near_right = (x2 - p.bbox[2]).abs() <= tol;
    let centred = ((x0 + x2) - (p.bbox[0] + p.bbox[2])).abs() <= 2.0 * tol;
    let (x0, x2) = match align {
        Align::Start if near_left => (p.bbox[0], p.bbox[2]),
        Align::End if near_right => (p.bbox[0], p.bbox[2]),
        Align::Center if centred => (p.bbox[0], p.bbox[2]),
        Align::Justify => (p.bbox[0], p.bbox[2]),
        _ => (x0, x2),
    };
    [x0, l.tight[1], x2, l.tight[3]]
}

// One block per line; the last line of a justified paragraph is flush.
fn finish_para(p: &Para, infos: &[Info], page_w: f64, para: usize) -> Vec<TextLine> {
    let lines: Vec<&Info> = p.lines.iter().map(|&i| &infos[i]).collect();
    let n = lines.len();
    let dir = para_dir(&lines);
    let mut sizes: Vec<f64> = lines.iter().map(|l| l.size).collect();
    let size_med = median(&mut sizes);
    let bold = lines.iter().filter(|l| l.bold).count() * 2 > n;
    let italic = lines.iter().filter(|l| l.italic).count() * 2 > n;
    let align = para_align(&lines, &p.bbox, page_w);
    let color = lines
        .iter()
        .max_by(|a, b| {
            (a.bbox[2] - a.bbox[0])
                .partial_cmp(&(b.bbox[2] - b.bbox[0]))
                .unwrap()
        })
        .unwrap()
        .color;
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let align = match align {
                Align::Justify if i + 1 == n && dir == Dir::Rtl => Align::End,
                Align::Justify if i + 1 == n => Align::Start,
                a => a,
            };
            let size_px = if n > 1 {
                l.size.clamp(
                    (1.0 - LINE_SIZE_WINDOW) * size_med,
                    (1.0 + LINE_SIZE_WINDOW) * size_med,
                )
            } else {
                l.size
            };
            TextLine {
                bbox: line_box(l, p, align),
                extent: l.tight[2] - l.tight[0],
                text: l.text.clone(),
                size_px,
                bold,
                italic,
                align,
                dir,
                color,
                para,
            }
        })
        .collect()
}

pub fn layout(lines: &[OcrLine], tables: &[Table], ink: &GrayImage, color: &RgbImage) -> Layout {
    let infos = measure(lines, ink, color);
    layout_infos(lines, &infos, tables, ink.width() as f64, false)
}

pub fn layout_native(
    lines: &[NativeLine],
    tables: &[Table],
    ink: &GrayImage,
    color: &RgbImage,
    column_tables: bool,
) -> Layout {
    let infos = measure_native(lines, ink, color);
    let plain: Vec<OcrLine> = lines.iter().map(|n| n.line.clone()).collect();
    layout_infos(&plain, &infos, tables, ink.width() as f64, column_tables)
}

// Lines in ruled tables become cell text; with `column_tables`, lines that
// line up in columns become tables too; the rest are grouped into paragraphs.
pub(super) fn layout_infos(
    lines: &[OcrLine],
    infos: &[Info],
    tables: &[Table],
    page_w: f64,
    column_tables: bool,
) -> Layout {
    let (mut free, cells) = assign_cells(lines, infos, tables);
    let mut found = Vec::new();
    if column_tables {
        for table in super::columns::find_column_tables(lines, infos, &free) {
            let (rest, cells) = assign_cells(lines, infos, std::slice::from_ref(&table));
            // The line indices in `rest` cover the whole page; keep only ours.
            free.retain(|i| rest.contains(i));
            found.push((table, cells.into_iter().next().unwrap_or_default()));
        }
    }
    let lines = group(infos, &free)
        .iter()
        .enumerate()
        .flat_map(|(i, p)| finish_para(p, infos, page_w, i))
        .collect();
    Layout {
        lines,
        cells,
        column_tables: found,
    }
}

#[cfg(test)]
mod tests {
    use super::super::tables::CellBox;
    use super::*;
    use image::{Luma, Rgb};

    fn line(text: &str, bbox: [f64; 4]) -> OcrLine {
        OcrLine {
            text: text.into(),
            conf: 1.0,
            bbox,
            words: Vec::new(),
        }
    }

    fn blank(w: u32, h: u32) -> (GrayImage, RgbImage) {
        (
            GrayImage::new(w, h),
            RgbImage::from_pixel(w, h, Rgb([255, 255, 255])),
        )
    }

    #[test]
    fn detects_script_direction() {
        assert_eq!(script_dir("المادة الثالثة 25,000"), Dir::Rtl);
        assert_eq!(script_dir("Article 4: Payment"), Dir::Ltr);
        assert_eq!(script_dir("1,400"), Dir::Auto);
    }

    #[test]
    fn groups_paragraphs_and_aligns_them() {
        let (ink, color) = blank(1000, 1000);
        let lines = vec![
            // Three justified lines, then a short last line, flush left.
            line("a", [100.0, 100.0, 900.0, 130.0]),
            line("b", [100.0, 140.0, 900.0, 170.0]),
            line("c", [100.0, 180.0, 900.0, 210.0]),
            line("d", [100.0, 220.0, 500.0, 250.0]),
            // A centred two-line block far below.
            line("e", [300.0, 500.0, 700.0, 530.0]),
            line("f", [400.0, 540.0, 600.0, 570.0]),
            // A single right-flush Arabic line.
            line("نص", [600.0, 800.0, 950.0, 830.0]),
        ];
        let out = layout(&lines, &[], &ink, &color);
        let l = &out.lines;
        assert_eq!(l.len(), 7, "{l:?}");
        let paras: Vec<usize> = l.iter().map(|t| t.para).collect();
        assert_eq!(paras, vec![0, 0, 0, 0, 1, 1, 2]);
        let aligns: Vec<Align> = l.iter().map(|t| t.align).collect();
        assert_eq!(
            aligns,
            vec![
                Align::Justify,
                Align::Justify,
                Align::Justify,
                Align::Start,
                Align::Center,
                Align::Center,
                Align::End,
            ]
        );
        // Lines take the paragraph's measure.
        assert_eq!(l[3].bbox, [100.0, 220.0, 900.0, 250.0]);
        assert_eq!(l[5].bbox, [300.0, 540.0, 700.0, 570.0]);
        assert_eq!(l[3].text, "d");
        // Alignment is physical: a right-flushed Arabic line is `End`.
        assert_eq!(l[6].dir, Dir::Rtl);
    }

    #[test]
    fn far_apart_lines_stay_separate() {
        let (ink, color) = blank(1000, 1000);
        let lines = vec![
            line("a", [100.0, 100.0, 900.0, 130.0]),
            line("b", [100.0, 300.0, 900.0, 330.0]),
        ];
        let out = layout(&lines, &[], &ink, &color);
        assert_ne!(out.lines[0].para, out.lines[1].para);
    }

    #[test]
    fn lines_side_by_side_are_separate_paragraphs() {
        let (ink, color) = blank(1000, 1000);
        let lines = vec![
            line("wide", [100.0, 100.0, 900.0, 130.0]),
            line("ITEM:", [100.0, 140.0, 200.0, 170.0]),
            line("Metal plaque", [250.0, 140.0, 600.0, 170.0]),
        ];
        let out = layout(&lines, &[], &ink, &color);
        let paras: Vec<usize> = out.lines.iter().map(|l| l.para).collect();
        assert_eq!(paras.len(), 3);
        assert_ne!(paras[1], paras[2]);
        assert_eq!(out.lines[2].bbox, [250.0, 140.0, 600.0, 170.0]);
    }

    #[test]
    fn a_line_box_follows_the_ink_and_skips_rules() {
        let (mut ink, color) = blank(1000, 400);
        for y in 120..150 {
            for x in (100..600).step_by(3) {
                ink.put_pixel(x, y, Luma([255]));
            }
        }
        // A speck above and a full-width rule below the glyphs.
        ink.put_pixel(300, 105, Luma([255]));
        for x in 100..700 {
            ink.put_pixel(x, 170, Luma([255]));
        }
        let out = layout(
            &[line("text", [100.0, 100.0, 700.0, 180.0])],
            &[],
            &ink,
            &color,
        );
        assert_eq!(out.lines[0].bbox, [100.0, 120.0, 700.0, 150.0]);
    }

    #[test]
    fn a_heading_does_not_join_the_body() {
        let (mut ink, color) = blank(1000, 1000);
        // Tall glyph bodies for the heading, short ones for the body text.
        for y in 100..160 {
            for x in 100..600 {
                ink.put_pixel(x, y, Luma([255]));
            }
        }
        for y in [175..195, 205..225] {
            for yy in y {
                for x in 100..900 {
                    ink.put_pixel(x, yy, Luma([255]));
                }
            }
        }
        let lines = vec![
            line("Heading", [100.0, 100.0, 600.0, 160.0]),
            line("body one", [100.0, 170.0, 900.0, 200.0]),
            line("body two", [100.0, 200.0, 900.0, 230.0]),
        ];
        let out = layout(&lines, &[], &ink, &color);
        assert_ne!(out.lines[0].para, out.lines[1].para);
        assert_eq!(out.lines[1].para, out.lines[2].para);
        assert!(out.lines[0].size_px > 1.5 * out.lines[1].size_px);
    }

    #[test]
    fn assigns_lines_to_cells() {
        let (ink, color) = blank(1000, 1000);
        let table = Table {
            bbox: [100.0, 100.0, 500.0, 300.0],
            rows: vec![
                vec![
                    CellBox {
                        bbox: [100.0, 100.0, 300.0, 200.0],
                    },
                    CellBox {
                        bbox: [300.0, 100.0, 500.0, 200.0],
                    },
                ],
                vec![
                    CellBox {
                        bbox: [100.0, 200.0, 300.0, 300.0],
                    },
                    CellBox {
                        bbox: [300.0, 200.0, 500.0, 300.0],
                    },
                ],
            ],
        };
        let lines = vec![
            line("البند", [110.0, 130.0, 170.0, 160.0]),
            line("350", [380.0, 130.0, 420.0, 160.0]),
            line("x", [110.0, 230.0, 130.0, 260.0]),
            line("y", [110.0, 270.0, 130.0, 295.0]),
            line("outside", [600.0, 130.0, 700.0, 160.0]),
        ];
        let out = layout(&lines, &[table], &ink, &color);
        assert_eq!(out.lines.len(), 1);
        let cells = &out.cells[0];
        assert_eq!(cells[0][0].text, "البند");
        assert_eq!(cells[0][0].dir, Dir::Rtl);
        assert_eq!(cells[0][0].align, Align::Start);
        assert_eq!(cells[0][1].text, "350");
        assert_eq!(cells[0][1].align, Align::Center);
        assert_eq!(cells[1][0].text, "x\ny");
        assert_eq!(cells[1][1].text, "");
    }

    #[test]
    fn a_boxed_paragraph_is_free_text() {
        let (ink, color) = blank(1000, 1000);
        let table = Table {
            bbox: [0.0, 0.0, 800.0, 400.0],
            rows: vec![vec![CellBox {
                bbox: [0.0, 0.0, 800.0, 400.0],
            }]],
        };
        let lines = vec![
            line("one", [20.0, 20.0, 700.0, 50.0]),
            line("two", [20.0, 60.0, 700.0, 90.0]),
            line("three", [20.0, 100.0, 300.0, 130.0]),
        ];
        let out = layout(&lines, &[table], &ink, &color);
        assert_eq!(out.cells[0][0][0].text, "");
        assert_eq!(out.lines.len(), 3);
        assert!(out.lines.iter().all(|l| l.para == 0));
    }

    #[test]
    fn splits_a_line_spanning_cells_by_words() {
        let (ink, color) = blank(1000, 1000);
        let table = Table {
            bbox: [0.0, 0.0, 400.0, 100.0],
            rows: vec![vec![
                CellBox {
                    bbox: [0.0, 0.0, 200.0, 100.0],
                },
                CellBox {
                    bbox: [200.0, 0.0, 400.0, 100.0],
                },
            ]],
        };
        let mut l = line("4 350", [20.0, 30.0, 260.0, 60.0]);
        l.words = vec![
            crate::ocr::OcrWord {
                text: "4".into(),
                bbox: [20.0, 30.0, 40.0, 60.0],
            },
            crate::ocr::OcrWord {
                text: "350".into(),
                bbox: [220.0, 30.0, 260.0, 60.0],
            },
        ];
        let out = layout(&[l], &[table], &ink, &color);
        assert_eq!(out.cells[0][0][0].text, "4");
        assert_eq!(out.cells[0][0][1].text, "350");
    }

    #[test]
    fn bold_by_ink_density() {
        let mut ink = GrayImage::new(600, 300);
        let color = RgbImage::from_pixel(600, 300, Rgb([0, 0, 0]));
        // Thin strokes: every 8th column; thick: 4 of every 8 columns.
        for y in 20..60 {
            for x in 20..580 {
                if x % 8 == 0 {
                    ink.put_pixel(x, y, Luma([255]));
                }
            }
        }
        for y in 120..160 {
            for x in 20..580 {
                if x % 8 < 4 {
                    ink.put_pixel(x, y, Luma([255]));
                }
            }
        }
        for y in 220..260 {
            for x in 20..580 {
                if x % 8 == 0 {
                    ink.put_pixel(x, y, Luma([255]));
                }
            }
        }
        let lines = vec![
            line("thin", [20.0, 20.0, 580.0, 60.0]),
            line("thick", [20.0, 120.0, 580.0, 160.0]),
            line("thin2", [20.0, 220.0, 580.0, 260.0]),
        ];
        let out = layout(&lines, &[], &ink, &color);
        let bold: Vec<bool> = out.lines.iter().map(|p| p.bold).collect();
        assert_eq!(bold, vec![false, true, false]);
        assert!(out.lines.iter().all(|p| p.color == [0.0; 3]));
    }

    #[test]
    fn coloured_ink_is_kept() {
        let mut ink = GrayImage::new(200, 100);
        let color = RgbImage::from_pixel(200, 100, Rgb([200, 30, 30]));
        for y in 20..60 {
            for x in 20..180 {
                ink.put_pixel(x, y, Luma([255]));
            }
        }
        let out = layout(&[line("red", [20.0, 20.0, 180.0, 60.0])], &[], &ink, &color);
        let c = out.lines[0].color;
        assert!(c[0] > 0.7 && c[1] < 0.2, "{c:?}");
    }
}
