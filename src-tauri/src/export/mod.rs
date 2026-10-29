// Export to Word and Excel. Every page is analysed into the document
// model: a text page brings its lines and a raster, a scanned page is
// recognised; the writers then build the files.
pub mod docx;
pub(crate) mod glyphs;
pub mod model;
mod native;
#[cfg(test)]
mod regression;
pub mod xlsx;
mod xml;

use base64::Engine;
use serde::Deserialize;

use crate::common::{load_plain, R};
use crate::ocr::{OcrLine, OcrWord};
use crate::scan::text::NativeLine;
use crate::scan::{self, page_image, Progress, ScanOptions, Text};

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WordIn {
    pub text: String,
    pub bbox: [f64; 4],
}

// A line of a text page in PDF points, origin bottom-left.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LineIn {
    pub text: String,
    pub bbox: [f64; 4],
    pub size: f64,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub words: Vec<WordIn>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum PageInput {
    // A page with real text: its lines plus a raster of the whole page.
    Native {
        page: u32,
        width: f64,
        height: f64,
        raster_png_b64: String,
        lines: Vec<LineIn>,
    },
    // A page that is one big picture; recognised from the file itself.
    Scanned {
        page: u32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Docx,
    Xlsx,
}

impl Format {
    pub fn parse(s: &str) -> R<Format> {
        match s {
            "docx" => Ok(Format::Docx),
            "xlsx" => Ok(Format::Xlsx),
            _ => Err(format!("unknown export format {s}")),
        }
    }
}

fn native_lines(lines: Vec<LineIn>, height: f64, s: f64) -> Vec<NativeLine> {
    let flip = |b: [f64; 4]| [b[0] * s, (height - b[3]) * s, b[2] * s, (height - b[1]) * s];
    lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty() && l.bbox[2] > l.bbox[0])
        .map(|l| NativeLine {
            line: OcrLine {
                text: l.text,
                conf: 1.0,
                bbox: flip(l.bbox),
                words: l
                    .words
                    .into_iter()
                    .map(|w| OcrWord {
                        text: w.text,
                        bbox: flip(w.bbox),
                    })
                    .collect(),
            },
            size: l.size * s,
            bold: l.bold,
            italic: l.italic,
        })
        .collect()
}

// Analyses one page of `src` into the document model.
pub fn analyse_input(
    src: &str,
    input: PageInput,
    langs: Vec<String>,
    progress: &mut Progress,
) -> R<model::Page> {
    let opts = ScanOptions { langs, flow: true };
    match input {
        PageInput::Native {
            page,
            width,
            height,
            raster_png_b64,
            lines,
        } => {
            if width <= 0.0 || height <= 0.0 {
                return Err("bad page size".into());
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raster_png_b64)
                .map_err(|e| format!("page raster: {e}"))?;
            let raster = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .map_err(|e| format!("page raster: {e}"))?
                .to_rgb8();
            let s = raster.width() as f64 / width;
            // The file's own glyphs are the better source; the viewer's
            // lines stand in when the reader cannot follow the file.
            let lines = match native::extract_lines(src, page) {
                Ok(mine) => native::merge_with_viewer(mine, &lines),
                Err(_) => lines,
            };
            let figures = native::figure_boxes(src, page, &lines)
                .unwrap_or_default()
                .into_iter()
                .map(|b| [b[0] * s, (height - b[3]) * s, b[2] * s, (height - b[1]) * s])
                .collect();
            let text = Text::Native {
                lines: native_lines(lines, height, s),
                figures,
            };
            let a = scan::analyse(raster, width, height, text, &opts, progress)?;
            Ok(model::page_from(&a, page))
        }
        PageInput::Scanned { page } => {
            let doc = load_plain(src)?;
            let pid = *doc
                .get_pages()
                .get(&page)
                .ok_or_else(|| format!("page {page} not found"))?;
            let Some(img) = page_image::page_image(&doc, pid)? else {
                return Err("this page is not a single scanned image".into());
            };
            let a = scan::analyse(
                img.image,
                img.width_pt,
                img.height_pt,
                Text::Recognise,
                &opts,
                progress,
            )?;
            Ok(model::page_from(&a, page))
        }
    }
}

pub fn write(pages: &[model::Page], format: Format, out: &str, lang: &str) -> R<()> {
    let bytes = match format {
        Format::Docx => docx::write_docx(pages)?,
        Format::Xlsx => xlsx::write_xlsx(pages, lang)?,
    };
    std::fs::write(out, bytes).map_err(|e| format!("cannot write {out}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::blocks::Dir;
    use image::{Rgb, RgbImage};

    fn raster(w: u32, h: u32, draw: impl Fn(&mut RgbImage)) -> String {
        let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
        draw(&mut img);
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    }

    fn line(text: &str, x: f64, y: f64, w: f64, size: f64, bold: bool) -> LineIn {
        let words = text
            .split(' ')
            .scan(x, |pos, word| {
                let ww = w * word.len() as f64 / text.len() as f64;
                let b = [*pos, y - 0.25 * size, *pos + ww, y + 0.75 * size];
                *pos += ww + 0.0;
                Some(WordIn {
                    text: word.into(),
                    bbox: b,
                })
            })
            .collect();
        LineIn {
            text: text.into(),
            bbox: [x, y - 0.25 * size, x + w, y + 0.75 * size],
            size,
            bold,
            italic: false,
            words,
        }
    }

    #[test]
    fn a_text_page_becomes_paragraphs_and_a_ruled_table() {
        // 300 x 400 pt page rastered at 2 px/pt with a 3 x 2 grid of rules.
        let (w, h) = (300.0, 400.0);
        let png = raster(600, 800, |img| {
            let black = Rgb([0, 0, 0]);
            for y in [400u32, 440, 480, 520] {
                for x in 100..500 {
                    for t in 0..2 {
                        img.put_pixel(x, y + t, black);
                    }
                }
            }
            for x in [100u32, 300, 498] {
                for y in 400..522 {
                    for t in 0..2 {
                        img.put_pixel(x + t, y, black);
                    }
                }
            }
        });
        let mut lines = vec![
            line("Heading of the page", 50.0, 370.0, 150.0, 16.0, true),
            line(
                "A first body line that is long",
                50.0,
                340.0,
                200.0,
                10.0,
                false,
            ),
            line("and a second body line", 50.0, 328.0, 160.0, 10.0, false),
        ];
        // Cell text: rows at raster y 400-440, 440-480, 480-520 → pt tops 200,220,240.
        for (r, (a, b)) in [("Item", "Qty"), ("Chair", "4"), ("Desk", "12")]
            .iter()
            .enumerate()
        {
            let y = h - (210.0 + r as f64 * 20.0) - 3.0;
            lines.push(line(a, 60.0, y, 40.0, 9.0, r == 0));
            lines.push(line(b, 170.0, y, 20.0, 9.0, r == 0));
        }
        let input = PageInput::Native {
            page: 1,
            width: w,
            height: h,
            raster_png_b64: png,
            lines,
        };
        let mut cb = |_: &str, _: usize, _: usize| {};
        let page = analyse_input("", input, vec![], &mut Progress(&mut cb)).unwrap();
        let paras: Vec<&model::Para> = page
            .blocks
            .iter()
            .filter_map(|b| match &b.item {
                model::Item::Para(p) => Some(p),
                _ => None,
            })
            .collect();
        assert_eq!(paras.len(), 2, "{:?}", page.blocks);
        assert!(paras[0].bold && paras[0].text.starts_with("Heading"));
        assert_eq!(
            paras[1].text,
            "A first body line that is long and a second body line"
        );
        assert_eq!(paras[1].dir, Dir::Ltr);
        let tables: Vec<&model::TableItem> = page
            .blocks
            .iter()
            .filter_map(|b| match &b.item {
                model::Item::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1, "{:?}", page.blocks);
        let t = tables[0];
        assert!(t.ruled);
        assert_eq!(t.rows.len(), 3);
        assert_eq!(t.rows[1][0].text, "Chair");
        assert_eq!(t.rows[2][1].text, "12");
        assert!(t.rows[0][0].bold);
        // Reading order: heading, body, table.
        assert!(matches!(page.blocks[0].item, model::Item::Para(_)));
        assert!(matches!(page.blocks[2].item, model::Item::Table(_)));
        assert!(Format::parse("pdf").is_err());
    }
}

#[cfg(test)]
mod dump {
    // Exports the pages in WUMPA_EXPORT_IN (a JSON array of PageInput, with
    // WUMPA_EXPORT_SRC as the PDF for scanned pages) to WUMPA_EXPORT_OUT
    // with both extensions, for inspection.
    #[test]
    #[ignore]
    fn export_from_dump() {
        let input = std::env::var("WUMPA_EXPORT_IN").unwrap();
        let src = std::env::var("WUMPA_EXPORT_SRC").unwrap_or_default();
        let out = std::env::var("WUMPA_EXPORT_OUT").unwrap();
        let lang = std::env::var("WUMPA_EXPORT_LANG").unwrap_or("ar".into());
        let json = std::fs::read(&input).unwrap();
        let inputs: Vec<super::PageInput> = serde_json::from_slice(&json).unwrap();
        let mut cb = |_: &str, _: usize, _: usize| {};
        let mut progress = crate::scan::Progress(&mut cb);
        let pages: Vec<super::model::Page> = inputs
            .into_iter()
            .map(|i| {
                super::analyse_input(&src, i, vec!["ar".into(), "en".into()], &mut progress)
                    .unwrap()
            })
            .collect();
        std::fs::write(
            format!("{out}-model.json"),
            serde_json::to_string_pretty(&pages).unwrap(),
        )
        .unwrap();
        super::write(&pages, super::Format::Docx, &format!("{out}.docx"), &lang).unwrap();
        super::write(&pages, super::Format::Xlsx, &format!("{out}.xlsx"), &lang).unwrap();
    }
}
