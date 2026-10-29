// Writes the document model as a Word file (WordprocessingML), keeping
// page sizes, margins, alignment, direction, sizes, weights, tables and
// pictures.

use super::model::{Block, CellItem, Item, Page, Para, TableItem};
use super::xml::{emu, esc, hex_color, package, picture_file, twips};
use crate::common::R;
use crate::scan::blocks::{Align, Dir};

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_DOC: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";

// Smallest page margin kept, in points.
const MIN_MARGIN: f64 = 18.0;

struct Media {
    rels: Vec<(String, String)>,
    files: Vec<(String, Vec<u8>)>,
}

// Word reads `left`/`right` relative to the paragraph direction in a
// bidi paragraph, so physical alignment flips for right-to-left text.
fn jc(align: Align, dir: Dir) -> &'static str {
    match (align, dir == Dir::Rtl) {
        (Align::Center, _) => "center",
        (Align::Justify, _) => "both",
        (Align::Start, false) | (Align::End, true) => "left",
        (Align::Start, true) | (Align::End, false) => "right",
    }
}

fn run_props(size: f64, bold: bool, italic: bool, color: Option<[f64; 3]>, rtl: bool) -> String {
    let hp = ((size * 2.0).round() as i64).max(2);
    let mut s = String::from("<w:rPr>");
    if bold {
        s.push_str("<w:b/><w:bCs/>");
    }
    if italic {
        s.push_str("<w:i/><w:iCs/>");
    }
    if let Some(c) = color {
        if c != [0.0; 3] {
            s.push_str(&format!("<w:color w:val=\"{}\"/>", hex_color(c)));
        }
    }
    s.push_str(&format!("<w:sz w:val=\"{hp}\"/><w:szCs w:val=\"{hp}\"/>"));
    if rtl {
        s.push_str("<w:rtl/>");
    }
    s.push_str("</w:rPr>");
    s
}

// Runs for a text; line breaks inside become `<w:br/>`.
fn runs(text: &str, props: &str) -> String {
    let mut s = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            s.push_str(&format!("<w:r>{props}<w:br/></w:r>"));
        }
        if !line.is_empty() {
            s.push_str(&format!(
                "<w:r>{props}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
                esc(line)
            ));
        }
    }
    s
}

fn para_xml(p: &Para, before_pt: f64) -> String {
    let rtl = p.dir == Dir::Rtl;
    let mut ppr = format!(
        "<w:pPr><w:spacing w:before=\"{}\" w:after=\"0\"/><w:jc w:val=\"{}\"/>",
        twips(before_pt.max(0.0)),
        jc(p.align, p.dir)
    );
    if rtl {
        ppr.push_str("<w:bidi/>");
    }
    ppr.push_str("</w:pPr>");
    let props = run_props(p.size, p.bold, p.italic, Some(p.color), rtl);
    format!("<w:p>{ppr}{}</w:p>", runs(&p.text, &props))
}

fn cell_xml(c: &CellItem, width_pt: f64) -> String {
    let rtl = c.dir == Dir::Rtl;
    let mut tcpr = format!(
        "<w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>",
        twips(width_pt)
    );
    if c.span > 1 {
        tcpr.push_str(&format!("<w:gridSpan w:val=\"{}\"/>", c.span));
    }
    tcpr.push_str("<w:vAlign w:val=\"center\"/></w:tcPr>");
    let mut ppr = format!(
        "<w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/><w:jc w:val=\"{}\"/>",
        jc(c.align, c.dir)
    );
    if rtl {
        ppr.push_str("<w:bidi/>");
    }
    ppr.push_str("</w:pPr>");
    let props = run_props(c.size.max(6.0), c.bold, c.italic, None, rtl);
    format!(
        "<w:tc>{tcpr}<w:p>{ppr}{}</w:p></w:tc>",
        runs(&c.text, &props)
    )
}

fn empty_cell(width_pt: f64) -> String {
    format!(
        "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc>",
        twips(width_pt)
    )
}

fn table_xml(t: &TableItem, before_pt: f64) -> String {
    let total: f64 = t.cols.iter().sum();
    let mut s = String::new();
    // Space above the table, as Word tables carry no spacing of their own.
    if before_pt > 2.0 {
        s.push_str(&format!(
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{}\" w:lineRule=\"exact\"/></w:pPr></w:p>",
            twips(before_pt)
        ));
    }
    s.push_str(&format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/>",
        twips(total)
    ));
    if t.rtl {
        s.push_str("<w:bidiVisual/>");
    }
    s.push_str("<w:tblLayout w:type=\"fixed\"/>");
    if t.ruled {
        s.push_str("<w:tblBorders>");
        for side in ["top", "left", "bottom", "right", "insideH", "insideV"] {
            s.push_str(&format!(
                "<w:{side} w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"000000\"/>"
            ));
        }
        s.push_str("</w:tblBorders>");
    }
    s.push_str("<w:tblCellMar><w:left w:w=\"60\" w:type=\"dxa\"/><w:right w:w=\"60\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>");
    for c in &t.cols {
        s.push_str(&format!("<w:gridCol w:w=\"{}\"/>", twips(*c)));
    }
    s.push_str("</w:tblGrid>");
    for row in &t.rows {
        s.push_str("<w:tr>");
        let mut cells: Vec<String> = Vec::new();
        let mut col = 0;
        for c in row {
            let span = c.span.max(1).min(t.cols.len().saturating_sub(col).max(1));
            let w: f64 = t.cols[col.min(t.cols.len() - 1)..(col + span).min(t.cols.len())]
                .iter()
                .sum();
            cells.push(cell_xml(c, w));
            col += span;
        }
        while col < t.cols.len() {
            cells.push(empty_cell(t.cols[col]));
            col += 1;
        }
        // A visually right-to-left table lists its cells from the right.
        if t.rtl {
            cells.reverse();
        }
        s.push_str(&cells.concat());
        s.push_str("</w:tr>");
    }
    s.push_str("</w:tbl><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/></w:pPr></w:p>");
    s
}

fn picture_xml(
    png_b64: &str,
    width: f64,
    height: f64,
    align: &str,
    before_pt: f64,
    media: &mut Media,
) -> Option<String> {
    let (ext, bytes) = picture_file(png_b64)?;
    let n = media.files.len() + 1;
    let rid = format!("rIdImg{n}");
    let name = format!("media/image{n}.{ext}");
    media.files.push((format!("word/{name}"), bytes));
    media.rels.push((rid.clone(), name));
    let (cx, cy) = (emu(width.max(1.0)), emu(height.max(1.0)));
    Some(format!(
        concat!(
            "<w:p><w:pPr><w:spacing w:before=\"{before}\" w:after=\"0\"/><w:jc w:val=\"{align}\"/></w:pPr>",
            "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">",
            "<wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\"/>",
            "<a:graphic><a:graphicData uri=\"{pic}\"><pic:pic>",
            "<pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"image{n}\"/><pic:cNvPicPr/></pic:nvPicPr>",
            "<pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>",
            "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>",
            "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>",
            "</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ),
        before = twips(before_pt.max(0.0)),
        align = align,
        cx = cx,
        cy = cy,
        n = n,
        pic = PIC_NS,
        rid = rid,
    ))
}

// Margins from the content's extent, so the flow starts where the page did.
fn margins(page: &Page) -> [f64; 4] {
    let (mut l, mut t, mut r, mut b) = (page.width, page.height, 0.0f64, 0.0f64);
    for blk in &page.blocks {
        l = l.min(blk.bbox[0]);
        t = t.min(blk.bbox[1]);
        r = r.max(blk.bbox[2]);
        b = b.max(blk.bbox[3]);
    }
    if r <= l || b <= t {
        return [72.0; 4];
    }
    let clamp = |v: f64, max: f64| v.clamp(MIN_MARGIN, max);
    [
        clamp(l, page.width / 4.0),
        clamp(t, page.height / 4.0),
        clamp(page.width - r, page.width / 4.0),
        clamp(page.height - b, page.height / 4.0),
    ]
}

fn sect_pr(page: &Page) -> String {
    let m = margins(page);
    let mut s = format!(
        "<w:sectPr><w:pgSz w:w=\"{}\" w:h=\"{}\"{}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/>",
        twips(page.width),
        twips(page.height),
        if page.width > page.height { " w:orient=\"landscape\"" } else { "" },
        twips(m[1]),
        twips(m[2]),
        twips(m[3]),
        twips(m[0])
    );
    if page.rtl() {
        s.push_str("<w:bidi/>");
    }
    s.push_str("</w:sectPr>");
    s
}

fn picture_align(b: &Block, page: &Page) -> &'static str {
    let cx = (b.bbox[0] + b.bbox[2]) / 2.0;
    let w = page.width;
    if cx < 0.4 * w {
        "left"
    } else if cx > 0.6 * w {
        "right"
    } else {
        "center"
    }
}

fn body(pages: &[Page], media: &mut Media) -> String {
    let mut s = String::new();
    for (pi, page) in pages.iter().enumerate() {
        let m = margins(page);
        let text_w = (page.width - m[0] - m[2]).max(36.0);
        let mut last_bottom = m[1];
        for b in &page.blocks {
            let before = (b.bbox[1] - last_bottom).max(0.0);
            match &b.item {
                Item::Para(p) => s.push_str(&para_xml(p, before)),
                Item::Table(t) => s.push_str(&table_xml(t, before)),
                Item::Picture(p) => {
                    let scale = (text_w / p.width.max(1.0)).min(1.0);
                    if let Some(xml) = picture_xml(
                        &p.png_b64,
                        p.width * scale,
                        p.height * scale,
                        picture_align(b, page),
                        before,
                        media,
                    ) {
                        s.push_str(&xml);
                    }
                }
            }
            last_bottom = last_bottom.max(b.bbox[3]);
        }
        if pi + 1 < pages.len() {
            // Each page is its own section so sizes and margins can differ.
            s.push_str(&format!("<w:p><w:pPr>{}</w:pPr></w:p>", sect_pr(page)));
        } else {
            s.push_str(&sect_pr(page));
        }
    }
    s
}

const STYLES: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
    "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:docDefaults><w:rPrDefault><w:rPr>",
    "<w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Calibri\" w:cs=\"Arial\"/>",
    "<w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\" w:eastAsia=\"en-US\" w:bidi=\"ar-SA\"/>",
    "</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>",
    "<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/>",
    "<w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/>",
    "<w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>",
    "</w:styles>"
);

pub fn write_docx(pages: &[Page]) -> R<Vec<u8>> {
    if pages.is_empty() {
        return Err("nothing to export".into());
    }
    let mut media = Media {
        rels: Vec::new(),
        files: Vec::new(),
    };
    let body = body(pages, &mut media);
    let document = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:wp=\"{WP_NS}\" xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\"><w:body>{body}</w:body></w:document>"
    );
    let mut rels = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    rels.push_str(&format!(
        "<Relationship Id=\"rIdStyles\" Type=\"{REL_STYLES}\" Target=\"styles.xml\"/>"
    ));
    for (id, target) in &media.rels {
        rels.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{REL_IMAGE}\" Target=\"{target}\"/>"
        ));
    }
    rels.push_str("</Relationships>");
    let content_types = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
        "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
        "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
        "<Default Extension=\"png\" ContentType=\"image/png\"/>",
        "<Default Extension=\"jpg\" ContentType=\"image/jpeg\"/>",
        "<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>",
        "<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>",
        "</Types>"
    );
    let root_rels = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"{REL_DOC}\" Target=\"word/document.xml\"/></Relationships>"
    );
    let mut parts: Vec<(&str, Vec<u8>)> = vec![
        ("[Content_Types].xml", content_types.as_bytes().to_vec()),
        ("_rels/.rels", root_rels.into_bytes()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", rels.into_bytes()),
        ("word/styles.xml", STYLES.as_bytes().to_vec()),
    ];
    for (name, bytes) in &media.files {
        parts.push((name.as_str(), bytes.clone()));
    }
    package(&parts)
}

#[cfg(test)]
trait PngBytes {
    fn write_to_png(&self) -> Option<Vec<u8>>;
}

#[cfg(test)]
impl PngBytes for image::RgbImage {
    fn write_to_png(&self) -> Option<Vec<u8>> {
        let mut buf = std::io::Cursor::new(Vec::new());
        self.write_to(&mut buf, image::ImageFormat::Png).ok()?;
        Some(buf.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::super::xml::unpack;
    use super::*;
    use base64::Engine;

    fn para(text: &str, dir: Dir, align: Align, bbox: [f64; 4]) -> Block {
        Block {
            bbox,
            item: Item::Para(Para {
                text: text.into(),
                size: 12.0,
                bold: true,
                italic: false,
                color: [0.0; 3],
                align,
                dir,
            }),
        }
    }

    #[test]
    fn alignment_flips_for_arabic_paragraphs() {
        assert_eq!(jc(Align::Start, Dir::Ltr), "left");
        assert_eq!(jc(Align::End, Dir::Rtl), "left");
        assert_eq!(jc(Align::Start, Dir::Rtl), "right");
        assert_eq!(jc(Align::Justify, Dir::Rtl), "both");
    }

    #[test]
    fn writes_a_document_with_text_table_and_picture() {
        let table = TableItem {
            cols: vec![100.0, 50.0],
            rows: vec![
                vec![
                    CellItem {
                        text: "الوصف".into(),
                        span: 1,
                        align: Align::End,
                        dir: Dir::Rtl,
                        bold: true,
                        italic: false,
                        size: 10.0,
                    },
                    CellItem {
                        text: "1".into(),
                        span: 1,
                        align: Align::Center,
                        dir: Dir::Auto,
                        bold: false,
                        italic: false,
                        size: 10.0,
                    },
                ],
                vec![CellItem {
                    text: "wide\ncell".into(),
                    span: 2,
                    align: Align::Start,
                    dir: Dir::Ltr,
                    bold: false,
                    italic: true,
                    size: 10.0,
                }],
            ],
            ruled: true,
            rtl: true,
            col_edges: vec![],
            row_edges: vec![],
        };
        let png = base64::engine::general_purpose::STANDARD.encode(
            image::RgbImage::from_pixel(4, 4, image::Rgb([9, 9, 9]))
                .write_to_png()
                .unwrap(),
        );
        let pages = vec![
            Page {
                number: 1,
                width: 595.0,
                height: 842.0,
                blocks: vec![
                    para("عنوان", Dir::Rtl, Align::End, [300.0, 50.0, 540.0, 70.0]),
                    Block {
                        bbox: [60.0, 100.0, 210.0, 160.0],
                        item: Item::Table(table),
                    },
                    Block {
                        bbox: [60.0, 200.0, 260.0, 300.0],
                        item: Item::Picture(super::super::model::Picture {
                            png_b64: png,
                            width: 200.0,
                            height: 100.0,
                        }),
                    },
                ],
            },
            Page {
                number: 2,
                width: 842.0,
                height: 595.0,
                blocks: vec![para(
                    "Second <page> & more",
                    Dir::Ltr,
                    Align::Start,
                    [60.0, 60.0, 400.0, 80.0],
                )],
            },
        ];
        let bytes = write_docx(&pages).unwrap();
        let doc = unpack(&bytes, "word/document.xml");
        assert!(doc.contains("<w:bidi/>"));
        assert!(doc.contains("<w:jc w:val=\"left\"/>"), "{doc}");
        assert!(doc.contains("<w:bidiVisual/>"));
        assert!(doc.contains("<w:gridSpan w:val=\"2\"/>"));
        assert!(doc.contains("<w:br/>"));
        assert!(doc.contains("r:embed=\"rIdImg1\""));
        assert!(doc.contains("Second &lt;page&gt; &amp; more"));
        assert!(doc.contains("w:orient=\"landscape\""));
        assert_eq!(doc.matches("<w:sectPr>").count(), 2);
        // The right-to-left table lists its first cell last.
        let first_cell = doc.find("<w:tc>").unwrap();
        assert!(
            doc[first_cell..]
                .find("<w:t xml:space=\"preserve\">1</w:t>")
                .unwrap()
                < doc[first_cell..].find("الوصف").unwrap()
        );
        let rels = unpack(&bytes, "word/_rels/document.xml.rels");
        assert!(rels.contains("Target=\"media/image1.png\""));
        assert!(unpack(&bytes, "[Content_Types].xml").contains("image/png"));
        assert!(
            super::super::xml::unpack_bytes(&bytes, "word/media/image1.png")
                .starts_with(b"\x89PNG")
        );
    }
}
