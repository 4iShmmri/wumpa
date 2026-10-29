// Small helpers for writing Office Open XML by hand.
use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::common::R;

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn hex_color(c: [f64; 3]) -> String {
    let ch = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("{:02X}{:02X}{:02X}", ch(c[0]), ch(c[1]), ch(c[2]))
}

// Points to twentieths of a point, the unit of most Word measurements.
pub fn twips(pt: f64) -> i64 {
    (pt * 20.0).round() as i64
}

// Points to English Metric Units, the unit of drawing sizes.
pub fn emu(pt: f64) -> i64 {
    (pt * 12700.0).round() as i64
}

// A picture is stored as it came (PNG) unless it is a large photograph
// with many colours, where JPEG is several times smaller at no visible
// cost. Returns the file extension and the bytes.
pub fn picture_file(png_b64: &str) -> Option<(&'static str, Vec<u8>)> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png_b64)
        .ok()?;
    if bytes.len() < 200_000 {
        return Some(("png", bytes));
    }
    let rgb = image::load_from_memory(&bytes).ok()?.to_rgb8();
    if !crate::common::photo_like(&rgb) {
        return Some(("png", bytes));
    }
    Some(("jpg", crate::common::jpeg_bytes(&rgb, 85).ok()?))
}

// Builds a zip in memory from (name, bytes) parts.
pub fn package(parts: &[(&str, Vec<u8>)]) -> R<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in parts {
        zip.start_file(*name, opts).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
pub fn unpack_bytes(bytes: &[u8], name: &str) -> Vec<u8> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut f = zip.by_name(name).unwrap();
    let mut v = Vec::new();
    f.read_to_end(&mut v).unwrap();
    v
}

#[cfg(test)]
pub fn unpack(bytes: &[u8], name: &str) -> String {
    String::from_utf8(unpack_bytes(bytes, name)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_drops_controls() {
        assert_eq!(esc("a<b>&\"c\"\u{1}"), "a&lt;b&gt;&amp;&quot;c&quot;");
        assert_eq!(hex_color([1.0, 0.5, 0.0]), "FF8000");
        assert_eq!(twips(12.0), 240);
        assert_eq!(emu(1.0), 12700);
    }

    #[test]
    fn packages_round_trip() {
        let bytes = package(&[("a.xml", b"<a/>".to_vec()), ("d/b.txt", b"hi".to_vec())]).unwrap();
        assert_eq!(unpack(&bytes, "a.xml"), "<a/>");
        assert_eq!(unpack(&bytes, "d/b.txt"), "hi");
    }
}
