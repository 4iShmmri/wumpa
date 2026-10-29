// Walks a page's content and reports every glyph it shows: position,
// advance, size, the text its font maps it to, and the font's weight and
// slant. Text inside form XObjects is followed too.
use std::collections::HashMap;

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::common::{err, R};
use crate::editor::fonts::{self, WidthModel};
use crate::editor::mat::{compose, transform, translate, Mat, IDENT};
use crate::editor::{deref, num, opnum};

#[derive(Debug, Clone)]
pub struct Glyph {
    // Origin on the baseline, PDF user space.
    pub x: f64,
    pub y: f64,
    // Advance along the baseline and font size, both in page units.
    pub w: f64,
    pub size: f64,
    // None when the font gives no way to read the glyph.
    pub text: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

// Form XObjects nested deeper than this are not followed.
const MAX_DEPTH: usize = 8;

enum Encoding {
    // Code → text from a ToUnicode CMap.
    Map(HashMap<u32, String>),
    // A single-byte font with a known base encoding and differences.
    Simple(Box<[Option<String>; 256]>),
    Unknown,
}

struct Font {
    widths: WidthModel,
    two_byte: bool,
    // Type 3 glyph space to text space, per 1000 units.
    scale: f64,
    encoding: Encoding,
    bold: bool,
    italic: bool,
}

impl Font {
    fn text(&self, code: u32) -> Option<String> {
        match &self.encoding {
            Encoding::Map(m) => m.get(&code).cloned(),
            Encoding::Simple(t) => t.get(code as usize).cloned().flatten(),
            Encoding::Unknown => None,
        }
    }
}

// ---- ToUnicode CMaps ----

fn hex_bytes(tok: &[u8]) -> Option<Vec<u8>> {
    let inner = tok.strip_prefix(b"<")?.strip_suffix(b">")?;
    let clean: Vec<u8> = inner
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(clean.len() / 2);
    for pair in clean.chunks(2) {
        let s = std::str::from_utf8(pair).ok()?;
        out.push(u8::from_str_radix(s, 16).ok()?);
    }
    Some(out)
}

fn code_of(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, b| (acc << 8) | *b as u32)
}

fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks(2)
        .map(|c| ((c[0] as u16) << 8) | *c.get(1).unwrap_or(&0) as u16)
        .collect();
    String::from_utf16_lossy(&units)
}

// Splits a CMap into tokens: hex strings, brackets, names and keywords.
fn cmap_tokens(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let c = data[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'%' {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
        } else if c == b'<' {
            let start = i;
            while i < data.len() && data[i] != b'>' {
                i += 1;
            }
            i += 1;
            out.push(&data[start..i.min(data.len())]);
        } else if c == b'[' || c == b']' {
            out.push(&data[i..i + 1]);
            i += 1;
        } else {
            let start = i;
            while i < data.len()
                && !data[i].is_ascii_whitespace()
                && !matches!(data[i], b'<' | b'[' | b']')
            {
                i += 1;
            }
            out.push(&data[start..i]);
        }
    }
    out
}

pub fn parse_to_unicode(data: &[u8]) -> HashMap<u32, String> {
    let toks = cmap_tokens(data);
    let mut map = HashMap::new();
    let mut i = 0;
    while i < toks.len() {
        match toks[i] {
            b"beginbfchar" => {
                i += 1;
                while i + 1 < toks.len() && toks[i] != b"endbfchar" {
                    if let (Some(src), Some(dst)) = (hex_bytes(toks[i]), hex_bytes(toks[i + 1])) {
                        map.insert(code_of(&src), utf16(&dst));
                    }
                    i += 2;
                }
            }
            b"beginbfrange" => {
                i += 1;
                while i + 2 < toks.len() && toks[i] != b"endbfrange" {
                    let (Some(lo), Some(hi)) = (hex_bytes(toks[i]), hex_bytes(toks[i + 1])) else {
                        i += 1;
                        continue;
                    };
                    let (lo, hi) = (code_of(&lo), code_of(&hi).min(code_of(&lo) + 65535));
                    if toks[i + 2] == b"[" {
                        let mut j = i + 3;
                        let mut code = lo;
                        while j < toks.len() && toks[j] != b"]" {
                            if let Some(dst) = hex_bytes(toks[j]) {
                                if code <= hi {
                                    map.insert(code, utf16(&dst));
                                }
                            }
                            code += 1;
                            j += 1;
                        }
                        i = j + 1;
                    } else {
                        if let Some(dst) = hex_bytes(toks[i + 2]) {
                            let base = utf16(&dst);
                            let mut chars: Vec<char> = base.chars().collect();
                            for code in lo..=hi {
                                map.insert(code, chars.iter().collect());
                                // The range steps the last character.
                                if let Some(last) = chars.last_mut() {
                                    *last = char::from_u32(*last as u32 + 1).unwrap_or(*last);
                                }
                            }
                        }
                        i += 3;
                    }
                }
            }
            _ => i += 1,
        }
    }
    map
}

// ---- Simple-font encodings ----

const WIN_ANSI_HIGH: [u32; 128] = [
    0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC,
    0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6,
    0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD, 0xAE, 0xAF, 0xB0, 0xB1, 0xB2, 0xB3, 0xB4, 0xB5, 0xB6,
    0xB7, 0xB8, 0xB9, 0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0, 0xC1, 0xC2, 0xC3, 0xC4, 0xC5, 0xC6,
    0xC7, 0xC8, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD, 0xCE, 0xCF, 0xD0, 0xD1, 0xD2, 0xD3, 0xD4, 0xD5, 0xD6,
    0xD7, 0xD8, 0xD9, 0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xE0, 0xE1, 0xE2, 0xE3, 0xE4, 0xE5, 0xE6,
    0xE7, 0xE8, 0xE9, 0xEA, 0xEB, 0xEC, 0xED, 0xEE, 0xEF, 0xF0, 0xF1, 0xF2, 0xF3, 0xF4, 0xF5, 0xF6,
    0xF7, 0xF8, 0xF9, 0xFA, 0xFB, 0xFC, 0xFD, 0xFE, 0xFF,
];

const MAC_ROMAN_HIGH: [u32; 128] = [
    0xC4, 0xC5, 0xC7, 0xC9, 0xD1, 0xD6, 0xDC, 0xE1, 0xE0, 0xE2, 0xE4, 0xE3, 0xE5, 0xE7, 0xE9, 0xE8,
    0xEA, 0xEB, 0xED, 0xEC, 0xEE, 0xEF, 0xF1, 0xF3, 0xF2, 0xF4, 0xF6, 0xF5, 0xFA, 0xF9, 0xFB, 0xFC,
    0x2020, 0xB0, 0xA2, 0xA3, 0xA7, 0x2022, 0xB6, 0xDF, 0xAE, 0xA9, 0x2122, 0xB4, 0xA8, 0x2260,
    0xC6, 0xD8, 0x221E, 0xB1, 0x2264, 0x2265, 0xA5, 0xB5, 0x2202, 0x2211, 0x220F, 0x3C0, 0x222B,
    0xAA, 0xBA, 0x3A9, 0xE6, 0xF8, 0xBF, 0xA1, 0xAC, 0x221A, 0x192, 0x2248, 0x2206, 0xAB, 0xBB,
    0x2026, 0xA0, 0xC0, 0xC3, 0xD5, 0x152, 0x153, 0x2013, 0x2014, 0x201C, 0x201D, 0x2018, 0x2019,
    0xF7, 0x25CA, 0xFF, 0x178, 0x2044, 0x20AC, 0x2039, 0x203A, 0xFB01, 0xFB02, 0x2021, 0xB7,
    0x201A, 0x201E, 0x2030, 0xC2, 0xCA, 0xC1, 0xCB, 0xC8, 0xCD, 0xCE, 0xCF, 0xCC, 0xD3, 0xD4,
    0xF8FF, 0xD2, 0xDA, 0xDB, 0xD9, 0x131, 0x2C6, 0x2DC, 0xAF, 0x2D8, 0x2D9, 0x2DA, 0xB8, 0x2DD,
    0x2DB, 0x2C7,
];

// The glyph names a Differences array commonly uses.
const GLYPH_NAMES: &[(&str, char)] = &[
    ("space", ' '),
    ("exclam", '!'),
    ("quotedbl", '"'),
    ("numbersign", '#'),
    ("dollar", '$'),
    ("percent", '%'),
    ("ampersand", '&'),
    ("quotesingle", '\''),
    ("parenleft", '('),
    ("parenright", ')'),
    ("asterisk", '*'),
    ("plus", '+'),
    ("comma", ','),
    ("hyphen", '-'),
    ("period", '.'),
    ("slash", '/'),
    ("zero", '0'),
    ("one", '1'),
    ("two", '2'),
    ("three", '3'),
    ("four", '4'),
    ("five", '5'),
    ("six", '6'),
    ("seven", '7'),
    ("eight", '8'),
    ("nine", '9'),
    ("colon", ':'),
    ("semicolon", ';'),
    ("less", '<'),
    ("equal", '='),
    ("greater", '>'),
    ("question", '?'),
    ("at", '@'),
    ("bracketleft", '['),
    ("backslash", '\\'),
    ("bracketright", ']'),
    ("asciicircum", '^'),
    ("underscore", '_'),
    ("grave", '`'),
    ("braceleft", '{'),
    ("bar", '|'),
    ("braceright", '}'),
    ("asciitilde", '~'),
    ("quoteright", '\u{2019}'),
    ("quoteleft", '\u{2018}'),
    ("quotedblleft", '\u{201C}'),
    ("quotedblright", '\u{201D}'),
    ("quotesinglbase", '\u{201A}'),
    ("quotedblbase", '\u{201E}'),
    ("endash", '\u{2013}'),
    ("emdash", '\u{2014}'),
    ("bullet", '\u{2022}'),
    ("ellipsis", '\u{2026}'),
    ("fi", '\u{FB01}'),
    ("fl", '\u{FB02}'),
    ("ff", '\u{FB00}'),
    ("ffi", '\u{FB03}'),
    ("ffl", '\u{FB04}'),
    ("dagger", '\u{2020}'),
    ("daggerdbl", '\u{2021}'),
    ("section", '\u{A7}'),
    ("paragraph", '\u{B6}'),
    ("copyright", '\u{A9}'),
    ("registered", '\u{AE}'),
    ("trademark", '\u{2122}'),
    ("degree", '\u{B0}'),
    ("plusminus", '\u{B1}'),
    ("multiply", '\u{D7}'),
    ("divide", '\u{F7}'),
    ("sterling", '\u{A3}'),
    ("Euro", '\u{20AC}'),
    ("yen", '\u{A5}'),
    ("cent", '\u{A2}'),
    ("periodcentered", '\u{B7}'),
    ("guillemotleft", '\u{AB}'),
    ("guillemotright", '\u{BB}'),
    ("exclamdown", '\u{A1}'),
    ("questiondown", '\u{BF}'),
    ("minus", '\u{2212}'),
    ("fraction", '\u{2044}'),
    ("nbspace", '\u{A0}'),
    ("softhyphen", '\u{AD}'),
    ("germandbls", '\u{DF}'),
    ("ae", '\u{E6}'),
    ("AE", '\u{C6}'),
    ("oe", '\u{153}'),
    ("OE", '\u{152}'),
    ("eth", '\u{F0}'),
    ("thorn", '\u{FE}'),
    ("Thorn", '\u{DE}'),
    ("dotlessi", '\u{131}'),
    ("Lslash", '\u{141}'),
    ("lslash", '\u{142}'),
    ("Oslash", '\u{D8}'),
    ("oslash", '\u{F8}'),
];

const ACCENTS: &[(&str, char)] = &[
    ("grave", '\u{300}'),
    ("acute", '\u{301}'),
    ("circumflex", '\u{302}'),
    ("tilde", '\u{303}'),
    ("macron", '\u{304}'),
    ("breve", '\u{306}'),
    ("dotaccent", '\u{307}'),
    ("dieresis", '\u{308}'),
    ("ring", '\u{30A}'),
    ("caron", '\u{30C}'),
    ("cedilla", '\u{327}'),
    ("ogonek", '\u{328}'),
    ("hungarumlaut", '\u{30B}'),
];

// Unicode for a glyph name: single letters, uniXXXX / uXXXX forms, the
// common list, and accented letters composed from base and accent.
pub fn glyph_name_to_text(name: &str) -> Option<String> {
    let name = name.split('.').next().unwrap_or(name);
    if name.chars().count() == 1 {
        return Some(name.to_string());
    }
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 && hex.len() % 4 == 0 {
            let mut s = String::new();
            for chunk in hex.as_bytes().chunks(4) {
                let v = u32::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
                s.push(char::from_u32(v)?);
            }
            return Some(s);
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) {
            if let Ok(v) = u32::from_str_radix(hex, 16) {
                return char::from_u32(v).map(String::from);
            }
        }
    }
    if let Some((_, c)) = GLYPH_NAMES.iter().find(|(n, _)| *n == name) {
        return Some(c.to_string());
    }
    // Letters with an accent: "eacute", "Udieresis".
    let mut chars = name.chars();
    let base = chars.next()?;
    let rest: &str = &name[base.len_utf8()..];
    if base.is_ascii_alphabetic() {
        if let Some((_, accent)) = ACCENTS.iter().find(|(n, _)| *n == rest) {
            let composed: String = unicode_normalization_compose(base, *accent);
            return Some(composed);
        }
    }
    None
}

fn unicode_normalization_compose(base: char, accent: char) -> String {
    // A small table of precomposed Latin letters; others stay decomposed,
    // which still reads correctly.
    const TABLE: &[(char, char, char)] = &[
        ('a', '\u{301}', 'á'),
        ('e', '\u{301}', 'é'),
        ('i', '\u{301}', 'í'),
        ('o', '\u{301}', 'ó'),
        ('u', '\u{301}', 'ú'),
        ('a', '\u{300}', 'à'),
        ('e', '\u{300}', 'è'),
        ('i', '\u{300}', 'ì'),
        ('o', '\u{300}', 'ò'),
        ('u', '\u{300}', 'ù'),
        ('a', '\u{302}', 'â'),
        ('e', '\u{302}', 'ê'),
        ('i', '\u{302}', 'î'),
        ('o', '\u{302}', 'ô'),
        ('u', '\u{302}', 'û'),
        ('a', '\u{308}', 'ä'),
        ('e', '\u{308}', 'ë'),
        ('i', '\u{308}', 'ï'),
        ('o', '\u{308}', 'ö'),
        ('u', '\u{308}', 'ü'),
        ('a', '\u{303}', 'ã'),
        ('n', '\u{303}', 'ñ'),
        ('o', '\u{303}', 'õ'),
        ('c', '\u{327}', 'ç'),
        ('a', '\u{30A}', 'å'),
        ('A', '\u{301}', 'Á'),
        ('E', '\u{301}', 'É'),
        ('I', '\u{301}', 'Í'),
        ('O', '\u{301}', 'Ó'),
        ('U', '\u{301}', 'Ú'),
        ('A', '\u{300}', 'À'),
        ('E', '\u{300}', 'È'),
        ('A', '\u{302}', 'Â'),
        ('E', '\u{302}', 'Ê'),
        ('O', '\u{302}', 'Ô'),
        ('A', '\u{308}', 'Ä'),
        ('O', '\u{308}', 'Ö'),
        ('U', '\u{308}', 'Ü'),
        ('N', '\u{303}', 'Ñ'),
        ('C', '\u{327}', 'Ç'),
        ('A', '\u{30A}', 'Å'),
        ('y', '\u{308}', 'ÿ'),
        ('y', '\u{301}', 'ý'),
        ('s', '\u{30C}', 'š'),
        ('S', '\u{30C}', 'Š'),
        ('z', '\u{30C}', 'ž'),
        ('Z', '\u{30C}', 'Ž'),
        ('c', '\u{30C}', 'č'),
        ('C', '\u{30C}', 'Č'),
    ];
    match TABLE.iter().find(|(b, a, _)| *b == base && *a == accent) {
        Some((_, _, c)) => c.to_string(),
        None => format!("{base}{accent}"),
    }
}

fn base_table(name: &[u8]) -> Box<[Option<String>; 256]> {
    let mut t: Box<[Option<String>; 256]> = Box::new(std::array::from_fn(|_| None));
    for (i, slot) in t.iter_mut().enumerate().take(127).skip(32) {
        *slot = Some((i as u8 as char).to_string());
    }
    let high: Option<&[u32; 128]> = match name {
        b"WinAnsiEncoding" => Some(&WIN_ANSI_HIGH),
        b"MacRomanEncoding" => Some(&MAC_ROMAN_HIGH),
        _ => None,
    };
    if let Some(high) = high {
        for (i, &cp) in high.iter().enumerate() {
            if cp != 0 {
                t[128 + i] = char::from_u32(cp).map(String::from);
            }
        }
    }
    if name == b"StandardEncoding" {
        t[39] = Some("\u{2019}".into());
        t[96] = Some("\u{2018}".into());
    }
    t
}

fn simple_encoding(doc: &Document, font: &Dictionary) -> Encoding {
    let enc = font.get(b"Encoding").map(|o| deref(doc, o)).ok();
    let symbolic = font
        .get(b"FontDescriptor")
        .map(|o| deref(doc, o))
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Flags").ok().and_then(num))
        .is_some_and(|f| (f as u32) & 4 != 0 && (f as u32) & 32 == 0);
    let mut table = match enc {
        Some(Object::Name(n)) => base_table(n),
        Some(Object::Dictionary(d)) => {
            let base = d
                .get(b"BaseEncoding")
                .ok()
                .and_then(|o| o.as_name().ok())
                .unwrap_or(if symbolic { b"" } else { b"StandardEncoding" });
            let mut t = base_table(base);
            if let Ok(diff) = d.get(b"Differences").map(|o| deref(doc, o)) {
                if let Ok(arr) = diff.as_array() {
                    let mut code = 0usize;
                    for el in arr {
                        match deref(doc, el) {
                            Object::Name(n) => {
                                if code < 256 {
                                    t[code] = glyph_name_to_text(&String::from_utf8_lossy(n));
                                    code += 1;
                                }
                            }
                            o => {
                                if let Some(c) = num(o) {
                                    code = c.max(0.0) as usize;
                                }
                            }
                        }
                    }
                }
            }
            t
        }
        _ if symbolic => return Encoding::Unknown,
        _ => base_table(b"StandardEncoding"),
    };
    // Control codes never carry text.
    for slot in table.iter_mut().take(32) {
        *slot = None;
    }
    Encoding::Simple(table)
}

fn font_style(doc: &Document, font: &Dictionary, base_font: &str) -> (bool, bool) {
    let desc = font
        .get(b"FontDescriptor")
        .map(|o| deref(doc, o))
        .ok()
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .or_else(|| {
            // Type 0: the descriptor lives on the descendant.
            font.get(b"DescendantFonts")
                .map(|o| deref(doc, o))
                .ok()
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first())
                .map(|o| deref(doc, o))
                .and_then(|o| o.as_dict().ok())
                .and_then(|d| d.get(b"FontDescriptor").map(|o| deref(doc, o)).ok())
                .and_then(|o| o.as_dict().ok())
                .cloned()
        });
    let name = base_font.to_ascii_lowercase();
    let flags = desc
        .as_ref()
        .and_then(|d| d.get(b"Flags").ok().and_then(num))
        .unwrap_or(0.0) as u32;
    let weight = desc
        .as_ref()
        .and_then(|d| d.get(b"FontWeight").ok().and_then(num))
        .unwrap_or(0.0);
    let bold = name.contains("bold")
        || name.contains("black")
        || name.contains("heavy")
        || name.contains("semibold")
        || flags & (1 << 18) != 0
        || weight >= 600.0;
    let italic = name.contains("italic") || name.contains("oblique") || flags & (1 << 6) != 0;
    (bold, italic)
}

fn load_font(doc: &Document, font: &Dictionary) -> Font {
    let subtype = font
        .get(b"Subtype")
        .and_then(|o| o.as_name())
        .unwrap_or(b"")
        .to_vec();
    let base_font = font
        .get(b"BaseFont")
        .and_then(|o| o.as_name())
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    let widths = fonts::width_model(doc, font);
    let two_byte = subtype == b"Type0";
    let scale = if subtype == b"Type3" {
        font.get(b"FontMatrix")
            .map(|o| deref(doc, o))
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first().and_then(num))
            .map(|m| m * 1000.0)
            .unwrap_or(1.0)
    } else {
        1.0
    };
    let to_unicode = font
        .get(b"ToUnicode")
        .map(|o| deref(doc, o))
        .ok()
        .and_then(|o| o.as_stream().ok())
        .and_then(|s| s.decompressed_content().ok())
        .map(|data| parse_to_unicode(&data))
        .filter(|m| !m.is_empty());
    let encoding = match to_unicode {
        Some(map) => Encoding::Map(map),
        None if two_byte => Encoding::Unknown,
        None => simple_encoding(doc, font),
    };
    let (bold, italic) = font_style(doc, font, &base_font);
    Font {
        widths,
        two_byte,
        scale,
        encoding,
        bold,
        italic,
    }
}

fn load_fonts(doc: &Document, res: &Dictionary) -> HashMap<Vec<u8>, Font> {
    let mut out = HashMap::new();
    let Some(fd) = res
        .get(b"Font")
        .ok()
        .and_then(|o| deref(doc, o).as_dict().ok())
    else {
        return out;
    };
    for (name, val) in fd.iter() {
        if let Ok(d) = deref(doc, val).as_dict() {
            out.insert(name.clone(), load_font(doc, d));
        }
    }
    out
}

fn page_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let mut cur = Some(page_id);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else {
            break;
        };
        if let Ok(res) = dict.get(b"Resources") {
            if let Ok(d) = deref(doc, res).as_dict() {
                return d.clone();
            }
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    Dictionary::new()
}

fn forms(doc: &Document, res: &Dictionary) -> HashMap<Vec<u8>, Stream> {
    let mut out = HashMap::new();
    let Some(xd) = res
        .get(b"XObject")
        .ok()
        .and_then(|o| deref(doc, o).as_dict().ok())
    else {
        return out;
    };
    for (name, val) in xd.iter() {
        if let Ok(s) = deref(doc, val).as_stream() {
            if s.dict.get(b"Subtype").and_then(|o| o.as_name()).ok() == Some(b"Form") {
                out.insert(name.clone(), s.clone());
            }
        }
    }
    out
}

fn matrix(doc: &Document, dict: &Dictionary, key: &[u8]) -> Option<Mat> {
    let arr = dict.get(key).map(|o| deref(doc, o)).ok()?.as_array().ok()?;
    if arr.len() < 6 {
        return None;
    }
    let g = |i: usize| arr.get(i).and_then(num).unwrap_or(0.0);
    Some([g(0), g(1), g(2), g(3), g(4), g(5)])
}

struct Walker<'a> {
    doc: &'a Document,
    out: Vec<Glyph>,
}

impl Walker<'_> {
    fn run(&mut self, data: &[u8], res: &Dictionary, base_ctm: Mat, depth: usize) -> R<()> {
        let content = Content::decode(data).map_err(err)?;
        let fonts = load_fonts(self.doc, res);
        let xforms = forms(self.doc, res);
        let mut ctm = base_ctm;
        let mut stack: Vec<Mat> = Vec::new();
        let mut tm = IDENT;
        let mut tlm = IDENT;
        let (mut tfs, mut th, mut tc, mut tw, mut tl, mut ts) = (0.0, 1.0, 0.0, 0.0, 0.0, 0.0);
        let mut cur: Option<&Font> = None;

        for op in &content.operations {
            let o = op.operator.as_str();
            match o {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(base_ctm),
                "cm" => {
                    let m = [
                        opnum(op, 0),
                        opnum(op, 1),
                        opnum(op, 2),
                        opnum(op, 3),
                        opnum(op, 4),
                        opnum(op, 5),
                    ];
                    ctm = compose(m, ctm);
                }
                "BT" => {
                    tm = IDENT;
                    tlm = IDENT;
                }
                "Tf" => {
                    cur = op
                        .operands
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .and_then(|n| fonts.get(n));
                    tfs = opnum(op, 1);
                }
                "Td" | "TD" => {
                    let (tx, ty) = (opnum(op, 0), opnum(op, 1));
                    if o == "TD" {
                        tl = -ty;
                    }
                    tlm = compose(translate(tx, ty), tlm);
                    tm = tlm;
                }
                "Tm" => {
                    tm = [
                        opnum(op, 0),
                        opnum(op, 1),
                        opnum(op, 2),
                        opnum(op, 3),
                        opnum(op, 4),
                        opnum(op, 5),
                    ];
                    tlm = tm;
                }
                "T*" => {
                    tlm = compose(translate(0.0, -tl), tlm);
                    tm = tlm;
                }
                "TL" => tl = opnum(op, 0),
                "Tc" => tc = opnum(op, 0),
                "Tw" => tw = opnum(op, 0),
                "Tz" => th = opnum(op, 0) / 100.0,
                "Ts" => ts = opnum(op, 0),
                "Tj" | "'" | "\"" | "TJ" => {
                    if o == "\"" {
                        tw = opnum(op, 0);
                        tc = opnum(op, 1);
                    }
                    if o == "'" || o == "\"" {
                        tlm = compose(translate(0.0, -tl), tlm);
                        tm = tlm;
                    }
                    let Some(font) = cur else {
                        continue;
                    };
                    let mut show = |tm: &mut Mat, bytes: &[u8]| {
                        for code in fonts::codes(bytes, font.two_byte) {
                            let m = compose(*tm, ctm);
                            let (x, y) = transform(m, (0.0, ts));
                            let (ax, ay) = transform(m, (0.0, ts + tfs));
                            let size = ((ax - x).powi(2) + (ay - y).powi(2)).sqrt();
                            let w0 = font.widths.advance(code) / 1000.0 * font.scale;
                            let sp = if !font.two_byte && code == 32 {
                                tw
                            } else {
                                0.0
                            };
                            let adv = (w0 * tfs + tc + sp) * th;
                            let (ex, ey) = transform(m, (adv, ts));
                            let w = ((ex - x).powi(2) + (ey - y).powi(2)).sqrt();
                            // Rotated text is left out of the flow.
                            if (ax - x).abs() <= (ay - y).abs() && size > 0.0 {
                                self.out.push(Glyph {
                                    x,
                                    y,
                                    w,
                                    size,
                                    text: font.text(code),
                                    bold: font.bold,
                                    italic: font.italic,
                                });
                            }
                            *tm = compose(translate(adv, 0.0), *tm);
                        }
                    };
                    if o == "TJ" {
                        if let Some(Object::Array(arr)) = op.operands.first() {
                            for el in arr {
                                match el {
                                    Object::String(bytes, _) => show(&mut tm, bytes),
                                    _ => {
                                        if let Some(n) = num(el) {
                                            let tx = -n / 1000.0 * tfs * th;
                                            tm = compose(translate(tx, 0.0), tm);
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        let idx = if o == "\"" { 2 } else { 0 };
                        if let Some(Object::String(bytes, _)) = op.operands.get(idx) {
                            show(&mut tm, bytes);
                        }
                    }
                }
                "Do" => {
                    let name = op
                        .operands
                        .first()
                        .and_then(|o| o.as_name().ok())
                        .unwrap_or(b"");
                    if let Some(form) = xforms.get(name) {
                        if depth < MAX_DEPTH {
                            let m = matrix(self.doc, &form.dict, b"Matrix").unwrap_or(IDENT);
                            let res = form
                                .dict
                                .get(b"Resources")
                                .map(|o| deref(self.doc, o))
                                .ok()
                                .and_then(|o| o.as_dict().ok())
                                .cloned()
                                .unwrap_or_else(|| res.clone());
                            if let Ok(data) = form.decompressed_content() {
                                self.run(&data, &res, compose(m, ctm), depth + 1)?;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

// Every glyph on the page, in content order, in PDF user space.
pub fn page_glyphs(doc: &Document, page_id: ObjectId) -> R<Vec<Glyph>> {
    let data = doc.get_page_content(page_id);
    let res = page_resources(doc, page_id);
    let mut w = Walker {
        doc,
        out: Vec::new(),
    };
    w.run(&data, &res, IDENT, 0)?;
    Ok(w.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bfchar_and_both_bfrange_forms() {
        let cmap = b"/CIDInit /ProcSet findresource begin
            begincodespacerange <0000> <FFFF> endcodespacerange
            2 beginbfchar <0003> <0020> <0010> <06440627> endbfchar
            2 beginbfrange <0020> <0022> <0041> <0030> <0031> [<0061> <00620063>] endbfrange
            endcmap";
        let m = parse_to_unicode(cmap);
        assert_eq!(m[&3], " ");
        assert_eq!(m[&0x10], "لا");
        assert_eq!(m[&0x20], "A");
        assert_eq!(m[&0x22], "C");
        assert_eq!(m[&0x30], "a");
        assert_eq!(m[&0x31], "bc");
    }

    #[test]
    fn glyph_names_resolve() {
        assert_eq!(glyph_name_to_text("a").as_deref(), Some("a"));
        assert_eq!(glyph_name_to_text("uni0644").as_deref(), Some("ل"));
        assert_eq!(glyph_name_to_text("u1F600").as_deref(), Some("😀"));
        assert_eq!(
            glyph_name_to_text("quoteright").as_deref(),
            Some("\u{2019}")
        );
        assert_eq!(glyph_name_to_text("eacute").as_deref(), Some("é"));
        assert_eq!(glyph_name_to_text("fi.alt").as_deref(), Some("\u{FB01}"));
        assert_eq!(glyph_name_to_text("gXYZ"), None);
    }

    #[test]
    fn walks_a_simple_page_with_positions_and_styles() {
        let doc = crate::editor::testing::sample_doc();
        let page = crate::editor::testing::page1(&doc);
        let glyphs = page_glyphs(&doc, page).unwrap();
        assert!(!glyphs.is_empty());
        let text: String = glyphs.iter().filter_map(|g| g.text.clone()).collect();
        assert!(text.contains("Hello"), "{text}");
        // Glyphs advance along the baseline.
        let xs: Vec<f64> = glyphs.iter().take(5).map(|g| g.x).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]), "{xs:?}");
        assert!(glyphs.iter().all(|g| g.size > 0.0 && g.w >= 0.0));
    }
}
