// The document handed to the Word and Excel writers: pages of paragraphs,
// tables and pictures in reading order, in PDF points.
use serde::{Deserialize, Serialize};

use crate::scan::blocks::{Align, Dir};
use crate::scan::{AnalysedTable, Analysis};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Para {
    pub text: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: [f64; 3],
    pub align: Align,
    pub dir: Dir,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CellItem {
    pub text: String,
    // Grid columns the cell covers.
    pub span: usize,
    pub align: Align,
    pub dir: Dir,
    pub bold: bool,
    pub italic: bool,
    pub size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableItem {
    // Column widths in points.
    pub cols: Vec<f64>,
    pub rows: Vec<Vec<CellItem>>,
    pub ruled: bool,
    pub rtl: bool,
    // Grid lines on the page, in points with the origin top-left: one more
    // than the columns and rows.
    #[serde(default)]
    pub col_edges: Vec<f64>,
    #[serde(default)]
    pub row_edges: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Picture {
    pub png_b64: String,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Item {
    Para(Para),
    Table(TableItem),
    Picture(Picture),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    // Top-left origin, points.
    pub bbox: [f64; 4],
    pub item: Item,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub number: u32,
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<Block>,
}

impl Page {
    pub fn rtl(&self) -> bool {
        let (mut rtl, mut ltr) = (0, 0);
        for b in &self.blocks {
            match &b.item {
                Item::Para(p) => match p.dir {
                    Dir::Rtl => rtl += p.text.len(),
                    Dir::Ltr => ltr += p.text.len(),
                    Dir::Auto => {}
                },
                Item::Table(t) => {
                    if t.rtl {
                        rtl += 1
                    } else {
                        ltr += 1
                    }
                }
                Item::Picture(_) => {}
            }
        }
        rtl > ltr
    }
}

// Gap between blocks, as a share of the page's shorter side, that
// separates them for reading order.
const CUT_GAP: f64 = 0.012;

fn vertical_gap(blocks: &[&Block], h: f64) -> Option<(f64, f64)> {
    let mut edges: Vec<(f64, f64)> = blocks.iter().map(|b| (b.bbox[1], b.bbox[3])).collect();
    edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut best: Option<(f64, f64)> = None;
    let mut reach = edges[0].1;
    for e in &edges[1..] {
        if e.0 > reach {
            let gap = e.0 - reach;
            if gap >= h && best.is_none_or(|b| gap > b.1 - b.0) {
                best = Some((reach, e.0));
            }
        }
        reach = reach.max(e.1);
    }
    best
}

fn horizontal_gap(blocks: &[&Block], w: f64) -> Option<(f64, f64)> {
    let mut edges: Vec<(f64, f64)> = blocks.iter().map(|b| (b.bbox[0], b.bbox[2])).collect();
    edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut best: Option<(f64, f64)> = None;
    let mut reach = edges[0].1;
    for e in &edges[1..] {
        if e.0 > reach {
            let gap = e.0 - reach;
            if gap >= w && best.is_none_or(|b| gap > b.1 - b.0) {
                best = Some((reach, e.0));
            }
        }
        reach = reach.max(e.1);
    }
    best
}

// Recursive XY cut: split at the widest band of white, horizontal (rows)
// or vertical (columns), until only a sorted run is left.
fn order<'a>(blocks: Vec<&'a Block>, min_gap: f64, rtl: bool, out: &mut Vec<&'a Block>) {
    if blocks.len() <= 1 {
        out.extend(blocks);
        return;
    }
    let v = vertical_gap(&blocks, min_gap);
    let h = horizontal_gap(&blocks, min_gap);
    let width = |g: Option<(f64, f64)>| g.map_or(0.0, |(lo, hi)| hi - lo);
    if let (Some((lo, hi)), true) = (v, width(v) >= width(h)) {
        let mid = (lo + hi) / 2.0;
        let (top, bottom): (Vec<_>, Vec<_>) = blocks.into_iter().partition(|b| b.bbox[3] <= mid);
        order(top, min_gap, rtl, out);
        order(bottom, min_gap, rtl, out);
        return;
    }
    if let Some((lo, hi)) = h {
        let mid = (lo + hi) / 2.0;
        let (left, right): (Vec<_>, Vec<_>) = blocks.into_iter().partition(|b| b.bbox[2] <= mid);
        // The columns run only where both sides have content: whatever
        // sits wholly above or below that band is read before or after.
        let extent = |side: &[&Block]| {
            side.iter().fold((f64::MAX, f64::MIN), |(t, b), blk| {
                (t.min(blk.bbox[1]), b.max(blk.bbox[3]))
            })
        };
        let ((lt, lb), (rt, rb)) = (extent(&left), extent(&right));
        let (band_top, band_bottom) = (lt.max(rt), lb.min(rb));
        let mut above = Vec::new();
        let mut below = Vec::new();
        let mut keep = |side: Vec<&'a Block>| -> Vec<&'a Block> {
            side.into_iter()
                .filter(|b| {
                    if b.bbox[3] <= band_top {
                        above.push(*b);
                        false
                    } else if b.bbox[1] >= band_bottom {
                        below.push(*b);
                        false
                    } else {
                        true
                    }
                })
                .collect()
        };
        let (left, right) = (keep(left), keep(right));
        order(above, min_gap, rtl, out);
        if rtl {
            order(right, min_gap, rtl, out);
            order(left, min_gap, rtl, out);
        } else {
            order(left, min_gap, rtl, out);
            order(right, min_gap, rtl, out);
        }
        order(below, min_gap, rtl, out);
        return;
    }
    // No clear cut: peel off the topmost block and look again, since the
    // rest may well split once it is gone.
    let mut rest = blocks;
    let first = (0..rest.len())
        .min_by(|&i, &j| {
            let (a, b) = (rest[i], rest[j]);
            a.bbox[1].partial_cmp(&b.bbox[1]).unwrap().then_with(|| {
                if rtl {
                    b.bbox[2].partial_cmp(&a.bbox[2]).unwrap()
                } else {
                    a.bbox[0].partial_cmp(&b.bbox[0]).unwrap()
                }
            })
        })
        .unwrap();
    out.push(rest.remove(first));
    order(rest, min_gap, rtl, out);
}

pub fn reading_order(page: &mut Page) {
    let rtl = page.rtl();
    let min_gap = CUT_GAP * page.width.min(page.height);
    let mut ordered: Vec<&Block> = Vec::new();
    order(page.blocks.iter().collect(), min_gap, rtl, &mut ordered);
    let blocks: Vec<Block> = ordered.into_iter().cloned().collect();
    page.blocks = blocks;
}

fn scale(b: &[f64; 4], sx: f64, sy: f64) -> [f64; 4] {
    [b[0] * sx, b[1] * sy, b[2] * sx, b[3] * sy]
}

// A label at the start of a line ("Tel:", "العنوان:") marks a new field.
const LABEL_MAX: usize = 30;

fn starts_with_label(t: &str) -> bool {
    let Some(colon) = t.find(':') else {
        return false;
    };
    let head = &t[..colon];
    colon > 0
        && head.chars().count() <= LABEL_MAX
        && head.split_whitespace().count() <= 3
        && (t[colon + 1..].starts_with(' ') || colon + 1 == t.len())
}

// Whether the first word of `next` would have fitted at the end of `line`:
// then the break between them was deliberate.
fn room_for_next(line: &crate::scan::text::TextLine, next: &str, measure: f64) -> bool {
    let chars = line.text.chars().count().max(1) as f64;
    let per_char = line.extent / chars;
    let word = next.split_whitespace().next().unwrap_or("").chars().count() as f64 + 1.0;
    line.extent + word * per_char <= measure
}

// Joins the lines of one paragraph: a wrapped line flows into the next
// with a space (or closes a hyphenated word); a line that stops short
// although the next word had room, or a line that starts a labelled
// field, keeps its line break.
fn join_lines(lines: &[&crate::scan::text::TextLine]) -> String {
    let measure = lines
        .iter()
        .map(|l| l.bbox[2] - l.bbox[0])
        .fold(0.0, f64::max);
    let mut text = String::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.text.trim();
        if i == 0 {
            text.push_str(t);
            continue;
        }
        let hyphenated = text.ends_with('-')
            && text
                .chars()
                .rev()
                .nth(1)
                .is_some_and(|c| c.is_ascii_alphabetic());
        // Labels only count outside justified prose, where "note: ..." is
        // just a sentence.
        let label = l.align != Align::Justify
            && lines[i - 1].align != Align::Justify
            && starts_with_label(t);
        if room_for_next(lines[i - 1], t, measure) || label {
            text.push('\n');
        } else if hyphenated {
            text.pop();
        } else {
            text.push(' ');
        }
        text.push_str(t);
    }
    text
}

fn table_item(t: &AnalysedTable, sx: f64, sy: f64) -> TableItem {
    // Grid columns from every distinct cell edge.
    let mut edges: Vec<f64> = t
        .table
        .rows
        .iter()
        .flatten()
        .flat_map(|c| [c.bbox[0], c.bbox[2]])
        .collect();
    edges.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let tol = 2.0 / sx.max(1e-9);
    edges.dedup_by(|a, b| (*a - *b).abs() <= tol);
    let col_index = |x: f64| {
        edges
            .iter()
            .enumerate()
            .min_by(|a, b| (a.1 - x).abs().partial_cmp(&(b.1 - x).abs()).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let cols: Vec<f64> = edges.windows(2).map(|w| (w[1] - w[0]) * sx).collect();
    let col_edges: Vec<f64> = edges.iter().map(|e| e * sx).collect();
    let mut row_edges: Vec<f64> = t
        .table
        .rows
        .iter()
        .flatten()
        .flat_map(|c| [c.bbox[1], c.bbox[3]])
        .collect();
    row_edges.sort_by(|a, b| a.partial_cmp(b).unwrap());
    row_edges.dedup_by(|a, b| (*a - *b).abs() <= 2.0 / sy.max(1e-9));
    let row_edges: Vec<f64> = row_edges.iter().map(|e| e * sy).collect();
    let mut rtl_cells = 0;
    let mut ltr_cells = 0;
    let rows = t
        .table
        .rows
        .iter()
        .zip(&t.cells)
        .map(|(row, texts)| {
            row.iter()
                .zip(texts)
                .map(|(cb, ct)| {
                    match ct.dir {
                        Dir::Rtl => rtl_cells += 1,
                        Dir::Ltr => ltr_cells += 1,
                        Dir::Auto => {}
                    }
                    let span = col_index(cb.bbox[2])
                        .saturating_sub(col_index(cb.bbox[0]))
                        .max(1);
                    CellItem {
                        text: ct.text.clone(),
                        span,
                        align: ct.align,
                        dir: ct.dir,
                        bold: ct.bold,
                        italic: ct.italic,
                        size: ct.size_px * sx,
                    }
                })
                .collect()
        })
        .collect();
    TableItem {
        cols,
        rows,
        ruled: t.ruled,
        rtl: rtl_cells > ltr_cells,
        col_edges,
        row_edges,
    }
}

// Builds the page from an analysis: paragraphs from the grouped lines,
// tables, and pictures, then puts them in reading order.
pub fn page_from(a: &Analysis, number: u32) -> Page {
    let mut blocks = Vec::new();
    let mut paras: Vec<Vec<&crate::scan::text::TextLine>> = Vec::new();
    for l in &a.lines {
        match paras.iter_mut().find(|p| p[0].para == l.para) {
            Some(p) => p.push(l),
            None => paras.push(vec![l]),
        }
    }
    for lines in paras {
        let bbox = lines
            .iter()
            .map(|l| l.bbox)
            .reduce(|x, y| crate::scan::text::union(&x, &y))
            .unwrap();
        let first = lines[0];
        let mut sizes: Vec<f64> = lines.iter().map(|l| l.size_px * a.sy).collect();
        blocks.push(Block {
            bbox: scale(&bbox, a.sx, a.sy),
            item: Item::Para(Para {
                text: join_lines(&lines),
                size: crate::scan::text::median(&mut sizes),
                bold: first.bold,
                italic: first.italic,
                color: first.color,
                align: first.align,
                dir: first.dir,
            }),
        });
    }
    for t in &a.tables {
        let item = table_item(t, a.sx, a.sy);
        // A lone empty box is a frame around text that already flows.
        if item.rows.len() == 1 && item.rows[0].len() == 1 && item.rows[0][0].text.trim().is_empty()
        {
            continue;
        }
        blocks.push(Block {
            bbox: scale(&t.table.bbox, a.sx, a.sy),
            item: Item::Table(item),
        });
    }
    for f in &a.figures {
        let b = scale(&f.bbox, a.sx, a.sy);
        blocks.push(Block {
            bbox: b,
            item: Item::Picture(Picture {
                png_b64: f.png_b64.clone(),
                width: b[2] - b[0],
                height: b[3] - b[1],
            }),
        });
    }
    let mut page = Page {
        number,
        width: a.width_pt,
        height: a.height_pt,
        blocks,
    };
    reading_order(&mut page);
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(bbox: [f64; 4], text: &str, dir: Dir) -> Block {
        Block {
            bbox,
            item: Item::Para(Para {
                text: text.into(),
                size: 10.0,
                bold: false,
                italic: false,
                color: [0.0; 3],
                align: Align::Start,
                dir,
            }),
        }
    }

    fn texts(p: &Page) -> Vec<&str> {
        p.blocks
            .iter()
            .map(|b| match &b.item {
                Item::Para(p) => p.text.as_str(),
                _ => "?",
            })
            .collect()
    }

    #[test]
    fn two_columns_read_left_column_first() {
        let mut page = Page {
            number: 1,
            width: 600.0,
            height: 800.0,
            blocks: vec![
                para([40.0, 40.0, 560.0, 60.0], "title", Dir::Ltr),
                para([320.0, 100.0, 560.0, 300.0], "right 1", Dir::Ltr),
                para([40.0, 100.0, 280.0, 300.0], "left 1", Dir::Ltr),
                para([320.0, 320.0, 560.0, 500.0], "right 2", Dir::Ltr),
                para([40.0, 320.0, 280.0, 500.0], "left 2", Dir::Ltr),
                para([40.0, 700.0, 560.0, 720.0], "footer", Dir::Ltr),
            ],
        };
        reading_order(&mut page);
        assert_eq!(
            texts(&page),
            vec!["title", "left 1", "left 2", "right 1", "right 2", "footer"]
        );
    }

    #[test]
    fn a_full_width_block_touching_two_columns_is_peeled_first() {
        // A table runs the width; a stamp (left) and totals (right) sit
        // right under it, and a footer line lies under both.
        let mut page = Page {
            number: 1,
            width: 600.0,
            height: 800.0,
            blocks: vec![
                para([40.0, 300.0, 560.0, 400.0], "table", Dir::Rtl),
                para([60.0, 403.0, 150.0, 480.0], "stamp", Dir::Rtl),
                para([430.0, 410.0, 560.0, 450.0], "totals", Dir::Rtl),
                para([180.0, 440.0, 300.0, 470.0], "signature", Dir::Rtl),
                para([280.0, 560.0, 320.0, 570.0], "footer", Dir::Rtl),
            ],
        };
        reading_order(&mut page);
        assert_eq!(
            texts(&page),
            // Right to left inside the lower band too: signature before stamp.
            vec!["table", "totals", "signature", "stamp", "footer"]
        );
    }

    #[test]
    fn arabic_pages_read_the_right_column_first() {
        let mut page = Page {
            number: 1,
            width: 600.0,
            height: 800.0,
            blocks: vec![
                para([40.0, 100.0, 280.0, 300.0], "يسار", Dir::Rtl),
                para([320.0, 100.0, 560.0, 300.0], "يمين", Dir::Rtl),
            ],
        };
        reading_order(&mut page);
        assert_eq!(texts(&page), vec!["يمين", "يسار"]);
    }

    #[test]
    fn hyphenated_latin_lines_join_without_a_space() {
        use crate::scan::text::TextLine;
        let mk = |t: &str| TextLine {
            bbox: [0.0, 0.0, 100.0, 10.0],
            extent: 100.0,
            text: t.into(),
            size_px: 10.0,
            bold: false,
            italic: false,
            align: Align::Start,
            dir: Dir::Ltr,
            color: [0.0; 3],
            para: 0,
        };
        let (a, b) = (mk("infor-"), mk("mation here"));
        assert_eq!(join_lines(&[&a, &b]), "information here");
        let (c, d) = (mk("كلمة"), mk("أخرى"));
        assert_eq!(join_lines(&[&c, &d]), "كلمة أخرى");
        // A short first line is a deliberate break.
        let mut e = mk("Street 1");
        e.extent = 40.0;
        let f = mk("City");
        assert_eq!(join_lines(&[&e, &f]), "Street 1\nCity");
        // A full line followed by a labelled field keeps the break too.
        let (g, h) = (
            mk("العنوان: جدة، حي الروضة، شارع"),
            mk("الرقم الضريبي: 3101"),
        );
        assert_eq!(
            join_lines(&[&g, &h]),
            "العنوان: جدة، حي الروضة، شارع\nالرقم الضريبي: 3101"
        );
        assert!(starts_with_label("Tel: 123") && !starts_with_label("see http://x"));
        assert!(!starts_with_label(
            "the long preamble of this sentence says: no"
        ));
        // Inside justified prose a colon is not a label.
        let mut j1 = mk("For decades the orthodoxy in distribution was");
        j1.align = Align::Justify;
        let mut j2 = mk("simple: bigger is better. A single enormous");
        j2.align = Align::Justify;
        assert_eq!(
            join_lines(&[&j1, &j2]),
            "For decades the orthodoxy in distribution was simple: bigger is better. A single enormous"
        );
    }
}
