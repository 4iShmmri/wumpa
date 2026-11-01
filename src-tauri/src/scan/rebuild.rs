// Replaces scanned pages of a document with composed text pages, keeping every
// other page as it was.
use crate::common::{err, load_plain, R};
use crate::compose::{self, Composer};
use crate::pages::{self, PageSpec};
use crate::scan::{analyse_page, Progress, ScanOptions};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub page: u32,
    pub index: usize,
    pub total: usize,
    pub stage: String,
}

// Internal pipeline stages collapsed to what the UI shows.
fn ui_stage(stage: &str) -> &'static str {
    match stage {
        "decode" => "detect",
        "deskew" => "prepare",
        "ocr" => "recognize",
        "done" => "compose",
        _ => "layout",
    }
}

pub fn rebuild_scanned(
    src: &str,
    out: &str,
    pages: &[u32],
    langs: Vec<String>,
    on_progress: &mut dyn FnMut(ScanProgress),
) -> R<usize> {
    let mut doc = load_plain(src)?;
    let page_map = doc.get_pages();
    let count = page_map.len() as u32;
    let opts = ScanOptions { langs, flow: false };
    let mut composer = Composer::new();
    let mut replaced: HashMap<u32, i32> = HashMap::new();
    let total = pages.len();

    for (index, &p) in pages.iter().enumerate() {
        let Some(&pid) = page_map.get(&p) else {
            continue;
        };
        let mut report = |stage: &str| {
            on_progress(ScanProgress {
                page: p,
                index,
                total,
                stage: stage.to_string(),
            });
        };
        let blocks = {
            let mut cb = |stage: &str, _done: usize, _total: usize| report(ui_stage(stage));
            let mut progress = Progress(&mut cb);
            analyse_page(&doc, pid, &opts, &mut progress)?
        };
        report("compose");
        let page: compose::PageBlocks =
            serde_json::from_value(serde_json::to_value(&blocks).map_err(err)?).map_err(err)?;
        composer.page(&mut doc, &page)?;
        replaced.insert(p, (count + replaced.len() as u32 + 1) as i32);
    }
    composer.finish(&mut doc)?;

    // The composed pages were appended; put each one where its scan was.
    let specs: Vec<PageSpec> = (1..=count)
        .map(|i| PageSpec::page(replaced.get(&i).copied().unwrap_or(i as i32), 0))
        .collect();
    pages::rebuild(&mut doc, &specs, &[], &[], &[])?;
    doc.save(out).map_err(err)?;
    Ok(replaced.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs the OCR sidecar and a real scanned PDF; run with
    // `WUMPA_SCAN_SRC=/path/to/scan.pdf cargo test -- --ignored rebuild_scanned_end_to_end`.
    #[test]
    #[ignore]
    fn rebuild_scanned_end_to_end() {
        let dir = std::env::temp_dir();
        let src = std::env::var("WUMPA_SCAN_SRC")
            .unwrap_or_else(|_| dir.join("scan.pdf").to_string_lossy().into_owned());
        let out = std::env::var("WUMPA_SCAN_OUT")
            .unwrap_or_else(|_| dir.join("rebuilt.pdf").to_string_lossy().into_owned());
        let mut stages = Vec::new();
        let n = rebuild_scanned(&src, &out, &[1], vec!["ar".into(), "en".into()], &mut |p| {
            stages.push(p.stage)
        })
        .unwrap();
        assert_eq!(n, 1);
        assert!(stages.contains(&"recognize".to_string()));
        let doc = lopdf::Document::load(&out).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
        eprintln!("stages={stages:?}");
    }
}
