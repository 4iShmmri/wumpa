// The reference documents: real pages whose analysis must keep coming out
// right. Each pairs a PDF with a raster of its first page, as the app
// would send them, and lists what the document model must contain.
use base64::Engine;

use super::model::{Item, Page};
use super::{analyse_input, PageInput};
use crate::scan::Progress;

struct Expect {
    name: &'static str,
    rtl: bool,
    // (rows, cols, ruled) of every table on page 1, in reading order.
    tables: &'static [(usize, usize, bool)],
    min_pictures: usize,
    // Paragraph text that must appear, exactly as a substring.
    paragraphs: &'static [&'static str],
    // Cell text that must appear in some table, exactly.
    cells: &'static [&'static str],
    // Text that must not appear anywhere (regressions caught before).
    never: &'static [&'static str],
}

const DOCS: &[Expect] = &[
    Expect {
        name: "ar-report",
        rtl: true,
        tables: &[(5, 5, true)],
        min_pictures: 0,
        paragraphs: &[
            "التقرير السنوي لقطاع الخدمات اللوجستية",
            "1. الملخص التنفيذي",
            "خلال العام المالي 2024",
            "اسم الجهة: شركة الأفق للخدمات اللوجستية\nالعنوان:",
            "بناءً على البيانات",
        ],
        cells: &["التكلفة (ريال)", "3,450,000.00", "الإجمالي", "22%"],
        never: &["امللخص", "خالل", "األفق", "بنا ًء", ")ريال("],
    },
    Expect {
        name: "en-article",
        rtl: false,
        tables: &[(4, 4, false)],
        min_pictures: 1,
        paragraphs: &[
            "Why Small Warehouses Win",
            "By Dana Reed",
            "For decades the orthodoxy in distribution was simple: bigger is better.",
            "Whatever the answer, the lesson of the past five years is clear enough",
        ],
        cells: &[
            "Avg.\ndelivery",
            "Cost /\nparcel",
            "Micro-\nfulfilment",
            "$2.95",
        ],
        never: &["W arehouses", "was\nsimple"],
    },
    Expect {
        name: "en-invoice",
        rtl: false,
        tables: &[(4, 2, false), (5, 5, false), (3, 2, false)],
        min_pictures: 0,
        paragraphs: &[
            "Bill to:\nContoso Pharmaceuticals",
            "Payment by bank transfer",
        ],
        cells: &[
            "INVOICE",
            "No. INV-2025-0142",
            "Description",
            "(300.00)",
            "Total due (GBP)",
            "4,041.60",
        ],
        never: &["INVOIC E", "D ate"],
    },
    Expect {
        name: "t-ar",
        rtl: true,
        tables: &[(5, 5, true)],
        min_pictures: 3,
        paragraphs: &[
            "شركة الأفق للتجهيزات المكتبية",
            "رقم الفاتورة: INV-2024-0187",
            "التاريخ: 14 مارس 2024",
            "Thank you for your business.",
        ],
        cells: &["طاولة اجتماعات خشب بلوط 240 سم", "4,800.00", "الوصف", "6"],
        never: &["شركةا لأفق", "0187-2024-INV"],
    },
    Expect {
        name: "t-pf",
        rtl: false,
        tables: &[(6, 2, true)],
        min_pictures: 1,
        paragraphs: &[
            "Dear Customer,",
            "Import parcel invoice and information",
            "Yours faithfully",
        ],
        cells: &[
            "Customs Duty",
            "£48.74",
            "Clearance Fee (zero rated for VAT)",
        ],
        never: &[],
    },
];

fn testdata(name: &str) -> String {
    format!("{}/testdata/export/{name}", env!("CARGO_MANIFEST_DIR"))
}

// The first page as the app sends it: no viewer lines (the file's own
// glyphs carry the text) and the page raster re-encoded as PNG.
fn input(name: &str) -> (String, PageInput) {
    let src = testdata(&format!("{name}.pdf"));
    let jpg = image::open(testdata(&format!("{name}.jpg")))
        .unwrap()
        .to_rgb8();
    let mut png = std::io::Cursor::new(Vec::new());
    jpg.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let doc = lopdf::Document::load(&src).unwrap();
    let pid = doc.get_pages()[&1];
    let page = doc.get_dictionary(pid).unwrap();
    let media = page
        .get(b"MediaBox")
        .map(|o| crate::editor::deref(&doc, o))
        .unwrap()
        .as_array()
        .unwrap();
    let g = |i: usize| crate::editor::num(&media[i]).unwrap();
    let (width, height) = ((g(2) - g(0)).abs(), (g(3) - g(1)).abs());
    let input = PageInput::Native {
        page: 1,
        width,
        height,
        raster_png_b64: base64::engine::general_purpose::STANDARD.encode(png.into_inner()),
        lines: Vec::new(),
    };
    (src, input)
}

fn analyse(name: &str) -> Page {
    let (src, input) = input(name);
    let mut cb = |_: &str, _: usize, _: usize| {};
    analyse_input(&src, input, Vec::new(), &mut Progress(&mut cb)).unwrap()
}

fn check(page: &Page, e: &Expect) {
    let paras: Vec<&str> = page
        .blocks
        .iter()
        .filter_map(|b| match &b.item {
            Item::Para(p) => Some(p.text.as_str()),
            _ => None,
        })
        .collect();
    let tables: Vec<_> = page
        .blocks
        .iter()
        .filter_map(|b| match &b.item {
            Item::Table(t) => Some(t),
            _ => None,
        })
        .collect();
    let pictures = page
        .blocks
        .iter()
        .filter(|b| matches!(b.item, Item::Picture(_)))
        .count();
    let cells: Vec<String> = tables
        .iter()
        .flat_map(|t| t.rows.iter().flatten().map(|c| c.text.clone()))
        .collect();
    let everything = format!("{}\n{}", paras.join("\n"), cells.join("\n"));

    assert_eq!(page.rtl(), e.rtl, "{}: direction", e.name);
    let shapes: Vec<(usize, usize, bool)> = tables
        .iter()
        .map(|t| (t.rows.len(), t.cols.len(), t.ruled))
        .collect();
    assert_eq!(
        shapes, e.tables,
        "{}: tables, paragraphs were {paras:#?}",
        e.name
    );
    assert!(
        pictures >= e.min_pictures,
        "{}: {pictures} pictures",
        e.name
    );
    for want in e.paragraphs {
        assert!(
            paras.iter().any(|p| p.contains(want)),
            "{}: paragraph {want:?} missing from {paras:#?}",
            e.name
        );
    }
    for want in e.cells {
        assert!(
            cells.iter().any(|c| c == want),
            "{}: cell {want:?} missing from {cells:#?}",
            e.name
        );
    }
    for bad in e.never {
        assert!(!everything.contains(bad), "{}: found {bad:?}", e.name);
    }
}

#[test]
fn reference_documents_analyse_as_expected() {
    for e in DOCS {
        check(&analyse(e.name), e);
    }
}

#[test]
fn reference_documents_export_to_both_formats() {
    let pages: Vec<Page> = DOCS.iter().map(|e| analyse(e.name)).collect();
    let docx = super::docx::write_docx(&pages).unwrap();
    let xlsx = super::xlsx::write_xlsx(&pages, "ar").unwrap();
    for bytes in [&docx, &xlsx] {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
        for name in names {
            use std::io::Read;
            let mut data = Vec::new();
            zip.by_name(&name).unwrap().read_to_end(&mut data).unwrap();
            if name.ends_with(".xml") || name.ends_with(".rels") {
                let text = String::from_utf8(data).unwrap();
                assert!(text.starts_with("<?xml"), "{name}");
                assert!(!text.contains('\u{0}'), "{name}");
            }
        }
    }
    assert!(docx.len() < 1_500_000, "docx is {} bytes", docx.len());
    assert!(xlsx.len() < 1_500_000, "xlsx is {} bytes", xlsx.len());
}
