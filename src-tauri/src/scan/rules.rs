// Ruling lines (table borders, underlines, separators) from the ink mask.
use image::GrayImage;

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub width: f64,
}

impl Rule {
    pub fn is_horizontal(&self) -> bool {
        (self.x2 - self.x1).abs() >= (self.y2 - self.y1).abs()
    }

    pub fn length(&self) -> f64 {
        if self.is_horizontal() {
            self.x2 - self.x1
        } else {
            self.y2 - self.y1
        }
    }

    pub fn bbox(&self) -> [f64; 4] {
        let half = (self.width / 2.0).max(0.5);
        if self.is_horizontal() {
            [self.x1, self.y1 - half, self.x2, self.y2 + half]
        } else {
            [self.x1 - half, self.y1, self.x2 + half, self.y2]
        }
    }
}

// Minimum run length as a share of the page dimension.
const MIN_LEN: f64 = 0.015;
// A rule must be at least this many times longer than it is thick.
const MIN_ASPECT: f64 = 8.0;
// Breaks in a run shorter than this are bridged.
const MAX_GAP: u32 = 2;
// Parallel rules this close (beyond their thickness) are one rule.
const MERGE_SLACK: f64 = 2.0;

// Dark runs along one scanline, as (start, end) inclusive-exclusive.
fn runs(line: impl Iterator<Item = bool>, min_len: u32) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let mut start: Option<u32> = None;
    let mut last_ink = 0u32;
    for (i, ink) in line.enumerate() {
        let i = i as u32;
        if ink {
            if start.is_none() {
                start = Some(i);
            }
            last_ink = i;
        } else if let Some(s) = start {
            if i - last_ink > MAX_GAP {
                if last_ink + 1 - s >= min_len {
                    out.push((s, last_ink + 1));
                }
                start = None;
            }
        }
    }
    if let Some(s) = start {
        if last_ink + 1 - s >= min_len {
            out.push((s, last_ink + 1));
        }
    }
    out
}

struct Segment {
    start: u32,
    end: u32,
    first: u32,
    last: u32,
    area: f64,
}

fn overlap(a: (u32, u32), b: (u32, u32)) -> f64 {
    let o = a.1.min(b.1) as f64 - a.0.max(b.0) as f64;
    let shorter = (a.1 - a.0).min(b.1 - b.0) as f64;
    if shorter <= 0.0 {
        0.0
    } else {
        o / shorter
    }
}

// Merges runs on consecutive scanlines into segments. `line(i)` yields the
// ink of scanline i across the perpendicular dimension.
fn segments(n_lines: u32, min_len: u32, line: impl Fn(u32) -> Vec<bool>) -> Vec<Segment> {
    let mut open: Vec<Segment> = Vec::new();
    let mut done = Vec::new();
    for i in 0..n_lines {
        let cur = runs(line(i).into_iter(), min_len);
        let mut next: Vec<Segment> = Vec::new();
        let mut used = vec![false; open.len()];
        for r in cur {
            let hit = open
                .iter()
                .enumerate()
                .find(|(k, s)| !used[*k] && s.last + 1 == i && overlap((s.start, s.end), r) >= 0.5);
            match hit {
                Some((k, _)) => {
                    used[k] = true;
                    let s = &open[k];
                    next.push(Segment {
                        start: s.start.min(r.0),
                        end: s.end.max(r.1),
                        first: s.first,
                        last: i,
                        area: s.area + (r.1 - r.0) as f64,
                    });
                }
                None => next.push(Segment {
                    start: r.0,
                    end: r.1,
                    first: i,
                    last: i,
                    area: (r.1 - r.0) as f64,
                }),
            }
        }
        for (k, s) in open.into_iter().enumerate() {
            if !used[k] {
                done.push(s);
            }
        }
        open = next;
    }
    done.extend(open);
    done
}

fn keep(seg: &Segment) -> Option<(f64, f64, f64, f64)> {
    let len = (seg.end - seg.start) as f64;
    let thick = (seg.area / len).max(1.0);
    if len < MIN_ASPECT * thick {
        return None;
    }
    let pos = (seg.first + seg.last + 1) as f64 / 2.0;
    Some((seg.start as f64, seg.end as f64, pos, thick))
}

// Position across the rule and extent along it.
fn axes(r: &Rule) -> (f64, f64, f64) {
    if r.is_horizontal() {
        (r.y1, r.x1, r.x2)
    } else {
        (r.x1, r.y1, r.y2)
    }
}

fn collinear(a: &Rule, b: &Rule) -> bool {
    if a.is_horizontal() != b.is_horizontal() {
        return false;
    }
    let (pa, a1, a2) = axes(a);
    let (pb, b1, b2) = axes(b);
    (pa - pb).abs() <= (a.width.max(b.width)) / 2.0 + MERGE_SLACK
        && a1 <= b2 + MERGE_SLACK
        && b1 <= a2 + MERGE_SLACK
}

fn join(a: &Rule, b: &Rule) -> Rule {
    let (pa, a1, a2) = axes(a);
    let (pb, b1, b2) = axes(b);
    let (la, lb) = (a2 - a1, b2 - b1);
    let pos = (pa * la + pb * lb) / (la + lb).max(1.0);
    let (lo, hi) = (a1.min(b1), a2.max(b2));
    let width = a.width.max(b.width);
    if a.is_horizontal() {
        Rule {
            x1: lo,
            y1: pos,
            x2: hi,
            y2: pos,
            width,
        }
    } else {
        Rule {
            x1: pos,
            y1: lo,
            x2: pos,
            y2: hi,
            width,
        }
    }
}

// A scanned line often comes out as several overlapping segments on
// neighbouring scanlines; they are folded into one.
fn merge_collinear(mut rules: Vec<Rule>) -> Vec<Rule> {
    let mut out: Vec<Rule> = Vec::new();
    while let Some(mut r) = rules.pop() {
        let mut merged = true;
        while merged {
            merged = false;
            if let Some(i) = out.iter().position(|o| collinear(o, &r)) {
                r = join(&out.swap_remove(i), &r);
                merged = true;
            }
        }
        out.push(r);
    }
    out
}

pub fn find_rules(ink: &GrayImage) -> Vec<Rule> {
    let (w, h) = ink.dimensions();
    let is_ink = |x: u32, y: u32| ink.get_pixel(x, y).0[0] > 0;
    let mut out = Vec::new();
    let min_w = ((w as f64 * MIN_LEN).round() as u32).max(3);
    for seg in segments(h, min_w, |y| (0..w).map(|x| is_ink(x, y)).collect()) {
        if let Some((x1, x2, y, width)) = keep(&seg) {
            out.push(Rule {
                x1,
                y1: y,
                x2,
                y2: y,
                width,
            });
        }
    }
    let min_h = ((h as f64 * MIN_LEN).round() as u32).max(3);
    for seg in segments(w, min_h, |x| (0..h).map(|y| is_ink(x, y)).collect()) {
        if let Some((y1, y2, x, width)) = keep(&seg) {
            out.push(Rule {
                x1: x,
                y1,
                x2: x,
                y2,
                width,
            });
        }
    }
    merge_collinear(out)
}

#[cfg(test)]
pub mod testing {
    use image::{GrayImage, Luma};

    // An ink mask with a grid: `cols` x positions and `rows` y positions.
    pub fn grid(w: u32, h: u32, xs: &[u32], ys: &[u32], thick: u32) -> GrayImage {
        let mut img = GrayImage::new(w, h);
        let (x0, x1) = (xs[0], xs[xs.len() - 1]);
        let (y0, y1) = (ys[0], ys[ys.len() - 1]);
        for &x in xs {
            for y in y0..=y1 {
                for t in 0..thick {
                    img.put_pixel(x + t, y, Luma([255]));
                }
            }
        }
        for &y in ys {
            for x in x0..=x1 + thick - 1 {
                for t in 0..thick {
                    img.put_pixel(x, y + t, Luma([255]));
                }
            }
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::testing::grid;
    use super::*;
    use image::Luma;

    #[test]
    fn finds_grid_lines() {
        let img = grid(400, 300, &[50, 150, 250], &[40, 120, 200, 280], 3);
        let rules = find_rules(&img);
        let h: Vec<_> = rules.iter().filter(|r| r.is_horizontal()).collect();
        let v: Vec<_> = rules.iter().filter(|r| !r.is_horizontal()).collect();
        assert_eq!(h.len(), 4, "{rules:?}");
        assert_eq!(v.len(), 3, "{rules:?}");
        assert!(h
            .iter()
            .all(|r| (r.width - 3.0).abs() < 0.6 && r.x1 <= 51.0 && r.x2 >= 251.0));
        assert!((v[0].y1 - 40.0).abs() <= 1.0 && (v[0].y2 - 283.0).abs() <= 1.0);
    }

    #[test]
    fn folds_overlapping_segments_into_one_rule() {
        let r = |x1: f64, y: f64, x2: f64, w: f64| Rule {
            x1,
            y1: y,
            x2,
            y2: y,
            width: w,
        };
        let rules = merge_collinear(vec![
            r(10.0, 100.0, 200.0, 2.0),
            r(150.0, 101.0, 400.0, 1.0),
            r(10.0, 140.0, 400.0, 1.0),
        ]);
        assert_eq!(rules.len(), 2, "{rules:?}");
        let long = rules.iter().find(|r| r.y1 < 120.0).unwrap();
        assert_eq!((long.x1, long.x2, long.width), (10.0, 400.0, 2.0));
        assert!((long.y1 - 100.0).abs() < 1.0);
    }

    #[test]
    fn ignores_blobs_and_short_runs() {
        let mut img = GrayImage::new(400, 300);
        for y in 100..140 {
            for x in 100..160 {
                img.put_pixel(x, y, Luma([255]));
            }
        }
        for x in 200..204 {
            img.put_pixel(x, 50, Luma([255]));
        }
        assert!(find_rules(&img).is_empty());
    }
}
