// Glyph-level text of a text page, read from the file itself. Each glyph
// keeps the whole string its font maps it to, so ligatures survive, and
// right-to-left lines are put back into logical order from the glyphs'
// positions rather than from a viewer's guess.

use super::glyphs;
use super::{LineIn, WordIn};
use crate::common::R;

#[derive(Debug, Clone)]
struct Glyph {
    x: f64,
    // Baseline, PDF points, origin bottom-left of the page.
    y: f64,
    w: f64,
    size: f64,
    // None when the font gives no way to name the glyph.
    text: Option<String>,
    bold: bool,
    italic: bool,
}

// Baselines closer than this share of the size are one line.
const BASELINE_TOL: f64 = 0.3;
// A gap wider than this many sizes starts a new line (another column).
const LINE_GAP: f64 = 2.0;
// A gap wider than this share of the size separates words.
const WORD_GAP: f64 = 0.2;

fn is_arabic(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

fn is_rtl_text(s: &str) -> bool {
    let rtl = s.chars().filter(|c| is_arabic(*c)).count();
    let ltr = s
        .chars()
        .filter(|c| c.is_alphabetic() && !is_arabic(*c))
        .count();
    rtl > 0 && rtl >= ltr
}

fn txt(g: &Glyph) -> &str {
    g.text.as_deref().unwrap_or("").trim()
}

// Combining marks never start a word, whatever the spacing says.
fn is_mark(g: &Glyph) -> bool {
    let t = txt(g);
    !t.is_empty()
        && t.chars()
            .all(|c| unicode_bidi::bidi_class(c) == unicode_bidi::BidiClass::NSM)
}

// Brackets are shown mirrored in right-to-left runs; logical text wants
// the original character back.
fn unmirror(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '(' => ')',
            ')' => '(',
            '[' => ']',
            ']' => '[',
            '{' => '}',
            '}' => '{',
            '<' => '>',
            '>' => '<',
            '«' => '»',
            '»' => '«',
            c => c,
        })
        .collect()
}

// Whether a glyph belongs to a left-to-right run: Latin letters and digits,
// plus the separators that sit inside numbers ("1,710.00", "3/4") and a
// percent sign after a digit.
fn ltr_class(glyphs: &[&Glyph], i: usize) -> bool {
    let strong = |g: &Glyph| {
        txt(g)
            .chars()
            .any(|c| c.is_ascii_alphanumeric() || (c.is_alphabetic() && !is_arabic(c)))
    };
    if strong(glyphs[i]) {
        return true;
    }
    let t = txt(glyphs[i]);
    let inner_sep = !t.is_empty()
        && t.chars()
            .all(|c| matches!(c, ',' | '.' | '/' | '\'' | '-' | '_' | '@' | '\u{2013}'));
    (inner_sep && i > 0 && i + 1 < glyphs.len() && strong(glyphs[i - 1]) && strong(glyphs[i + 1]))
        || (t == "%" && i > 0 && strong(glyphs[i - 1]))
}

// Logical text of glyphs given in visual (left-to-right) order. A
// right-to-left line reads from the right, except that left-to-right
// runs (numbers, Latin words) keep their own order.
fn logical_text(glyphs: &[&Glyph], rtl: bool) -> String {
    if !rtl {
        return glyphs.iter().map(|g| txt(g)).collect();
    }
    // Fonts draw the fathatan of "ـاءً" over the alef, so by position it
    // lands before the hamza; in writing it follows it.
    reversed(glyphs).replace("اًء", "اءً")
}

fn reversed(glyphs: &[&Glyph]) -> String {
    let mut out = String::new();
    let mut i = glyphs.len();
    while i > 0 {
        i -= 1;
        if ltr_class(glyphs, i) {
            let end = i + 1;
            let mut start = i;
            while start > 0 && ltr_class(glyphs, start - 1) {
                start -= 1;
            }
            for g in &glyphs[start..end] {
                out.push_str(txt(g));
            }
            i = start;
        } else {
            out.push_str(&unmirror(txt(glyphs[i])));
        }
    }
    out
}

// A line from glyphs sharing a baseline, in visual order. Lines with a
// glyph the font cannot name come back without text.
fn line_from(run: &[&Glyph]) -> Option<LineIn> {
    if run.is_empty() {
        return None;
    }
    let size = run.iter().map(|g| g.size).fold(0.0, f64::max);
    let base = run.iter().map(|g| g.y).sum::<f64>() / run.len() as f64;
    let bbox_of = |gs: &[&Glyph]| {
        let x0 = gs.iter().map(|g| g.x).fold(f64::MAX, f64::min);
        let x1 = gs.iter().map(|g| g.x + g.w).fold(f64::MIN, f64::max);
        [x0, base - 0.25 * size, x1, base + 0.75 * size]
    };
    let weight = |pick: fn(&Glyph) -> bool| {
        run.iter().filter(|g| pick(g)).map(|g| g.w).sum::<f64>() * 2.0
            > run.iter().map(|g| g.w).sum::<f64>()
    };
    if run.iter().any(|g| g.text.is_none()) {
        return Some(LineIn {
            text: String::new(),
            bbox: bbox_of(run),
            size,
            bold: weight(|g| g.bold),
            italic: weight(|g| g.italic),
            words: Vec::new(),
        });
    }
    // Words: split at blank glyphs, at spaces carried by a glyph, and at
    // positioned gaps.
    let mut words: Vec<Vec<&Glyph>> = Vec::new();
    let mut cur: Vec<&Glyph> = Vec::new();
    let mut last_end = f64::MIN;
    for g in run {
        let raw = g.text.as_deref().unwrap_or("");
        let blank = raw.trim().is_empty();
        let gap = g.x - last_end;
        let breaks = blank
            || ((gap > WORD_GAP * g.size || raw.starts_with(char::is_whitespace)) && !is_mark(g));
        if breaks && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        if !blank {
            cur.push(g);
            if raw.ends_with(char::is_whitespace) {
                words.push(std::mem::take(&mut cur));
            }
        }
        // A zero-width mark hangs on the letter before it and does not
        // move the pen.
        if !is_mark(g) || g.w > 0.0 {
            last_end = g.x + g.w;
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    if words.is_empty() {
        return None;
    }
    let all_text: String = run.iter().map(|g| txt(g)).collect();
    let rtl = is_rtl_text(&all_text);
    let mut word_items: Vec<WordIn> = words
        .iter()
        .map(|w| WordIn {
            text: logical_text(w, rtl),
            bbox: bbox_of(w),
        })
        .collect();
    if rtl {
        word_items.reverse();
    }
    let text = word_items
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let inked: Vec<&Glyph> = run.iter().copied().filter(|g| !txt(g).is_empty()).collect();
    Some(LineIn {
        text,
        bbox: bbox_of(&inked),
        size,
        bold: weight(|g| g.bold),
        italic: weight(|g| g.italic),
        words: word_items,
    })
}

fn lines_from(mut glyphs: Vec<Glyph>) -> Vec<LineIn> {
    glyphs.retain(|g| g.size > 0.0 && g.x.is_finite() && g.y.is_finite());
    glyphs.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap()
            .then(a.x.partial_cmp(&b.x).unwrap())
    });
    // Baseline bands.
    let mut bands: Vec<Vec<Glyph>> = Vec::new();
    for g in glyphs {
        match bands
            .iter_mut()
            .find(|b| (b[0].y - g.y).abs() <= BASELINE_TOL * b[0].size.min(g.size))
        {
            Some(b) => b.push(g),
            None => bands.push(vec![g]),
        }
    }
    let mut out = Vec::new();
    for band in &mut bands {
        band.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
        let mut run: Vec<&Glyph> = Vec::new();
        for g in band.iter() {
            if let Some(last) = run.last() {
                if g.x - (last.x + last.w) > LINE_GAP * last.size.min(g.size) {
                    out.extend(line_from(&run));
                    run.clear();
                }
            }
            run.push(g);
        }
        out.extend(line_from(&run));
    }
    out
}

// Reads the glyphs of `page` (1-based) straight from the file. Lines
// holding a glyph the font cannot name are returned without text, so the
// caller can take the viewer's reading for them.
pub fn extract_lines(src: &str, page: u32) -> R<Vec<LineIn>> {
    let doc = crate::common::load_plain(src)?;
    let pid = *doc
        .get_pages()
        .get(&page)
        .ok_or_else(|| format!("page {page} not found"))?;
    let (ox, oy) = media_origin(&doc, pid);
    let glyphs = glyphs::page_glyphs(&doc, pid)?
        .into_iter()
        .map(|g| Glyph {
            x: g.x - ox,
            y: g.y - oy,
            w: g.w,
            size: g.size,
            text: g.text,
            bold: g.bold,
            italic: g.italic,
        })
        .collect();
    Ok(lines_from(glyphs))
}

// Pictures a text page draws: image XObjects, and clusters of vector
// paths big enough to be a drawing. Thin strokes (rules, borders), page
// backgrounds and shaded text (a highlighted row, a framed paragraph) are
// not pictures.
pub fn figure_boxes(src: &str, page: u32, lines: &[LineIn]) -> R<Vec<[f64; 4]>> {
    use crate::editor::{enumerate, Kind};
    let doc = crate::common::load_plain(src)?;
    let pid = *doc
        .get_pages()
        .get(&page)
        .ok_or_else(|| format!("page {page} not found"))?;
    let (ox, oy) = media_origin(&doc, pid);
    let page_area = {
        let (w, h) = page_size(&doc, pid);
        w * h
    };
    let objs = enumerate(&doc, pid)?;
    let shift = |b: [f64; 4]| [b[0] - ox, b[1] - oy, b[2] - ox, b[3] - oy];
    let area = |b: &[f64; 4]| (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let mut images: Vec<[f64; 4]> = Vec::new();
    let mut paths: Vec<[f64; 4]> = Vec::new();
    for o in &objs {
        let b = shift(o.bbox);
        let (w, h) = (b[2] - b[0], b[3] - b[1]);
        match o.kind {
            Kind::Image if w >= 4.0 && h >= 4.0 => images.push(b),
            Kind::Path if w >= 2.0 && h >= 2.0 && area(&b) < 0.8 * page_area => paths.push(b),
            _ => {}
        }
    }
    // Paths that touch or nearly touch form one drawing.
    let near = |a: &[f64; 4], b: &[f64; 4]| {
        a[0] <= b[2] + 3.0 && b[0] <= a[2] + 3.0 && a[1] <= b[3] + 3.0 && b[1] <= a[3] + 3.0
    };
    let mut clusters: Vec<[f64; 4]> = Vec::new();
    for p in paths {
        let mut cur = p;
        while let Some(i) = clusters.iter().position(|c| near(c, &cur)) {
            let c = clusters.swap_remove(i);
            cur = [
                c[0].min(cur[0]),
                c[1].min(cur[1]),
                c[2].max(cur[2]),
                c[3].max(cur[3]),
            ];
        }
        clusters.push(cur);
    }
    let mut out = images;
    for c in clusters {
        if area(&c) < 400.0 {
            continue;
        }
        let inside: Vec<&LineIn> = lines
            .iter()
            .filter(|l| {
                let cx = (l.bbox[0] + l.bbox[2]) / 2.0;
                let cy = (l.bbox[1] + l.bbox[3]) / 2.0;
                cx >= c[0] && cx <= c[2] && cy >= c[1] && cy <= c[3]
            })
            .collect();
        let covered: f64 = inside.iter().map(|l| area(&l.bbox)).sum::<f64>() / area(&c);
        let tallest = inside
            .iter()
            .map(|l| l.bbox[3] - l.bbox[1])
            .fold(0.0, f64::max);
        let shaded_text = !inside.is_empty() && (c[3] - c[1]) < 2.5 * tallest;
        let framed_text = covered >= 0.3 || (inside.len() >= 3 && covered >= 0.15);
        if !shaded_text && !framed_text {
            out.push(c);
        }
    }
    Ok(out)
}

fn page_size(doc: &lopdf::Document, pid: lopdf::ObjectId) -> (f64, f64) {
    let mut cur = Some(pid);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else {
            break;
        };
        if let Ok(arr) = dict
            .get(b"MediaBox")
            .map(|o| crate::editor::deref(doc, o))
            .and_then(|o| o.as_array())
        {
            let g = |i: usize| arr.get(i).and_then(crate::editor::num).unwrap_or(0.0);
            return ((g(2) - g(0)).abs(), (g(3) - g(1)).abs());
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    (612.0, 792.0)
}

// Lower-left corner of the page box, so coordinates start at 0.
fn media_origin(doc: &lopdf::Document, pid: lopdf::ObjectId) -> (f64, f64) {
    let mut cur = Some(pid);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else {
            break;
        };
        if let Ok(arr) = dict
            .get(b"MediaBox")
            .map(|o| crate::editor::deref(doc, o))
            .and_then(|o| o.as_array())
        {
            let g = |i: usize| arr.get(i).and_then(crate::editor::num).unwrap_or(0.0);
            return (g(0).min(g(2)), g(1).min(g(3)));
        }
        cur = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    (0.0, 0.0)
}

fn overlap(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    let w = a[2].min(b[2]) - a[0].max(b[0]);
    let h = a[3].min(b[3]) - a[1].max(b[1]);
    if w <= 0.0 || h <= 0.0 {
        0.0
    } else {
        w * h
    }
}

// Lines the file could not name take the viewer's reading at the same
// place; viewer lines the walk never saw (text in widgets, odd fonts) are
// added as they are.
pub fn merge_with_viewer(mine: Vec<LineIn>, viewer: &[LineIn]) -> Vec<LineIn> {
    let mut used = vec![false; viewer.len()];
    let mut out: Vec<LineIn> = Vec::new();
    for l in mine {
        let best = viewer
            .iter()
            .enumerate()
            .map(|(i, v)| (overlap(&l.bbox, &v.bbox), i))
            .filter(|(o, _)| *o > 0.0)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        if let Some((_, i)) = best {
            used[i] = true;
            if l.text.is_empty() {
                out.push(viewer[i].clone());
                continue;
            }
        }
        if !l.text.is_empty() {
            out.push(l);
        }
    }
    for (i, v) in viewer.iter().enumerate() {
        if !used[i] && !v.text.trim().is_empty() {
            let seen = out.iter().any(|l| overlap(&l.bbox, &v.bbox) > 0.0);
            if !seen {
                out.push(v.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(text: &str, x: f64, y: f64, w: f64) -> Glyph {
        Glyph {
            x,
            y,
            w,
            size: 10.0,
            text: Some(text.into()),
            bold: false,
            italic: false,
        }
    }

    // Lays glyphs out left to right from `x`, returning them in content order.
    fn row(y: f64, x: f64, parts: &[&str]) -> Vec<Glyph> {
        let mut out = Vec::new();
        let mut pos = x;
        for p in parts {
            out.push(g(p, pos, y, 6.0));
            pos += 6.0;
        }
        out
    }

    #[test]
    fn right_to_left_lines_read_from_the_right_with_numbers_intact() {
        // Visual order of "التاريخ: 14 مارس 2024": 2024 | مارس | 14 | :التاريخ
        let mut glyphs = row(500.0, 100.0, &["2", "0", "2", "4", " "]);
        glyphs.extend(row(500.0, 130.0, &["س", "ر", "ا", "م", " "]));
        glyphs.extend(row(500.0, 160.0, &["1", "4", " "]));
        glyphs.extend(row(500.0, 178.0, &[":", "خ", "ي", "ر", "ا", "ت", "ل", "ا"]));
        let lines = lines_from(glyphs);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "التاريخ: 14 مارس 2024");
        assert_eq!(lines[0].words[0].text, "التاريخ:");
        assert_eq!(lines[0].words[3].text, "2024");
    }

    #[test]
    fn ligature_glyphs_keep_their_characters_together() {
        // "خلال" with a lam-alef ligature glyph, visual: ل | لا | خ
        let glyphs = row(300.0, 50.0, &["ل", "لا", "خ"]);
        assert_eq!(lines_from(glyphs)[0].text, "خلال");
    }

    #[test]
    fn marks_stay_with_their_word_and_brackets_unmirror() {
        // Visual "(ريال)" as a viewer shows it from the left: the closing
        // bracket comes first and both are drawn mirrored.
        let mut glyphs = row(200.0, 10.0, &["(", "ل", "ا", "ي", "ر", ")"]);
        // A shadda drawn as a zero-width glyph before the next letter.
        glyphs.push(g("ة", 60.0, 200.0, 5.0));
        glyphs.push(g("\u{651}", 63.0, 200.0, 0.0));
        glyphs.push(g("ل", 65.0, 200.0, 5.0));
        let lines = lines_from(glyphs);
        assert_eq!(lines[0].words[1].text, "(ريال)");
        assert_eq!(lines[0].words[0].text, "ل\u{651}ة");
        // A fathatan drawn over the alef of "بناءً" belongs after the hamza.
        let word = row(400.0, 10.0, &["ء", "\u{64b}", "ا", "ن", "ب"]);
        assert_eq!(lines_from(word)[0].text, "بناءً");
        // A hyphenated code inside an Arabic line stays whole.
        let mut code = row(300.0, 10.0, &["I", "N", "V", "-", "2", "0", "-", "7", " "]);
        code.extend(row(300.0, 80.0, &["م", "ق", "ر"]));
        assert_eq!(lines_from(code)[0].text, "رقم INV-20-7");
    }

    #[test]
    fn mixed_number_and_colon_inside_arabic() {
        // Logical "ضريبة 15%: 1,710.00 ريال"; visual: ريال | 1,710.00 | :15% | ضريبة
        let mut glyphs = row(200.0, 10.0, &["ل", "ا", "ي", "ر", " "]);
        glyphs.extend(row(
            200.0,
            40.0,
            &["1", ",", "7", "1", "0", ".", "0", "0", " "],
        ));
        glyphs.extend(row(200.0, 94.0, &[":", "1", "5", "%", " "]));
        glyphs.extend(row(200.0, 124.0, &["ة", "ب", "ي", "ر", "ض"]));
        assert_eq!(lines_from(glyphs)[0].text, "ضريبة 15%: 1,710.00 ريال");
    }

    #[test]
    fn left_to_right_lines_split_at_wide_gaps_and_words_at_spaces() {
        let mut glyphs = row(100.0, 0.0, &["H", "i", " ", "y", "o", "u"]);
        glyphs.extend(row(100.0, 200.0, &["f", "a", "r"]));
        let lines = lines_from(glyphs);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Hi you");
        assert_eq!(lines[0].words.len(), 2);
        assert_eq!(lines[1].text, "far");
    }

    #[test]
    fn unreadable_lines_take_the_viewer_reading() {
        let mut glyphs = row(100.0, 0.0, &["a", "b"]);
        glyphs[1].text = None;
        let mine = lines_from(glyphs);
        assert_eq!(mine[0].text, "");
        let viewer = vec![
            LineIn {
                text: "ab".into(),
                bbox: [0.0, 96.0, 12.0, 108.0],
                size: 10.0,
                bold: true,
                italic: false,
                words: vec![],
            },
            LineIn {
                text: "elsewhere".into(),
                bbox: [300.0, 500.0, 340.0, 510.0],
                size: 10.0,
                bold: false,
                italic: false,
                words: vec![],
            },
        ];
        let merged = merge_with_viewer(mine, &viewer);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].text, "ab");
        assert_eq!(merged[1].text, "elsewhere");
    }

    #[test]
    fn spaces_carried_by_glyphs_split_words() {
        // The composer folds a space into the glyph before it.
        let glyphs = vec![g("a ", 0.0, 100.0, 6.0), g("b", 6.0, 100.0, 6.0)];
        let lines = lines_from(glyphs);
        assert_eq!(lines[0].text, "a b");
        assert_eq!(lines[0].words.len(), 2);
    }
}
