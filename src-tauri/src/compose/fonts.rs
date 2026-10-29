// Embedded Noto faces and their PDF embedding as subset CIDFontType2 fonts
// with Identity-H encoding and a ToUnicode CMap.
//
// A CID stands for one (glyph, text, advance) combination rather than a
// bare glyph: the Naskh face draws several letters with one shared base
// glyph plus dot marks, so the text a glyph means depends on its cluster,
// and an advance baked into the width keeps the pen from ever jumping,
// which text extractors read as word breaks.
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use rustybuzz::ttf_parser::GlyphId;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use subsetter::GlyphRemapper;

use crate::common::{deflate, R};

static SANS: &[u8] = include_bytes!("../../fonts/NotoSans-Regular.ttf");
static SANS_BOLD: &[u8] = include_bytes!("../../fonts/NotoSans-Bold.ttf");
static NASKH: &[u8] = include_bytes!("../../fonts/NotoNaskhArabic-Regular.ttf");
static NASKH_BOLD: &[u8] = include_bytes!("../../fonts/NotoNaskhArabic-Bold.ttf");

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Family {
    Sans,
    Naskh,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct FontKey {
    pub family: Family,
    pub bold: bool,
}

impl FontKey {
    fn data(self) -> &'static [u8] {
        match (self.family, self.bold) {
            (Family::Sans, false) => SANS,
            (Family::Sans, true) => SANS_BOLD,
            (Family::Naskh, false) => NASKH,
            (Family::Naskh, true) => NASKH_BOLD,
        }
    }

    fn name(self) -> &'static str {
        match (self.family, self.bold) {
            (Family::Sans, false) => "NotoSans-Regular",
            (Family::Sans, true) => "NotoSans-Bold",
            (Family::Naskh, false) => "NotoNaskhArabic-Regular",
            (Family::Naskh, true) => "NotoNaskhArabic-Bold",
        }
    }
}

pub struct Face {
    pub hb: rustybuzz::Face<'static>,
    chars: HashMap<u16, char>,
}

impl Face {
    fn load(key: FontKey) -> R<Face> {
        let hb = rustybuzz::Face::from_slice(key.data(), 0)
            .ok_or_else(|| format!("cannot parse font {}", key.name()))?;
        let chars = reverse_cmap(&hb);
        Ok(Face { hb, chars })
    }

    // Font units to thousandths of the em, the unit PDF glyph metrics use.
    pub fn em(&self, v: f64) -> f64 {
        v * 1000.0 / self.hb.units_per_em() as f64
    }

    pub fn has(&self, c: char) -> bool {
        self.hb.glyph_index(c).is_some()
    }

    // The character the cmap maps straight to this glyph, if any.
    pub fn char_of(&self, gid: u16) -> Option<char> {
        self.chars.get(&gid).copied()
    }

    pub fn is_mark(&self, gid: u16) -> bool {
        use rustybuzz::ttf_parser::gdef::GlyphClass;
        self.hb
            .tables()
            .gdef
            .and_then(|g| g.glyph_class(GlyphId(gid)))
            == Some(GlyphClass::Mark)
    }

    // Vertical ink extent of a glyph, in em thousandths.
    pub fn ink(&self, gid: u16) -> Option<(f64, f64)> {
        let r = self.hb.glyph_bounding_box(GlyphId(gid))?;
        Some((self.em(r.y_min as f64), self.em(r.y_max as f64)))
    }

    pub fn ascent(&self) -> f64 {
        self.em(self.hb.ascender() as f64)
    }

    pub fn descent(&self) -> f64 {
        self.em(self.hb.descender() as f64)
    }
}

fn reverse_cmap(face: &rustybuzz::Face<'static>) -> HashMap<u16, char> {
    let mut map = HashMap::new();
    let Some(cmap) = face.tables().cmap else {
        return map;
    };
    for sub in cmap.subtables.into_iter().filter(|s| s.is_unicode()) {
        sub.codepoints(|cp| {
            if let (Some(c), Some(g)) = (char::from_u32(cp), sub.glyph_index(cp)) {
                map.entry(g.0).or_insert(c);
            }
        });
    }
    map
}

struct Cid {
    gid: u16,
    width: f64,
    text: String,
}

// One embedded font and the CIDs handed out for it so far.
pub struct FontUse {
    pub key: FontKey,
    pub face: Face,
    pub id: ObjectId,
    pub name: String,
    remap: GlyphRemapper,
    cids: Vec<Cid>,
    index: HashMap<(u16, String, i32), u16>,
}

impl FontUse {
    // The CID for drawing `gid` as `text` with the pen moving `width`
    // (em thousandths) afterwards.
    pub fn cid(&mut self, gid: u16, text: &str, width: f64) -> R<u16> {
        let tenths = (width * 10.0).round() as i32;
        let key = (gid, text.to_string(), tenths);
        if let Some(&cid) = self.index.get(&key) {
            return Ok(cid);
        }
        let cid =
            u16::try_from(self.cids.len()).map_err(|_| "too many distinct glyphs in one font")?;
        let gid = self.remap.remap(gid);
        self.cids.push(Cid {
            gid,
            width: tenths as f64 / 10.0,
            text: text.to_string(),
        });
        self.index.insert(key, cid);
        Ok(cid)
    }

    pub fn width(&self, cid: u16) -> f64 {
        self.cids.get(cid as usize).map(|c| c.width).unwrap_or(0.0)
    }
}

#[derive(Default)]
pub struct Fonts {
    fonts: HashMap<FontKey, FontUse>,
    faces: HashMap<Family, Face>,
}

impl Fonts {
    // A face for coverage checks only; it reserves nothing in the document.
    pub fn face(&mut self, family: Family) -> R<&Face> {
        match self.faces.entry(family) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(v) => Ok(v.insert(Face::load(FontKey {
                family,
                bold: false,
            })?)),
        }
    }

    pub fn get(&mut self, doc: &mut Document, key: FontKey) -> R<&mut FontUse> {
        let name = format!("F{}", self.fonts.len() + 1);
        match self.fonts.entry(key) {
            Entry::Occupied(e) => Ok(e.into_mut()),
            Entry::Vacant(v) => Ok(v.insert(FontUse {
                key,
                face: Face::load(key)?,
                id: doc.new_object_id(),
                name,
                remap: GlyphRemapper::new(),
                cids: vec![Cid {
                    gid: 0,
                    width: 0.0,
                    text: String::new(),
                }],
                index: HashMap::new(),
            })),
        }
    }

    pub fn resource_entries(&self) -> impl Iterator<Item = (&str, ObjectId)> {
        self.fonts.values().map(|f| (f.name.as_str(), f.id))
    }

    // Writes every font used so far into the document at its reserved id.
    pub fn embed(&self, doc: &mut Document) -> R<()> {
        for font in self.fonts.values() {
            embed_font(doc, font)?;
        }
        Ok(())
    }
}

fn embed_font(doc: &mut Document, font: &FontUse) -> R<()> {
    let subset = subsetter::subset(font.key.data(), 0, &font.remap)
        .map_err(|e| format!("cannot subset {}: {e}", font.key.name()))?;
    let base_font = format!("{}+{}", subset_tag(&font.cids), font.key.name());
    let file_id = doc.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode", "Length1" => subset.len() as i64 },
        deflate(&subset)?,
    ));
    let descriptor_id = doc.add_object(descriptor(&font.face, &base_font, file_id));
    let cid_to_gid: Vec<u8> = font.cids.iter().flat_map(|c| c.gid.to_be_bytes()).collect();
    let cid_to_gid_id = doc.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        deflate(&cid_to_gid)?,
    ));
    let widths: Vec<Object> = font.cids.iter().map(|c| c.width.into()).collect();
    let cid_font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "BaseFont" => base_font.as_str(),
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"),
            "Supplement" => 0,
        },
        "FontDescriptor" => descriptor_id,
        "DW" => 0,
        "W" => vec![0.into(), Object::Array(widths)],
        "CIDToGIDMap" => cid_to_gid_id,
    });
    let to_unicode_id = doc.add_object(Stream::new(
        dictionary! { "Filter" => "FlateDecode" },
        deflate(to_unicode_cmap(&font.cids).as_bytes())?,
    ));
    doc.objects.insert(
        font.id,
        Object::Dictionary(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => base_font.as_str(),
            "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(cid_font_id)],
            "ToUnicode" => to_unicode_id,
        }),
    );
    Ok(())
}

fn descriptor(face: &Face, base_font: &str, file_id: ObjectId) -> lopdf::Dictionary {
    let bbox = face.hb.global_bounding_box();
    let cap = face
        .hb
        .capital_height()
        .map(|v| face.em(v as f64))
        .unwrap_or(700.0);
    dictionary! {
        "Type" => "FontDescriptor",
        "FontName" => base_font,
        "Flags" => 4,
        "FontBBox" => vec![
            face.em(bbox.x_min as f64).into(),
            face.em(bbox.y_min as f64).into(),
            face.em(bbox.x_max as f64).into(),
            face.em(bbox.y_max as f64).into(),
        ],
        "ItalicAngle" => 0,
        "Ascent" => face.ascent(),
        "Descent" => face.descent(),
        "CapHeight" => cap,
        "StemV" => 80,
        "FontFile2" => file_id,
    }
}

fn subset_tag(cids: &[Cid]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in cids {
        h = (h ^ (c.gid as u64 ^ ((c.width * 10.0) as u64) << 16)).wrapping_mul(0x100000001b3);
    }
    (0..6)
        .map(|i| (b'A' + ((h >> (i * 5)) % 26) as u8) as char)
        .collect()
}

fn to_unicode_cmap(cids: &[Cid]) -> String {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(usize, &str)> = cids
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.text.is_empty())
        .map(|(i, c)| (i, c.text.as_str()))
        .collect();
    for chunk in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (cid, text) in chunk {
            let utf16: String = text.encode_utf16().map(|u| format!("{u:04X}")).collect();
            out.push_str(&format!("<{cid:04X}> <{utf16}>\n"));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out
}
