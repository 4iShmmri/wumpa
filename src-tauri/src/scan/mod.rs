// Page analysis: describes a page as text lines, tables, rules and pictures.
// A scanned page (one big image) is recognised region by region; a text
// page brings its own lines and only needs the raster for rules, tables
// and pictures. "Image to PDF" composes the result back into a PDF page;
// the Word and Excel exports build documents from it.
pub mod blocks;
pub mod columns;
pub mod figures;
pub mod page_image;
pub mod prep;
pub mod rebuild;
pub mod rules;
pub mod tables;
pub mod text;

use image::{GrayImage, RgbImage};
use lopdf::{Document, ObjectId};
use std::collections::HashMap;

use crate::common::{err, R};
use crate::ocr::{self, OcrLine};
pub use blocks::{Block, Cell, PageBlocks};

pub struct ScanOptions {
    pub langs: Vec<String>,
    // The result flows into a document rather than back onto a page:
    // tables aligned in columns count, empty boxes are decoration.
    pub flow: bool,
}

// Where the page's text comes from.
pub enum Text {
    Recognise,
    // A text page: its lines, and the boxes of pictures the file draws.
    Native {
        lines: Vec<text::NativeLine>,
        figures: Vec<[f64; 4]>,
    },
}

pub struct AnalysedTable {
    pub table: tables::Table,
    pub cells: Vec<Vec<text::CellText>>,
    pub ruled: bool,
}

// Everything found on a page, in raster pixels with the origin top-left;
// `sx`/`sy` scale pixels to points.
pub struct Analysis {
    pub width_pt: f64,
    pub height_pt: f64,
    pub sx: f64,
    pub sy: f64,
    pub lines: Vec<text::TextLine>,
    pub tables: Vec<AnalysedTable>,
    pub rules: Vec<rules::Rule>,
    pub figures: Vec<figures::Figure>,
}

// Reports progress as (stage, done, total).
pub struct Progress<'a>(pub &'a mut dyn FnMut(&str, usize, usize));

impl Progress<'_> {
    fn report(&mut self, stage: &str, done: usize) {
        (self.0)(stage, done, STAGES);
    }
}

const STAGES: usize = 6;
// A rule outside any table must be at least this share of the page's
// longer side; shorter strokes are almost always glyph parts.
const FREE_RULE_MIN: f64 = 0.03;
// Share of a box that must lie inside a region to count as being in it.
const INSIDE: f64 = 0.9;
// Ink fill of a table cell interior that calls for a second look: enough
// for a glyph, not a shaded or filled cell.
const CELL_INK: std::ops::RangeInclusive<f64> = 0.003..=0.5;
// Inset from the cell border when judging its interior, in pixels.
const CELL_INSET: f64 = 4.0;

// Pixel-to-point mapping for a page image.
struct Frame {
    sx: f64,
    sy: f64,
    page_h: f64,
}

impl Frame {
    fn bbox(&self, b: &[f64; 4]) -> [f64; 4] {
        [
            b[0] * self.sx,
            self.page_h - b[3] * self.sy,
            b[2] * self.sx,
            self.page_h - b[1] * self.sy,
        ]
    }

    fn point(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.sx, self.page_h - y * self.sy)
    }
}

// Rules that sit inside a text line above its bottom quarter are glyph
// strokes (Arabic baselines, dashes), not underlines.
fn drop_glyph_rules(rules: &mut Vec<rules::Rule>, lines: &[OcrLine]) {
    rules.retain(|r| {
        let b = r.bbox();
        let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        !lines.iter().any(|l| {
            let lb = l.bbox;
            cx >= lb[0]
                && cx <= lb[2]
                && cy >= lb[1]
                && cy <= lb[3]
                && cy < lb[1] + 0.75 * (lb[3] - lb[1])
                && b[2] - b[0] <= lb[2] - lb[0]
        })
    });
}

fn ocr_image(img: &RgbImage, langs: &[String], raw: bool) -> R<Vec<OcrLine>> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("wumpa-scan-{}-{nanos}.png", std::process::id()));
    img.save_with_format(&path, image::ImageFormat::Png)
        .map_err(err)?;
    let langs: Vec<&str> = langs.iter().map(String::as_str).collect();
    let result = ocr::recognize(&path, &langs, raw);
    let _ = std::fs::remove_file(&path);
    result
}

// Recognises a crop of the page; boxes come back in page pixels.
fn ocr_region(img: &RgbImage, b: &[f64; 4], langs: &[String]) -> R<Vec<OcrLine>> {
    let x0 = b[0].max(0.0) as u32;
    let y0 = b[1].max(0.0) as u32;
    let w = (b[2].ceil() as u32).min(img.width()).saturating_sub(x0);
    let h = (b[3].ceil() as u32).min(img.height()).saturating_sub(y0);
    if w < 2 || h < 2 {
        return Ok(Vec::new());
    }
    let crop = image::imageops::crop_imm(img, x0, y0, w, h).to_image();
    let shift = |b: &mut [f64; 4]| {
        b[0] += x0 as f64;
        b[2] += x0 as f64;
        b[1] += y0 as f64;
        b[3] += y0 as f64;
    };
    let mut lines = ocr_image(&crop, langs, true)?;
    for l in &mut lines {
        shift(&mut l.bbox);
        for w in &mut l.words {
            shift(&mut w.bbox);
        }
    }
    Ok(lines)
}

fn inset(b: &[f64; 4], d: f64) -> [f64; 4] {
    [
        b[0] + d,
        b[1] + d,
        (b[2] - d).max(b[0] + d),
        (b[3] - d).max(b[1] + d),
    ]
}

// Lone digits and short codes in small cells are often skipped on the
// full page. An inked cell without text gets further looks: the whole
// row, then the whole column, then the cell with generous padding, where
// the neighbours give the recogniser context.
fn fill_empty_cells(
    lines: &mut Vec<OcrLine>,
    tables: &[tables::Table],
    ink: &GrayImage,
    color: &RgbImage,
    langs: &[String],
) -> R<()> {
    let mut seen: HashMap<[i64; 4], Vec<OcrLine>> = HashMap::new();
    for t in tables {
        let all: Vec<[f64; 4]> = t.rows.iter().flatten().map(|c| c.bbox).collect();
        for row in &t.rows {
            let row_span = row
                .iter()
                .map(|c| c.bbox)
                .reduce(|a, b| text::union(&a, &b));
            for c in row {
                let b = inset(&c.bbox, CELL_INSET);
                if lines
                    .iter()
                    .any(|l| text::contains(&b, text::center(&l.bbox)))
                    || !CELL_INK.contains(&text::ink_density(ink, &b))
                {
                    continue;
                }
                let col_span = all
                    .iter()
                    .filter(|o| text::h_overlap(o, &c.bbox) >= 0.8)
                    .copied()
                    .reduce(|a, b| text::union(&a, &b));
                let padded = inset(&c.bbox, -0.5 * (c.bbox[3] - c.bbox[1]));
                for region in row_span.into_iter().chain(col_span).chain([padded]) {
                    let region = inset(&region, -CELL_INSET);
                    let key = region.map(|v| v as i64);
                    let found = match seen.get(&key) {
                        Some(f) => f.clone(),
                        None => {
                            let f = ocr_region(color, &region, langs)?;
                            seen.insert(key, f.clone());
                            f
                        }
                    };
                    let hits: Vec<OcrLine> = found
                        .into_iter()
                        .filter(|l| text::contains(&b, text::center(&l.bbox)))
                        .collect();
                    if !hits.is_empty() {
                        lines.extend(hits);
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

// Thin boxes along every cell border, so the consumed grid lines are
// not mistaken for figures.
fn table_edges(t: &tables::Table) -> Vec<[f64; 4]> {
    const PAD: f64 = 3.0;
    t.rows
        .iter()
        .flatten()
        .flat_map(|c| {
            let b = c.bbox;
            [
                [b[0] - PAD, b[1] - PAD, b[2] + PAD, b[1] + PAD],
                [b[0] - PAD, b[3] - PAD, b[2] + PAD, b[3] + PAD],
                [b[0] - PAD, b[1] - PAD, b[0] + PAD, b[3] + PAD],
                [b[2] - PAD, b[1] - PAD, b[2] + PAD, b[3] + PAD],
            ]
        })
        .collect()
}

// The box is the ink's extent, so the composer puts the glyphs exactly
// where the scan had them.
fn text_block(l: &text::TextLine, f: &Frame) -> Block {
    Block::Text {
        bbox: f.bbox(&l.bbox),
        text: l.text.clone(),
        size: Some(l.size_px * f.sy),
        tight: true,
        bold: l.bold,
        italic: l.italic,
        align: l.align,
        dir: l.dir,
        color: l.color,
    }
}

// Cells of one table share a typeface size: individual estimates are pulled
// towards the table median, kept inside the cell, and bold is judged against
// the table's own cells rather than the whole page.
fn table_block(t: &tables::Table, cells: &[Vec<text::CellText>], f: &Frame) -> Block {
    let filled: Vec<&text::CellText> = cells.iter().flatten().filter(|c| c.size_px > 0.0).collect();
    let mut sizes: Vec<f64> = filled.iter().map(|c| c.size_px).collect();
    let mut dens: Vec<f64> = filled.iter().map(|c| c.density).collect();
    let size_med = text::median(&mut sizes);
    let dens_med = text::median(&mut dens);
    let rows = t
        .rows
        .iter()
        .zip(cells)
        .map(|(row, texts)| {
            row.iter()
                .zip(texts)
                .map(|(cb, ct)| {
                    let bbox = f.bbox(&cb.bbox);
                    let cell_h = (bbox[3] - bbox[1]).abs();
                    let n_lines = ct.text.split('\n').count() as f64;
                    let size = (ct.size_px > 0.0).then(|| {
                        let px = if size_med > 0.0 {
                            ct.size_px.clamp(0.95 * size_med, 1.15 * size_med)
                        } else {
                            ct.size_px
                        };
                        (px * f.sy).min(0.72 * cell_h / n_lines)
                    });
                    Cell {
                        bbox,
                        text: ct.text.clone(),
                        size,
                        align: ct.align,
                        dir: ct.dir,
                        bold: dens_med > 0.0 && ct.density >= 1.7 * dens_med,
                    }
                })
                .collect()
        })
        .collect();
    Block::Table {
        bbox: f.bbox(&t.bbox),
        rows,
    }
}

fn rule_block(r: &rules::Rule, f: &Frame) -> Block {
    let (x1, y1) = f.point(r.x1, r.y1);
    let (x2, y2) = f.point(r.x2, r.y2);
    Block::Rule {
        x1,
        y1,
        x2,
        y2,
        width: r.width * f.sx.min(f.sy),
        color: [0.0; 3],
    }
}

fn top_of(b: &Block) -> f64 {
    match b {
        Block::Text { bbox, .. }
        | Block::Rect { bbox, .. }
        | Block::Image { bbox, .. }
        | Block::Table { bbox, .. } => bbox[3],
        Block::Rule { y1, y2, .. } => y1.max(*y2),
    }
}

// Analyses a page raster of `page_w` x `page_h` points.
pub fn analyse(
    image: RgbImage,
    page_w: f64,
    page_h: f64,
    text: Text,
    opts: &ScanOptions,
    progress: &mut Progress,
) -> R<Analysis> {
    if image.width() == 0 || image.height() == 0 || page_w <= 0.0 || page_h <= 0.0 {
        return Err("empty page image".into());
    }
    progress.report("deskew", 1);
    // A text page's raster is straight already; its lines must stay put.
    let prep = match &text {
        Text::Recognise => prep::prepare(image),
        Text::Native { .. } => prep::Prepped {
            ink: prep::binarise(&prep::grayscale(&image)),
            color: image,
        },
    };
    let (sx, sy) = (
        page_w / prep.color.width() as f64,
        page_h / prep.color.height() as f64,
    );
    progress.report("ocr", 2);
    let (mut native, known_figures) = match text {
        Text::Recognise => (None, Vec::new()),
        Text::Native { lines, figures } => (Some(lines), figures),
    };
    let mut lines = match &native {
        Some(n) => n.iter().map(|l| l.line.clone()).collect(),
        None => ocr_image(&prep.color, &opts.langs, false)?,
    };
    lines.retain(|l| l.text.chars().any(char::is_alphanumeric));
    progress.report("rules", 3);
    let mut free_rules = rules::find_rules(&prep.ink);
    drop_glyph_rules(&mut free_rules, &lines);
    let line_boxes: Vec<[f64; 4]> = lines.iter().map(|l| l.bbox).collect();
    let photos = figures::dense_regions(&prep.ink, &line_boxes);
    free_rules.retain(|r| {
        !photos
            .iter()
            .any(|p| figures::mostly_inside(&r.bbox(), p, INSIDE))
    });
    let mut tables = tables::find_tables(&mut free_rules);
    // A grid with nothing in its cells on a coloured ground is part of a
    // picture; an empty single box is decoration when the text will flow.
    tables.retain(|t| {
        let has_text = lines
            .iter()
            .any(|l| text::contains(&t.bbox, text::center(&l.bbox)));
        has_text
            || (figures::paper_like(&prep.color, &t.bbox)
                && !(opts.flow && t.rows.len() == 1 && t.rows[0].len() == 1))
    });
    let min_free = FREE_RULE_MIN * (prep.ink.width().max(prep.ink.height()) as f64);
    free_rules.retain(|r| {
        r.length() >= min_free
            && !tables
                .iter()
                .any(|t| figures::mostly_inside(&r.bbox(), &t.bbox, INSIDE))
    });
    if native.is_none() {
        fill_empty_cells(&mut lines, &tables, &prep.ink, &prep.color, &opts.langs)?;
    }
    progress.report("figures", 4);
    let mut exclude: Vec<[f64; 4]> = lines.iter().map(|l| l.bbox).collect();
    exclude.extend(tables.iter().flat_map(table_edges));
    let mut figures = figures::find_figures(&prep.ink, &prep.color, &exclude, &free_rules);
    // Pictures the file itself draws join those seen in the raster, unless
    // they turn out to be a table's own frame.
    for b in known_figures {
        if tables
            .iter()
            .any(|t| figures::mostly_inside(&b, &t.bbox, 0.5))
        {
            continue;
        }
        figures.retain(|f| !figures::mostly_inside(&f.bbox, &b, 0.5));
        figures.extend(figures::crop(&prep.color, &b));
    }
    // Text and lines inside a picture (a stamp, a logo) stay part of the picture.
    lines.retain(|l| {
        !figures
            .iter()
            .any(|fig| figures::mostly_inside(&l.bbox, &fig.bbox, INSIDE))
    });
    free_rules.retain(|r| {
        !figures
            .iter()
            .any(|fig| figures::mostly_inside(&r.bbox(), &fig.bbox, INSIDE))
    });
    progress.report("layout", 5);
    let layout = match native.take() {
        Some(mut n) => {
            n.retain(|l| lines.contains(&l.line));
            text::layout_native(&n, &tables, &prep.ink, &prep.color, opts.flow)
        }
        None => text::layout(&lines, &tables, &prep.ink, &prep.color),
    };
    let mut found: Vec<AnalysedTable> = tables
        .into_iter()
        .zip(layout.cells)
        .map(|(table, cells)| AnalysedTable {
            table,
            cells,
            ruled: true,
        })
        .collect();
    found.extend(
        layout
            .column_tables
            .into_iter()
            .map(|(table, cells)| AnalysedTable {
                table,
                cells,
                ruled: false,
            }),
    );
    progress.report("done", 6);
    Ok(Analysis {
        width_pt: page_w,
        height_pt: page_h,
        sx,
        sy,
        lines: layout.lines,
        tables: found,
        rules: free_rules,
        figures,
    })
}

// The page as blocks for the composer, in PDF points.
pub fn to_blocks(a: Analysis) -> PageBlocks {
    let frame = Frame {
        sx: a.sx,
        sy: a.sy,
        page_h: a.height_pt,
    };
    let mut blocks: Vec<Block> = Vec::new();
    blocks.extend(a.lines.iter().map(|l| text_block(l, &frame)));
    blocks.extend(
        a.tables
            .iter()
            .map(|t| table_block(&t.table, &t.cells, &frame)),
    );
    blocks.extend(a.rules.iter().map(|r| rule_block(r, &frame)));
    blocks.extend(a.figures.into_iter().map(|fig| Block::Image {
        bbox: frame.bbox(&fig.bbox),
        png_b64: fig.png_b64,
    }));
    blocks.sort_by(|a, b| top_of(b).partial_cmp(&top_of(a)).unwrap());
    PageBlocks {
        width: a.width_pt,
        height: a.height_pt,
        blocks,
    }
}

// Analyses an already decoded scanned page image of `page_w` x `page_h` points.
pub fn analyse_image(
    image: RgbImage,
    page_w: f64,
    page_h: f64,
    opts: &ScanOptions,
    progress: &mut Progress,
) -> R<PageBlocks> {
    analyse(image, page_w, page_h, Text::Recognise, opts, progress).map(to_blocks)
}

pub fn analyse_page(
    doc: &Document,
    page_id: ObjectId,
    opts: &ScanOptions,
    progress: &mut Progress,
) -> R<PageBlocks> {
    progress.report("decode", 0);
    let Some(src) = page_image::page_image(doc, page_id)? else {
        return Err("this page is not a single scanned image".into());
    };
    analyse_image(src.image, src.width_pt, src.height_pt, opts, progress)
}

#[cfg(test)]
mod tests {
    use super::blocks::Dir;
    use super::page_image::image_only_pages;
    use super::page_image::testing::{image_doc, jpeg_stream};
    use super::*;

    const SAMPLE: &[u8] = include_bytes!("testdata/sample.jpg");

    #[test]
    fn frame_maps_pixels_to_points() {
        let f = Frame {
            sx: 0.5,
            sy: 0.5,
            page_h: 100.0,
        };
        assert_eq!(f.bbox(&[10.0, 20.0, 30.0, 40.0]), [5.0, 80.0, 15.0, 90.0]);
        assert_eq!(f.point(0.0, 200.0), (0.0, 0.0));
    }

    #[test]
    fn drops_rules_inside_glyphs_but_keeps_underlines() {
        let line = OcrLine {
            text: "x".into(),
            conf: 1.0,
            bbox: [100.0, 100.0, 500.0, 200.0],
            words: vec![],
        };
        let mut rules = vec![
            rules::Rule {
                x1: 150.0,
                y1: 160.0,
                x2: 300.0,
                y2: 160.0,
                width: 4.0,
            },
            rules::Rule {
                x1: 100.0,
                y1: 196.0,
                x2: 500.0,
                y2: 196.0,
                width: 2.0,
            },
            rules::Rule {
                x1: 0.0,
                y1: 400.0,
                x2: 800.0,
                y2: 400.0,
                width: 2.0,
            },
        ];
        drop_glyph_rules(&mut rules, &[line]);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].y1, 196.0);
    }

    // Needs the on-device OCR sidecar; skipped when it is not around.
    #[test]
    #[cfg_attr(not(target_os = "macos"), ignore)]
    fn analyses_sample_scan_end_to_end() {
        if ocr::sidecar_path().is_err() {
            eprintln!("skipping: OCR sidecar not available");
            return;
        }
        let doc = image_doc(jpeg_stream(SAMPLE), 612.0, 306.0, false);
        assert_eq!(image_only_pages(&doc), vec![1]);
        let page_id = *doc.get_pages().values().next().unwrap();
        let mut stages = Vec::new();
        let mut cb = |s: &str, _: usize, _: usize| stages.push(s.to_string());
        let opts = ScanOptions {
            langs: vec!["ar".into(), "en".into()],
            flow: false,
        };
        let page = analyse_page(&doc, page_id, &opts, &mut Progress(&mut cb)).unwrap();
        assert_eq!(stages.first().map(String::as_str), Some("decode"));
        assert_eq!(stages.last().map(String::as_str), Some("done"));
        assert_eq!((page.width, page.height), (612.0, 306.0));
        for b in &page.blocks {
            match b {
                Block::Text {
                    text,
                    size,
                    bold,
                    align,
                    dir,
                    bbox,
                    ..
                } => eprintln!(
                    "text {align:?} {dir:?} size={:.1} bold={bold} bbox={bbox:.1?} {text}",
                    size.unwrap_or(0.0)
                ),
                Block::Table { bbox, rows } => {
                    eprintln!("table {bbox:.1?} {}x{}", rows.len(), rows[0].len());
                    for r in rows {
                        eprintln!(
                            "  {:?}",
                            r.iter()
                                .map(|c| format!("{}|{:?}|{:?}", c.text, c.align, c.dir))
                                .collect::<Vec<_>>()
                        );
                    }
                }
                Block::Rule {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    ..
                } => eprintln!("rule ({x1:.1},{y1:.1})-({x2:.1},{y2:.1}) w={width:.2}"),
                Block::Image { bbox, .. } => eprintln!("image {bbox:.1?}"),
                Block::Rect { bbox, .. } => eprintln!("rect {bbox:.1?}"),
            }
        }

        let texts: Vec<_> = page
            .blocks
            .iter()
            .filter(|b| matches!(b, Block::Text { .. }))
            .collect();
        assert!(texts.len() >= 2, "{texts:?}");
        let tables: Vec<_> = page
            .blocks
            .iter()
            .filter_map(|b| match b {
                Block::Table { rows, .. } => Some(rows),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].len(), 3);
        assert!(tables[0].iter().all(|r| r.len() == 4), "{:?}", tables[0]);
        assert!(tables[0][0][0].text.contains("البند"), "{:?}", tables[0][0]);
        for b in &texts {
            if let Block::Text {
                text,
                dir,
                bbox,
                size,
                ..
            } = b
            {
                let arabic = text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
                assert_eq!(*dir, if arabic { Dir::Rtl } else { Dir::Ltr }, "{text}");
                assert!(bbox[0] >= 0.0 && bbox[2] <= 612.0 && bbox[1] >= 0.0 && bbox[3] <= 306.0);
                assert!(
                    size.unwrap() > 4.0 && size.unwrap() < 40.0,
                    "size {size:?} for {text}"
                );
            }
        }
        assert!(
            !page.blocks.iter().any(|b| matches!(b, Block::Image { .. })),
            "stray figures"
        );
    }
}

#[cfg(test)]
mod dump {
    // Writes the analysed blocks of WUMPA_SCAN_SRC page 1 to WUMPA_SCAN_DUMP for inspection.
    #[test]
    #[ignore]
    fn dump_blocks() {
        let src = std::env::var("WUMPA_SCAN_SRC").unwrap();
        let out = std::env::var("WUMPA_SCAN_DUMP").unwrap();
        let doc = crate::common::load_plain(&src).unwrap();
        let pid = doc.get_pages()[&1];
        let opts = super::ScanOptions {
            langs: vec!["ar".into(), "en".into()],
            flow: false,
        };
        let mut cb = |_: &str, _: usize, _: usize| {};
        let mut progress = super::Progress(&mut cb);
        let page = super::analyse_page(&doc, pid, &opts, &mut progress).unwrap();
        std::fs::write(&out, serde_json::to_string_pretty(&page).unwrap()).unwrap();
    }
}
