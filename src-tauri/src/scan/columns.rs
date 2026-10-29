// Tables drawn without rules: runs of rows whose words line up in columns
// separated by gaps that stay clear down the whole block.
use super::tables::{CellBox, Table};
use super::text::{union, Info};
use crate::ocr::OcrLine;

// Rows a block needs; two suffice when every gap is at least WIDE_GAP.
const MIN_ROWS: usize = 3;
const MIN_COLS: usize = 2;
// Gap widths as multiples of the block's median line height.
const GAP_MIN: f64 = 1.2;
const WIDE_GAP: f64 = 2.5;
// Rows further apart than this many line heights end a block.
const ROW_GAP: f64 = 1.6;
// Share of rows in which a column gap must be clear of ink.
const CLEAR_SHARE: f64 = 0.8;
// Share of rows that must fill a column for it to count.
const COL_FILL: f64 = 0.5;
// Lines whose centres differ by less than this share of their height sit
// on one row.
const ROW_CENTRE: f64 = 0.5;
// A row this close (in line heights) to a neighbour whose columns it
// does not clash with (a superset, or none in common) is another line of
// the same cells: a two-line header, a wrapped description.
const CONTINUATION_GAP: f64 = 0.35;
// A column whose cells hold this many words or more (by median) is a
// column of prose; two such columns are text columns, not a table.
const PROSE_WORDS: usize = 5;

#[derive(Clone)]
struct Row {
    y0: f64,
    y1: f64,
    // Word boxes with the index of their line.
    words: Vec<([f64; 4], usize)>,
}

fn same_row(r: &Row, b: &[f64; 4]) -> bool {
    let (rc, bc) = ((r.y0 + r.y1) / 2.0, (b[1] + b[3]) / 2.0);
    let h = (r.y1 - r.y0).max(b[3] - b[1]);
    (rc - bc).abs() <= ROW_CENTRE * h
}

fn rows(lines: &[OcrLine], infos: &[Info], free: &[usize]) -> Vec<Row> {
    let mut order: Vec<usize> = free.to_vec();
    order.sort_by(|&a, &b| infos[a].bbox[1].partial_cmp(&infos[b].bbox[1]).unwrap());
    let mut rows: Vec<Row> = Vec::new();
    for i in order {
        let b = infos[i].bbox;
        let words: Vec<([f64; 4], usize)> = if lines[i].words.is_empty() {
            vec![(b, i)]
        } else {
            lines[i].words.iter().map(|w| (w.bbox, i)).collect()
        };
        match rows.iter_mut().find(|r| same_row(r, &b)) {
            Some(r) => {
                r.y0 = r.y0.min(b[1]);
                r.y1 = r.y1.max(b[3]);
                r.words.extend(words);
            }
            None => rows.push(Row {
                y0: b[1],
                y1: b[3],
                words,
            }),
        }
    }
    rows.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap());
    rows
}

// Columns a row has words in.
fn occupied(r: &Row, seps: &[f64]) -> Vec<usize> {
    let mut cols: Vec<usize> = r
        .words
        .iter()
        .map(|(b, _)| col_of(seps, (b[0] + b[2]) / 2.0))
        .collect();
    cols.sort_unstable();
    cols.dedup();
    cols
}

// Folds rows that only continue a neighbour's cells (the second line of
// a two-line header or description) into that neighbour. The row with
// more columns absorbs the other.
fn merge_continuations(mut rows: Vec<Row>, seps: &[f64], line_h: f64) -> Vec<Row> {
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..rows.len() {
            let mine = occupied(&rows[i], seps);
            let candidates = [i.checked_sub(1), (i + 1 < rows.len()).then_some(i + 1)];
            for n in candidates.into_iter().flatten() {
                let gap = if n < i {
                    rows[i].y0 - rows[n].y1
                } else {
                    rows[n].y0 - rows[i].y1
                };
                let theirs = occupied(&rows[n], seps);
                let subset = mine.iter().all(|c| theirs.contains(c)) && mine.len() < theirs.len();
                let disjoint =
                    mine.iter().all(|c| !theirs.contains(c)) && mine.len() <= theirs.len();
                if gap < CONTINUATION_GAP * line_h && (subset || disjoint) {
                    let r = rows.remove(i);
                    let target = if n < i { n } else { n - 1 };
                    rows[target].y0 = rows[target].y0.min(r.y0);
                    rows[target].y1 = rows[target].y1.max(r.y1);
                    rows[target].words.extend(r.words);
                    changed = true;
                    break;
                }
            }
            if changed {
                break;
            }
        }
    }
    rows
}

fn median_height(rows: &[Row]) -> f64 {
    let mut h: Vec<f64> = rows.iter().map(|r| r.y1 - r.y0).collect();
    super::text::median(&mut h)
}

// x positions splitting the block into columns: centres of the gaps that
// are clear in enough rows, wide enough, and strictly inside the block.
fn separators(rows: &[Row], line_h: f64) -> Vec<f64> {
    let (mut x0, mut x1) = (f64::MAX, f64::MIN);
    for (b, _) in rows.iter().flat_map(|r| &r.words) {
        x0 = x0.min(b[0]);
        x1 = x1.max(b[2]);
    }
    if x1 <= x0 {
        return Vec::new();
    }
    let step = (line_h / 8.0).max(0.5);
    let n = ((x1 - x0) / step).ceil() as usize;
    let max_blocked = ((1.0 - CLEAR_SHARE) * rows.len() as f64).floor() as usize;
    let covered = |x: f64| {
        rows.iter()
            .filter(|r| r.words.iter().any(|(b, _)| x >= b[0] && x <= b[2]))
            .count()
    };
    let mut seps = Vec::new();
    let mut run: Option<usize> = None;
    for k in 0..=n {
        let x = x0 + k as f64 * step;
        let clear = covered(x) <= max_blocked;
        match (clear, run) {
            (true, None) => run = Some(k),
            (false, Some(s)) => {
                let (a, b) = (x0 + s as f64 * step, x0 + (k - 1) as f64 * step);
                if s > 0 && b - a >= GAP_MIN * line_h {
                    seps.push((a + b) / 2.0);
                }
                run = None;
            }
            _ => {}
        }
    }
    seps
}

fn col_of(seps: &[f64], x: f64) -> usize {
    seps.iter().filter(|&&s| x > s).count()
}

// A row whose word straddles a separator is not part of the grid unless
// it sits in the middle of the block (a cell spanning columns).
fn straddles(row: &Row, seps: &[f64]) -> bool {
    row.words
        .iter()
        .any(|(b, _)| col_of(seps, b[0]) != col_of(seps, b[2]))
}

fn fills_columns(rows: &[Row], seps: &[f64]) -> bool {
    let cols = seps.len() + 1;
    let need = (COL_FILL * rows.len() as f64).ceil() as usize;
    let filled = (0..cols).all(|c| {
        rows.iter()
            .filter(|r| {
                r.words
                    .iter()
                    .any(|(b, _)| col_of(seps, (b[0] + b[2]) / 2.0) == c)
            })
            .count()
            >= need
    });
    let multi = rows
        .iter()
        .filter(|r| {
            let mut seen: Vec<usize> = r
                .words
                .iter()
                .map(|(b, _)| col_of(seps, (b[0] + b[2]) / 2.0))
                .collect();
            seen.sort_unstable();
            seen.dedup();
            seen.len() >= 2
        })
        .count();
    filled && multi >= 2
}

// Columns whose cells read like running text.
fn prose_columns(rows: &[Row], seps: &[f64]) -> usize {
    let cols = seps.len() + 1;
    (0..cols)
        .filter(|&c| {
            let mut counts: Vec<usize> = rows
                .iter()
                .map(|r| {
                    r.words
                        .iter()
                        .filter(|(b, _)| col_of(seps, (b[0] + b[2]) / 2.0) == c)
                        .count()
                })
                .filter(|&n| n > 0)
                .collect();
            counts.sort_unstable();
            !counts.is_empty() && counts[counts.len() / 2] >= PROSE_WORDS
        })
        .count()
}

fn grid(rows: &[Row], line_h: f64) -> Option<Vec<f64>> {
    let seps = separators(rows, line_h);
    if seps.len() + 1 < MIN_COLS {
        return None;
    }
    if prose_columns(rows, &seps) >= 2 {
        return None;
    }
    let gaps: Vec<f64> = seps.iter().map(|s| gap_width(rows, *s)).collect();
    let wide = gaps.iter().all(|&g| g >= WIDE_GAP * line_h);
    if rows.len() < MIN_ROWS && !(rows.len() == 2 && wide) {
        return None;
    }
    fills_columns(rows, &seps).then_some(seps)
}

fn build(rows: &[Row], seps: &[f64], line_h: f64) -> Table {
    let cols = seps.len() + 1;
    let (mut x0, mut x1) = (f64::MAX, f64::MIN);
    for (b, _) in rows.iter().flat_map(|r| &r.words) {
        x0 = x0.min(b[0]);
        x1 = x1.max(b[2]);
    }
    let pad = 0.25 * line_h;
    let edges: Vec<f64> = std::iter::once(x0 - pad)
        .chain(seps.iter().copied())
        .chain(std::iter::once(x1 + pad))
        .collect();
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        // Row bands meet halfway between neighbours so every word has a home.
        let top = if i == 0 {
            r.y0 - pad
        } else {
            (rows[i - 1].y1 + r.y0) / 2.0
        };
        let bottom = if i + 1 == rows.len() {
            r.y1 + pad
        } else {
            (r.y1 + rows[i + 1].y0) / 2.0
        };
        let mut cells: Vec<CellBox> = (0..cols)
            .map(|c| CellBox {
                bbox: [edges[c], top, edges[c + 1], bottom],
            })
            .collect();
        // Words crossing a separator merge the cells they span.
        let mut spans: Vec<(usize, usize)> = r
            .words
            .iter()
            .map(|(b, _)| (col_of(seps, b[0]), col_of(seps, b[2])))
            .filter(|(a, b)| a < b)
            .collect();
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (a, b) in spans {
            match merged.last_mut() {
                Some(m) if a <= m.1 => m.1 = m.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        for (a, b) in merged.into_iter().rev() {
            let bbox = union(&cells[a].bbox, &cells[b].bbox);
            cells.drain(a + 1..=b);
            cells[a].bbox = bbox;
        }
        out.push(cells);
    }
    let bbox = out
        .iter()
        .flatten()
        .map(|c| c.bbox)
        .reduce(|a, b| union(&a, &b))
        .unwrap_or([0.0; 4]);
    Table { bbox, rows: out }
}

// Rows clustered afresh from word boxes alone.
fn rows_from_words(mut words: Vec<([f64; 4], usize)>) -> Vec<Row> {
    words.sort_by(|a, b| a.0[1].partial_cmp(&b.0[1]).unwrap());
    let mut rows: Vec<Row> = Vec::new();
    for (b, l) in words {
        match rows.iter_mut().find(|r| same_row(r, &b)) {
            Some(r) => {
                r.y0 = r.y0.min(b[1]);
                r.y1 = r.y1.max(b[3]);
                r.words.push((b, l));
            }
            None => rows.push(Row {
                y0: b[1],
                y1: b[3],
                words: vec![(b, l)],
            }),
        }
    }
    rows.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap());
    rows
}

// The block's words split at `x`, each side clustered into its own rows
// so lines of the other column no longer stretch them.
fn split_at(block: &[Row], x: f64) -> (Vec<Row>, Vec<Row>) {
    let (l, r): (Vec<_>, Vec<_>) = block
        .iter()
        .flat_map(|r| r.words.iter().cloned())
        .partition(|(b, _)| (b[0] + b[2]) / 2.0 < x);
    (rows_from_words(l), rows_from_words(r))
}

// Edge rows with words in a single column are text above or below the
// table (a caption, the end of a paragraph), not part of it.
fn trim_single_column_edges(rows: &mut Vec<Row>, seps: &[f64]) {
    while rows.len() > 2 && occupied(&rows[0], seps).len() < 2 {
        rows.remove(0);
    }
    while rows.len() > 2 && occupied(rows.last().unwrap(), seps).len() < 2 {
        rows.pop();
    }
}

// Blocks of rows close together.
fn blocks_of(rows: Vec<Row>, line_h: f64) -> Vec<Vec<Row>> {
    let mut blocks: Vec<Vec<Row>> = Vec::new();
    for r in rows {
        match blocks.last_mut() {
            Some(b) if r.y0 - b.last().unwrap().y1 < ROW_GAP * line_h => b.push(r),
            _ => blocks.push(vec![r]),
        }
    }
    blocks
}

// Finds tables in a region's rows. A region laid out in text columns is
// first cut at its gutters so each column is read on its own.
fn detect(rows: Vec<Row>, line_h: f64, depth: usize, out: &mut Vec<Table>) {
    for block in blocks_of(rows, line_h) {
        let seps = separators(&block, line_h);
        if depth < 3 && seps.len() + 1 >= 2 && prose_columns(&block, &seps) >= 2 {
            // Cut at the widest gutter and read both sides separately.
            let widest = seps
                .iter()
                .copied()
                .max_by(|a, b| {
                    gap_width(&block, *a)
                        .partial_cmp(&gap_width(&block, *b))
                        .unwrap()
                })
                .unwrap();
            let (left, right) = split_at(&block, widest);
            detect(left, line_h, depth + 1, out);
            detect(right, line_h, depth + 1, out);
            continue;
        }
        // Grow a grid row by row; when the columns break, close the table.
        let mut start = 0;
        while start < block.len() {
            let mut best: Option<(usize, Vec<f64>)> = None;
            for end in start + 2..=block.len() {
                match grid(&block[start..end], line_h) {
                    Some(seps) => best = Some((end, seps)),
                    None if best.is_some() => break,
                    None => {}
                }
            }
            match best {
                Some((end, seps)) => {
                    let mut rows = block[start..end].to_vec();
                    // A title above or a note below that runs across the
                    // columns is not part of the table.
                    while rows.len() > 2 && straddles(&rows[0], &seps) {
                        rows.remove(0);
                    }
                    while rows.len() > 2 && straddles(rows.last().unwrap(), &seps) {
                        rows.pop();
                    }
                    trim_single_column_edges(&mut rows, &seps);
                    if let Some(seps) = grid(&rows, line_h) {
                        let rows = merge_continuations(rows, &seps, line_h);
                        out.push(build(&rows, &seps, line_h));
                    }
                    start = end;
                }
                None => start += 1,
            }
        }
    }
}

// Width of the clear gap around a separator.
fn gap_width(rows: &[Row], s: f64) -> f64 {
    let left = rows
        .iter()
        .flat_map(|r| &r.words)
        .map(|(b, _)| b[2])
        .filter(|&e| e <= s)
        .fold(f64::MIN, f64::max);
    let right = rows
        .iter()
        .flat_map(|r| &r.words)
        .map(|(b, _)| b[0])
        .filter(|&e| e >= s)
        .fold(f64::MAX, f64::min);
    right - left
}

pub(super) fn find_column_tables(lines: &[OcrLine], infos: &[Info], free: &[usize]) -> Vec<Table> {
    let rows = rows(lines, infos, free);
    if rows.len() < 2 {
        return Vec::new();
    }
    let line_h = median_height(&rows);
    if line_h <= 0.0 {
        return Vec::new();
    }
    let mut tables = Vec::new();
    detect(rows, line_h, 0, &mut tables);
    tables
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::OcrWord;

    fn word(text: &str, x0: f64, y0: f64, w: f64) -> OcrWord {
        OcrWord {
            text: text.into(),
            bbox: [x0, y0, x0 + w, y0 + 20.0],
        }
    }

    fn line(words: Vec<OcrWord>) -> OcrLine {
        let bbox = words
            .iter()
            .map(|w| w.bbox)
            .reduce(|a, b| union(&a, &b))
            .unwrap();
        OcrLine {
            text: words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            conf: 1.0,
            bbox,
            words,
        }
    }

    fn infos(lines: &[OcrLine]) -> Vec<Info> {
        lines.iter().map(Info::plain).collect()
    }

    #[test]
    fn finds_a_three_column_list() {
        let mut lines = Vec::new();
        for r in 0..4 {
            let y = 100.0 + r as f64 * 30.0;
            lines.push(line(vec![
                word("item", 50.0, y, 60.0),
                word("qty", 300.0, y, 40.0),
                word("price", 500.0, y, 70.0),
            ]));
        }
        // Prose below, far away, must not become a table.
        for r in 0..3 {
            let y = 400.0 + r as f64 * 30.0;
            lines.push(line(vec![
                word("some", 50.0, y, 60.0),
                word("prose", 118.0, y + 1.0, 70.0),
                word("text", 196.0, y, 60.0),
            ]));
        }
        let free: Vec<usize> = (0..lines.len()).collect();
        let tables = find_column_tables(&lines, &infos(&lines), &free);
        assert_eq!(tables.len(), 1, "{tables:?}");
        let t = &tables[0];
        assert_eq!(t.rows.len(), 4);
        assert!(t.rows.iter().all(|r| r.len() == 3), "{t:?}");
        assert!(t.bbox[1] < 100.0 && t.bbox[3] > 210.0 && t.bbox[3] < 300.0);
    }

    #[test]
    fn a_spanning_title_is_left_out() {
        let mut lines = vec![line(vec![word(
            "Price list for the whole year",
            50.0,
            60.0,
            500.0,
        )])];
        for r in 0..3 {
            let y = 100.0 + r as f64 * 30.0;
            lines.push(line(vec![
                word("a", 50.0, y, 60.0),
                word("b", 400.0, y, 60.0),
            ]));
        }
        let free: Vec<usize> = (0..lines.len()).collect();
        let tables = find_column_tables(&lines, &infos(&lines), &free);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].rows.len(), 3);
        assert!(tables[0].bbox[1] > 80.0);
    }

    #[test]
    fn two_line_headers_and_cells_fold_into_their_rows() {
        // A borderless table whose header and one cell take two lines.
        let w = |t: &str, x0: f64, y0: f64, x1: f64, y1: f64| OcrWord {
            text: t.into(),
            bbox: [x0, y0, x1, y1],
        };
        let lines = vec![
            line(vec![
                w("Facility", 57.0, 404.0, 95.0, 413.0),
                w("type", 97.0, 404.0, 113.0, 413.0),
                w("Sites", 133.0, 404.0, 153.0, 413.0),
            ]),
            line(vec![w("Avg.", 181.0, 397.0, 200.0, 409.0)]),
            line(vec![w("delivery", 181.0, 409.0, 220.0, 421.0)]),
            line(vec![
                w("Cost", 253.0, 397.0, 272.0, 409.0),
                w("/", 274.0, 397.0, 278.0, 409.0),
            ]),
            line(vec![w("parcel", 255.0, 409.0, 284.0, 421.0)]),
            line(vec![
                w("Regional", 57.0, 429.0, 95.0, 439.0),
                w("hub", 97.0, 429.0, 113.0, 439.0),
            ]),
            line(vec![w("4", 148.0, 429.0, 153.0, 439.0)]),
            line(vec![
                w("3.4", 200.0, 429.0, 212.0, 439.0),
                w("d", 214.0, 429.0, 220.0, 439.0),
            ]),
            line(vec![w("$2.10", 260.0, 429.0, 284.0, 439.0)]),
            line(vec![w("Micro-", 57.0, 447.0, 99.0, 458.0)]),
            line(vec![w("fulfilment", 57.0, 459.0, 99.0, 470.0)]),
            line(vec![w("31", 144.0, 454.0, 153.0, 464.0)]),
            line(vec![
                w("0.9", 199.0, 454.0, 212.0, 464.0),
                w("d", 214.0, 454.0, 220.0, 464.0),
            ]),
            line(vec![w("$2.15", 261.0, 454.0, 284.0, 464.0)]),
            line(vec![
                w("Dark", 57.0, 479.0, 80.0, 488.0),
                w("store", 82.0, 479.0, 101.0, 488.0),
            ]),
            line(vec![w("12", 144.0, 479.0, 153.0, 488.0)]),
            line(vec![
                w("0.4", 199.0, 479.0, 212.0, 488.0),
                w("d", 214.0, 479.0, 220.0, 488.0),
            ]),
            line(vec![w("$2.95", 260.0, 479.0, 284.0, 488.0)]),
        ];
        let free: Vec<usize> = (0..lines.len()).collect();
        let tables = find_column_tables(&lines, &infos(&lines), &free);
        assert_eq!(tables.len(), 1, "{tables:?}");
        let t = &tables[0];
        assert_eq!(t.rows.len(), 4, "{:?}", t.rows);
        assert!(t.rows.iter().all(|r| r.len() == 4), "{:?}", t.rows);
    }

    #[test]
    fn a_table_inside_a_text_column_is_still_found() {
        let mut lines = Vec::new();
        let prose = |x0: f64, y: f64| {
            line(vec![
                word("lorem", x0, y, 50.0),
                word("ipsum", x0 + 60.0, y, 50.0),
                word("dolor", x0 + 120.0, y, 50.0),
                word("sit", x0 + 180.0, y, 30.0),
                word("amet", x0 + 220.0, y, 40.0),
                word("sed", x0 + 270.0, y, 30.0),
            ])
        };
        for r in 0..12 {
            lines.push(prose(500.0, 100.0 + r as f64 * 30.0));
        }
        for r in 0..3 {
            lines.push(prose(50.0, 100.0 + r as f64 * 30.0));
        }
        for r in 3..7 {
            let y = 100.0 + r as f64 * 30.0;
            lines.push(line(vec![
                word("item", 50.0, y, 60.0),
                word("qty", 200.0, y, 40.0),
                word("price", 300.0, y, 50.0),
            ]));
        }
        for r in 7..12 {
            lines.push(prose(50.0, 100.0 + r as f64 * 30.0));
        }
        let free: Vec<usize> = (0..lines.len()).collect();
        let tables = find_column_tables(&lines, &infos(&lines), &free);
        assert_eq!(tables.len(), 1, "{tables:?}");
        assert_eq!(tables[0].rows.len(), 4);
        assert!(tables[0].bbox[2] < 400.0);
    }

    #[test]
    fn two_text_columns_are_not_a_table() {
        let mut lines = Vec::new();
        for r in 0..6 {
            let y = 100.0 + r as f64 * 30.0;
            for x0 in [50.0, 500.0] {
                lines.push(line(vec![
                    word("lorem", x0, y, 50.0),
                    word("ipsum", x0 + 60.0, y, 50.0),
                    word("dolor", x0 + 120.0, y, 50.0),
                    word("sit", x0 + 180.0, y, 30.0),
                    word("amet", x0 + 220.0, y, 40.0),
                    word("sed", x0 + 270.0, y, 30.0),
                ]));
            }
        }
        let free: Vec<usize> = (0..lines.len()).collect();
        assert!(find_column_tables(&lines, &infos(&lines), &free).is_empty());
    }

    #[test]
    fn two_rows_need_wide_gaps() {
        let narrow = vec![
            line(vec![
                word("a", 50.0, 100.0, 60.0),
                word("b", 140.0, 100.0, 60.0),
            ]),
            line(vec![
                word("c", 50.0, 130.0, 60.0),
                word("d", 140.0, 130.0, 60.0),
            ]),
        ];
        let free = vec![0, 1];
        assert!(find_column_tables(&narrow, &infos(&narrow), &free).is_empty());
        let wide = vec![
            line(vec![
                word("a", 50.0, 100.0, 60.0),
                word("b", 300.0, 100.0, 60.0),
            ]),
            line(vec![
                word("c", 50.0, 130.0, 60.0),
                word("d", 300.0, 130.0, 60.0),
            ]),
        ];
        assert_eq!(find_column_tables(&wide, &infos(&wide), &free).len(), 1);
    }
}
