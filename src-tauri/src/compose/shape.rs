// Turns one line of text into positioned glyphs: bidi reordering, font
// choice per script, OpenType shaping and the text each glyph stands for.
use lopdf::Document;
use rustybuzz::{Direction, UnicodeBuffer};
use serde::Deserialize;
use std::ops::Range;
use unicode_bidi::{bidi_class, BidiClass, Level, ParagraphBidiInfo};
use unicode_script::{Script, UnicodeScript};

use super::fonts::{Face, Family, FontKey, FontUse, Fonts};
use crate::common::R;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    Auto,
    Rtl,
    Ltr,
}

// All distances are in thousandths of the em, like PDF glyph metrics.
pub struct Glyph {
    pub font: FontKey,
    pub gid: u16,
    pub text: String,
    pub advance: f64,
    pub dx: f64,
    pub dy: f64,
    pub ink: Option<(f64, f64)>,
    pub space: bool,
    // Shaped as part of a right-to-left run.
    pub rtl: bool,
}

pub struct Line {
    pub glyphs: Vec<Glyph>,
    pub ascent: f64,
    pub descent: f64,
}

impl Line {
    pub fn width(&self) -> f64 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    pub fn spaces(&self) -> usize {
        self.glyphs.iter().filter(|g| g.space).count()
    }

    // Vertical extent of the ink, relative to the baseline.
    pub fn ink(&self) -> Option<(f64, f64)> {
        self.glyphs
            .iter()
            .filter_map(|g| g.ink.map(|(lo, hi)| (lo + g.dy, hi + g.dy)))
            .reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
    }
}

pub fn shape_line(
    fonts: &mut Fonts,
    doc: &mut Document,
    text: &str,
    bold: bool,
    dir: Dir,
) -> R<Line> {
    let level = match dir {
        Dir::Auto => None,
        Dir::Rtl => Some(Level::rtl()),
        Dir::Ltr => Some(Level::ltr()),
    };
    let bidi = ParagraphBidiInfo::new(text, level);
    let mut glyphs = Vec::new();
    if !text.is_empty() {
        let (levels, runs) = bidi.visual_runs(0..text.len());
        for run in runs {
            let run_rtl = levels[run.start].is_rtl();
            let mut segments = itemize(fonts, &text[run.clone()], run_rtl)?;
            if run_rtl {
                segments.reverse();
            }
            for (range, family) in segments {
                let seg = &text[run.start + range.start..run.start + range.end];
                let font = fonts.get(doc, FontKey { family, bold })?;
                shape_segment(font, seg, run_rtl, &mut glyphs);
            }
        }
    }
    let key = glyphs.first().map(|g| g.font).unwrap_or(FontKey {
        family: Family::Sans,
        bold,
    });
    let face = &fonts.get(doc, key)?.face;
    Ok(Line {
        glyphs,
        ascent: face.ascent(),
        descent: face.descent(),
    })
}

fn family_of(c: char) -> Option<Family> {
    match c.script() {
        Script::Arabic => Some(Family::Naskh),
        Script::Common | Script::Inherited => None,
        _ => Some(Family::Sans),
    }
}

fn other(f: Family) -> Family {
    match f {
        Family::Sans => Family::Naskh,
        Family::Naskh => Family::Sans,
    }
}

// Splits a run into maximal stretches drawn from one family. Characters
// of no particular script take the family of their neighbours; a character
// its family cannot draw falls back to the other one.
fn itemize(fonts: &mut Fonts, run: &str, rtl: bool) -> R<Vec<(Range<usize>, Family)>> {
    let mut chars: Vec<(Range<usize>, Option<Family>)> = Vec::new();
    for (i, c) in run.char_indices() {
        let mut fam = family_of(c);
        if let Some(f) = fam {
            if !fonts.face(f)?.has(c) && fonts.face(other(f))?.has(c) {
                fam = Some(other(f));
            }
        }
        chars.push((i..i + c.len_utf8(), fam));
    }
    let default = chars.iter().find_map(|(_, f)| *f).unwrap_or(if rtl {
        Family::Naskh
    } else {
        Family::Sans
    });
    let mut out: Vec<(Range<usize>, Family)> = Vec::new();
    let mut current = default;
    for (range, fam) in chars {
        let fam = fam.unwrap_or(current);
        match out.last_mut() {
            Some((r, f)) if *f == fam => r.end = range.end,
            _ => out.push((range, fam)),
        }
        current = fam;
    }
    Ok(out)
}

fn shape_segment(font: &FontUse, seg: &str, rtl: bool, out: &mut Vec<Glyph>) {
    let mut buf = UnicodeBuffer::new();
    buf.push_str(seg);
    buf.guess_segment_properties();
    buf.set_direction(if rtl {
        Direction::RightToLeft
    } else {
        Direction::LeftToRight
    });
    let shaped = rustybuzz::shape(&font.face.hb, &[], buf);
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    let mut bounds: Vec<usize> = infos.iter().map(|i| i.cluster as usize).collect();
    bounds.sort_unstable();
    bounds.dedup();
    let mut i = 0;
    while i < infos.len() {
        let cluster = infos[i].cluster;
        let j = i + infos[i..]
            .iter()
            .take_while(|g| g.cluster == cluster)
            .count();
        let start = cluster as usize;
        let end = bounds
            .iter()
            .find(|&&b| b > start)
            .copied()
            .unwrap_or(seg.len());
        let chars = &seg[start..end];
        let gids: Vec<u16> = infos[i..j].iter().map(|g| g.glyph_id as u16).collect();
        let texts = split_cluster(&font.face, &gids, chars, rtl);
        for (k, (gid, text)) in gids.iter().zip(texts).enumerate() {
            let p = positions[i + k];
            out.push(Glyph {
                font: font.key,
                gid: *gid,
                text,
                advance: font.face.em(p.x_advance as f64),
                dx: font.face.em(p.x_offset as f64),
                dy: font.face.em(p.y_offset as f64),
                ink: font.face.ink(*gid),
                space: chars.chars().all(char::is_whitespace),
                rtl,
            });
        }
        i = j;
    }
}

fn is_mark_char(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

// Distributes a cluster's characters over its glyphs in stream order:
// base characters go to base glyphs (all to the first when counts differ,
// as for ligatures); combining marks go to the mark glyphs the cmap ties
// them to, then to the remaining mark glyphs, then onto the last glyph
// with text. Glyphs left without text, such as the Naskh dots, are drawn
// as outlines instead. Right-to-left streams run backwards through the
// logical text, so their multi-character texts are reversed to match.
fn split_cluster(face: &Face, gids: &[u16], chars: &str, rtl: bool) -> Vec<String> {
    let mut texts = vec![String::new(); gids.len()];
    if gids.is_empty() {
        return texts;
    }
    let (bases, marks): (Vec<usize>, Vec<usize>) =
        (0..gids.len()).partition(|&i| !face.is_mark(gids[i]));
    let (mark_chars, base_chars): (Vec<char>, Vec<char>) =
        chars.chars().partition(|&c| is_mark_char(c));
    let stream = |v: Vec<usize>| {
        if rtl {
            v.into_iter().rev().collect()
        } else {
            v
        }
    };
    if bases.len() == base_chars.len() {
        for (i, c) in stream(bases.clone()).into_iter().zip(base_chars) {
            texts[i].push(c);
        }
    } else {
        texts[bases.first().copied().unwrap_or(0)].extend(base_chars);
    }
    let mut pending: Vec<Option<char>> = mark_chars.into_iter().map(Some).collect();
    let mut free = Vec::new();
    for &i in &marks {
        match face
            .char_of(gids[i])
            .and_then(|c| pending.iter().position(|p| *p == Some(c)))
        {
            Some(k) => texts[i].push(pending[k].take().expect("pending mark")),
            None => free.push(i),
        }
    }
    let mut free = stream(free).into_iter();
    for c in pending.into_iter().flatten() {
        let target = free
            .next()
            .or_else(|| (0..gids.len()).rev().find(|&i| !texts[i].is_empty()))
            .unwrap_or(0);
        texts[target].push(c);
    }
    if rtl {
        for t in &mut texts {
            *t = t.chars().rev().collect();
        }
    }
    texts
}
