// On-device text recognition through the `wumpa-ocr` sidecar. The sidecar
// prints Vision-style boxes (normalised, origin bottom-left); they are
// converted here to pixel boxes with the origin at the top-left corner.
use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::common::R;

#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    pub bbox: [f64; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub conf: f64,
    pub bbox: [f64; 4],
    pub words: Vec<OcrWord>,
}

#[derive(Deserialize)]
struct RawBox {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Deserialize)]
struct RawWord {
    text: String,
    #[serde(rename = "box")]
    bbox: RawBox,
}

#[derive(Deserialize)]
struct RawLine {
    text: String,
    conf: f64,
    #[serde(rename = "box")]
    bbox: RawBox,
    #[serde(default)]
    words: Vec<RawWord>,
}

#[derive(Deserialize)]
struct RawOutput {
    width: f64,
    height: f64,
    lines: Vec<RawLine>,
}

#[cfg(target_os = "macos")]
pub fn sidecar_path() -> R<PathBuf> {
    let path = if cfg!(debug_assertions) {
        PathBuf::from(format!(
            "{}/binaries/wumpa-ocr-{}-apple-darwin",
            env!("CARGO_MANIFEST_DIR"),
            std::env::consts::ARCH
        ))
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("wumpa-ocr")))
            .ok_or("cannot locate the application directory")?
    };
    if !path.is_file() {
        return Err(format!(
            "text recognition helper not found at {}",
            path.display()
        ));
    }
    Ok(path)
}

#[cfg(not(target_os = "macos"))]
pub fn sidecar_path() -> R<PathBuf> {
    Err("text recognition is not available on this platform yet".into())
}

// Vision boxes: normalised 0..1, origin bottom-left.
fn to_pixels(b: &RawBox, w: f64, h: f64) -> [f64; 4] {
    let x1 = b.x * w;
    let x2 = (b.x + b.w) * w;
    let y1 = (1.0 - b.y - b.h) * h;
    let y2 = (1.0 - b.y) * h;
    [x1.max(0.0), y1.max(0.0), x2.min(w), y2.min(h)]
}

fn parse(json: &[u8]) -> R<Vec<OcrLine>> {
    let raw: RawOutput = serde_json::from_slice(json).map_err(|e| format!("ocr output: {e}"))?;
    let (w, h) = (raw.width, raw.height);
    if w <= 0.0 || h <= 0.0 {
        return Err("ocr output: missing image size".into());
    }
    Ok(raw
        .lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty())
        .map(|l| OcrLine {
            text: l.text,
            conf: l.conf,
            bbox: to_pixels(&l.bbox, w, h),
            words: l
                .words
                .into_iter()
                .map(|wd| OcrWord {
                    text: wd.text,
                    bbox: to_pixels(&wd.bbox, w, h),
                })
                .collect(),
        })
        .collect())
}

// `raw` turns language correction off; better for lone digits and codes.
pub fn recognize(image_png: &Path, langs: &[&str], raw: bool) -> R<Vec<OcrLine>> {
    let bin = sidecar_path()?;
    let langs = if langs.is_empty() {
        "ar,en".to_string()
    } else {
        langs.join(",")
    };
    let mut cmd = std::process::Command::new(&bin);
    cmd.arg(image_png).arg(langs);
    if raw {
        cmd.arg("--raw");
    }
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run text recognition: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(format!("text recognition failed: {}", msg.trim()));
    }
    parse(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_vision_boxes_to_top_left_pixels() {
        let json = br#"{"width":200,"height":100,"lines":[
            {"text":"hi","conf":0.9,"box":{"x":0.1,"y":0.8,"w":0.2,"h":0.1},
             "words":[{"text":"hi","box":{"x":0.1,"y":0.8,"w":0.2,"h":0.1}}]},
            {"text":"  ","conf":1,"box":{"x":0,"y":0,"w":0.1,"h":0.1},"words":[]}
        ]}"#;
        let lines = parse(json).unwrap();
        assert_eq!(lines.len(), 1);
        let b = lines[0].bbox;
        assert!((b[0] - 20.0).abs() < 1e-9);
        assert!((b[1] - 10.0).abs() < 1e-9);
        assert!((b[2] - 60.0).abs() < 1e-9);
        assert!((b[3] - 20.0).abs() < 1e-9);
        assert_eq!(lines[0].words[0].bbox, b);
    }

    #[test]
    fn rejects_bad_json() {
        assert!(parse(b"nope").is_err());
    }
}
