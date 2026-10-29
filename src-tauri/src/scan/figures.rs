// Picture regions: ink that is neither recognised text nor a ruling line.
use base64::Engine;
use image::{GrayImage, ImageFormat, Luma, RgbImage};
use imageproc::distance_transform::Norm;
use imageproc::morphology::dilate;
use imageproc::region_labelling::{connected_components, Connectivity};
use std::collections::HashMap;

use super::rules::Rule;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub bbox: [f64; 4],
    pub png_b64: String,
}

// Minimum bbox area as a share of the page.
const MIN_AREA: f64 = 0.005;
// Minimum short side as a share of the page's longer side.
const MIN_SIDE: f64 = 0.01;
// Padding around excluded boxes and around the final crop, in pixels.
const PAD: f64 = 3.0;
// A dense region: minimum area and short side as shares of the page,
// minimum ink fill, and the most of it recognised text may cover.
const DENSE_MIN_AREA: f64 = 0.004;
const DENSE_MIN_SIDE: f64 = 0.03;
const DENSE_MIN_FILL: f64 = 0.25;
const DENSE_MAX_TEXT: f64 = 0.3;

// Mask at reduced resolution with the excluded boxes cleared.
fn reduced_mask(ink: &GrayImage, exclude: &[[f64; 4]], f: u32) -> GrayImage {
    let (w, h) = (ink.width().div_ceil(f), ink.height().div_ceil(f));
    let mut mask = GrayImage::new(w, h);
    for by in 0..h {
        for bx in 0..w {
            let hit = (0..f).any(|dy| {
                (0..f).any(|dx| {
                    let (x, y) = (bx * f + dx, by * f + dy);
                    x < ink.width() && y < ink.height() && ink.get_pixel(x, y).0[0] > 0
                })
            });
            if hit {
                mask.put_pixel(bx, by, Luma([255]));
            }
        }
    }
    for b in exclude {
        let x0 = ((b[0] - PAD) / f as f64).floor().max(0.0) as u32;
        let y0 = ((b[1] - PAD) / f as f64).floor().max(0.0) as u32;
        let x1 = (((b[2] + PAD) / f as f64).ceil() as u32).min(w);
        let y1 = (((b[3] + PAD) / f as f64).ceil() as u32).min(h);
        for y in y0..y1 {
            for x in x0..x1 {
                mask.put_pixel(x, y, Luma([0]));
            }
        }
    }
    mask
}

fn component_boxes(mask: &GrayImage) -> Vec<[u32; 4]> {
    let labels = connected_components(mask, Connectivity::Eight, Luma([0u8]));
    let mut boxes: HashMap<u32, [u32; 4]> = HashMap::new();
    for (x, y, p) in labels.enumerate_pixels() {
        if p.0[0] == 0 {
            continue;
        }
        let b = boxes.entry(p.0[0]).or_insert([x, y, x, y]);
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].max(x);
        b[3] = b[3].max(y);
    }
    boxes.into_values().collect()
}

fn area(b: &[f64; 4]) -> f64 {
    (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0)
}

fn intersection(a: &[f64; 4], b: &[f64; 4]) -> f64 {
    area(&[
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ])
}

// True when at least `share` of `a` lies inside `b`.
pub fn mostly_inside(a: &[f64; 4], b: &[f64; 4], share: f64) -> bool {
    let aa = area(a);
    aa > 0.0 && intersection(a, b) >= share * aa
}

fn ink_fill(ink: &GrayImage, b: &[f64; 4]) -> f64 {
    let x0 = b[0].max(0.0) as u32;
    let y0 = b[1].max(0.0) as u32;
    let x1 = (b[2] as u32).min(ink.width());
    let y1 = (b[3] as u32).min(ink.height());
    let total = ((x1.saturating_sub(x0)) * (y1.saturating_sub(y0))) as f64;
    if total == 0.0 {
        return 0.0;
    }
    let n = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| ink.get_pixel(x, y).0[0] > 0)
        .count();
    n as f64 / total
}

// Paper: light and unsaturated. Share of such pixels a box needs.
const PAPER_SHARE: f64 = 0.6;
const PAPER_MIN: u8 = 200;
const PAPER_SPREAD: u8 = 40;

// True when the box is mostly blank paper rather than a picture.
pub fn paper_like(color: &RgbImage, b: &[f64; 4]) -> bool {
    let x0 = b[0].max(0.0) as u32;
    let y0 = b[1].max(0.0) as u32;
    let x1 = (b[2] as u32).min(color.width());
    let y1 = (b[3] as u32).min(color.height());
    let total = (x1.saturating_sub(x0) * y1.saturating_sub(y0)) as f64;
    if total == 0.0 {
        return true;
    }
    let paper = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let p = color.get_pixel(x, y).0;
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            lo >= PAPER_MIN && hi - lo <= PAPER_SPREAD
        })
        .count();
    paper as f64 / total >= PAPER_SHARE
}

// Regions of heavy ink that recognised text covers little of: photos,
// logos, stamps and filled banners. Lines found inside them are picture
// content rather than rules.
pub fn dense_regions(ink: &GrayImage, text: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let (w, h) = (ink.width() as f64, ink.height() as f64);
    let f = ink.width().div_ceil(350).max(1);
    let mask = dilate(&reduced_mask(ink, &[], f), Norm::LInf, 1);
    let s = f as f64;
    let min_area = DENSE_MIN_AREA * w * h;
    let min_side = DENSE_MIN_SIDE * w.max(h);
    component_boxes(&mask)
        .into_iter()
        .map(|b| {
            [
                (b[0] as f64 * s).max(0.0),
                (b[1] as f64 * s).max(0.0),
                ((b[2] + 1) as f64 * s).min(w),
                ((b[3] + 1) as f64 * s).min(h),
            ]
        })
        .filter(|b| {
            let a = area(b);
            a >= min_area
                && (b[2] - b[0]).min(b[3] - b[1]) >= min_side
                && ink_fill(ink, b) >= DENSE_MIN_FILL
                && text.iter().map(|t| intersection(t, b)).sum::<f64>() < DENSE_MAX_TEXT * a
        })
        .collect()
}

fn overlaps(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

fn merge_overlapping(mut boxes: Vec<[f64; 4]>) -> Vec<[f64; 4]> {
    loop {
        let mut merged = false;
        'outer: for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                if overlaps(&boxes[i], &boxes[j]) {
                    let b = boxes.remove(j);
                    let a = &mut boxes[i];
                    *a = [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[2].max(b[2]),
                        a[3].max(b[3]),
                    ];
                    merged = true;
                    break 'outer;
                }
            }
        }
        if !merged {
            return boxes;
        }
    }
}

// Figure boxes in full-resolution pixels (without the crops).
pub fn figure_boxes(ink: &GrayImage, exclude: &[[f64; 4]], rules: &[Rule]) -> Vec<[f64; 4]> {
    let (w, h) = (ink.width() as f64, ink.height() as f64);
    let f = ink.width().div_ceil(700).max(1);
    let mut excl: Vec<[f64; 4]> = exclude.to_vec();
    excl.extend(rules.iter().map(Rule::bbox));
    let mask = reduced_mask(ink, &excl, f);
    let min_area = MIN_AREA * w * h;
    let min_side = MIN_SIDE * w.max(h);
    let boxes: Vec<[f64; 4]> = component_boxes(&mask)
        .into_iter()
        .map(|b| {
            let s = f as f64;
            [
                b[0] as f64 * s,
                b[1] as f64 * s,
                (b[2] + 1) as f64 * s,
                (b[3] + 1) as f64 * s,
            ]
        })
        .filter(|b| {
            let (bw, bh) = (b[2] - b[0], b[3] - b[1]);
            bw * bh >= min_area && bw.min(bh) >= min_side && bw.max(bh) / bw.min(bh) < 20.0
        })
        .collect();
    let mut out = merge_overlapping(boxes);
    for b in &mut out {
        *b = [
            (b[0] - PAD).max(0.0),
            (b[1] - PAD).max(0.0),
            (b[2] + PAD).min(w),
            (b[3] + PAD).min(h),
        ];
    }
    out.sort_by(|a, b| a[1].partial_cmp(&b[1]).unwrap());
    out
}

pub fn png_base64(img: &RgbImage) -> Option<String> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(buf.into_inner()))
}

// Longest side a picture keeps, in pixels; scans are far denser than needed.
const MAX_SIDE: u32 = 1600;

// The picture inside `b`, cut from the page and kept to a sensible size.
pub fn crop(color: &RgbImage, b: &[f64; 4]) -> Option<Figure> {
    let x = (b[0].max(0.0).floor() as u32).min(color.width());
    let y = (b[1].max(0.0).floor() as u32).min(color.height());
    let bw = (b[2].ceil() as u32).min(color.width()).saturating_sub(x);
    let bh = (b[3].ceil() as u32).min(color.height()).saturating_sub(y);
    if bw < 2 || bh < 2 {
        return None;
    }
    let mut crop = image::imageops::crop_imm(color, x, y, bw, bh).to_image();
    let long = bw.max(bh);
    if long > MAX_SIDE {
        let k = MAX_SIDE as f64 / long as f64;
        crop = image::imageops::resize(
            &crop,
            ((bw as f64 * k).round() as u32).max(1),
            ((bh as f64 * k).round() as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    Some(Figure {
        bbox: *b,
        png_b64: png_base64(&crop)?,
    })
}

pub fn find_figures(
    ink: &GrayImage,
    color: &RgbImage,
    exclude: &[[f64; 4]],
    rules: &[Rule],
) -> Vec<Figure> {
    figure_boxes(ink, exclude, rules)
        .into_iter()
        .filter_map(|b| crop(color, &b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill(img: &mut GrayImage, b: [u32; 4]) {
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                img.put_pixel(x, y, Luma([255]));
            }
        }
    }

    #[test]
    fn finds_picture_and_skips_text_and_rules() {
        let mut ink = GrayImage::new(1000, 800);
        fill(&mut ink, [100, 100, 400, 350]); // picture
        fill(&mut ink, [500, 100, 900, 130]); // a text line
        fill(&mut ink, [100, 500, 900, 504]); // a rule
        fill(&mut ink, [600, 600, 620, 620]); // speck
        let text = [[500.0, 100.0, 900.0, 130.0]];
        let rules = [Rule {
            x1: 100.0,
            y1: 502.0,
            x2: 900.0,
            y2: 502.0,
            width: 4.0,
        }];
        let boxes = figure_boxes(&ink, &text, &rules);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        let b = boxes[0];
        assert!(
            b[0] <= 100.0 && b[1] <= 100.0 && b[2] >= 400.0 && b[3] >= 350.0,
            "{b:?}"
        );
        assert!(b[0] >= 90.0 && b[3] <= 360.0, "{b:?}");
        let color = RgbImage::from_pixel(1000, 800, image::Rgb([10, 20, 30]));
        let figs = find_figures(&ink, &color, &text, &rules);
        assert_eq!(figs.len(), 1);
        assert!(!figs[0].png_b64.is_empty());
    }

    #[test]
    fn dense_regions_are_pictures_not_text() {
        let mut ink = GrayImage::new(1000, 800);
        fill(&mut ink, [100, 100, 400, 350]); // photo
        fill(&mut ink, [500, 100, 900, 130]); // a text line
        fill(&mut ink, [500, 150, 900, 180]); // another text line
        for y in (400..700).step_by(60) {
            fill(&mut ink, [100, y, 900, y + 3]); // table rules
        }
        let text = [[500.0, 100.0, 900.0, 130.0], [500.0, 150.0, 900.0, 180.0]];
        let dense = dense_regions(&ink, &text);
        assert_eq!(dense.len(), 1, "{dense:?}");
        assert!(mostly_inside(
            &[100.0, 100.0, 400.0, 350.0],
            &dense[0],
            0.99
        ));
        assert!(!mostly_inside(
            &[100.0, 400.0, 900.0, 403.0],
            &dense[0],
            0.5
        ));
    }

    #[test]
    fn paper_is_light_and_grey() {
        let mut img = RgbImage::from_pixel(100, 100, image::Rgb([250, 250, 248]));
        assert!(paper_like(&img, &[0.0, 0.0, 100.0, 100.0]));
        for y in 0..100 {
            for x in 0..70 {
                img.put_pixel(x, y, image::Rgb([220, 170, 60]));
            }
        }
        assert!(!paper_like(&img, &[0.0, 0.0, 100.0, 100.0]));
        assert!(paper_like(&img, &[70.0, 0.0, 100.0, 100.0]));
    }

    #[test]
    fn merges_touching_parts() {
        let mut ink = GrayImage::new(1000, 800);
        fill(&mut ink, [100, 100, 300, 300]);
        fill(&mut ink, [290, 290, 500, 500]);
        assert_eq!(figure_boxes(&ink, &[], &[]).len(), 1);
    }
}
