// Image preparation: grayscale, Otsu binarisation and deskew.
use image::{GrayImage, Luma, Rgb, RgbImage};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};

pub struct Prepped {
    pub color: RgbImage,
    // 255 where the page has ink, 0 elsewhere.
    pub ink: GrayImage,
}

pub fn grayscale(img: &RgbImage) -> GrayImage {
    image::imageops::grayscale(img)
}

// Otsu's threshold: the gray level that maximises the between-class
// variance of the histogram.
pub fn otsu_threshold(gray: &GrayImage) -> u8 {
    let mut hist = [0u64; 256];
    for p in gray.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total = gray.pixels().len() as f64;
    if total == 0.0 {
        return 128;
    }
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &n)| i as f64 * n as f64)
        .sum();
    let (mut w_b, mut sum_b, mut best, mut best_t) = (0.0, 0.0, -1.0, 128u8);
    for (t, &n) in hist.iter().enumerate() {
        w_b += n as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += t as f64 * n as f64;
        let m_b = sum_b / w_b;
        let m_f = (sum - sum_b) / w_f;
        let between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if between > best {
            best = between;
            best_t = t as u8;
        }
    }
    best_t
}

// Ink mask: pixels at or below the Otsu threshold. A page without any
// real contrast (blank or a flat tint) yields an empty mask.
pub fn binarise(gray: &GrayImage) -> GrayImage {
    let t = otsu_threshold(gray);
    let (mut lo, mut hi, mut n_lo) = (0.0, 0.0, 0usize);
    for p in gray.pixels() {
        let v = p.0[0] as f64;
        if p.0[0] <= t {
            lo += v;
            n_lo += 1;
        } else {
            hi += v;
        }
    }
    let n_hi = gray.pixels().len() - n_lo;
    let contrast = if n_lo > 0 && n_hi > 0 {
        hi / n_hi as f64 - lo / n_lo as f64
    } else {
        0.0
    };
    if contrast < 40.0 {
        return GrayImage::new(gray.width(), gray.height());
    }
    GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
        Luma([if gray.get_pixel(x, y).0[0] <= t {
            255
        } else {
            0
        }])
    })
}

// Coordinates of ink pixels on a downscaled copy of the mask; a block is
// ink if any of its pixels is.
fn ink_points(ink: &GrayImage, max_w: u32) -> (Vec<(f32, f32)>, u32, u32) {
    let f = ink.width().div_ceil(max_w).max(1);
    let (w, h) = (ink.width().div_ceil(f), ink.height().div_ceil(f));
    let mut pts = Vec::new();
    for by in 0..h {
        for bx in 0..w {
            let hit = (0..f).any(|dy| {
                (0..f).any(|dx| {
                    let (x, y) = (bx * f + dx, by * f + dy);
                    x < ink.width() && y < ink.height() && ink.get_pixel(x, y).0[0] > 0
                })
            });
            if hit {
                pts.push((bx as f32, by as f32));
            }
        }
    }
    (pts, w, h)
}

// Variance of the horizontal projection profile after rotating the ink
// points by `deg` (clockwise, image coordinates).
fn profile_variance(pts: &[(f32, f32)], w: u32, h: u32, deg: f64) -> f64 {
    let (s, c) = deg.to_radians().sin_cos();
    let margin = (w as f64 * 0.1).ceil() as usize + 2;
    let mut rows = vec![0u32; h as usize + 2 * margin];
    for &(x, y) in pts {
        let yr = s * x as f64 + c * y as f64 + margin as f64;
        let i = yr.round();
        if i >= 0.0 && (i as usize) < rows.len() {
            rows[i as usize] += 1;
        }
    }
    let n = rows.len() as f64;
    let mean = rows.iter().map(|&r| r as f64).sum::<f64>() / n;
    rows.iter().map(|&r| (r as f64 - mean).powi(2)).sum::<f64>() / n
}

fn best_angle(pts: &[(f32, f32)], w: u32, h: u32, center: f64, half: f64, step: f64) -> f64 {
    let n = (2.0 * half / step).round() as i64;
    (0..=n)
        .map(|i| center - half + i as f64 * step)
        .map(|a| (a, profile_variance(pts, w, h, a)))
        .fold(
            (center, f64::MIN),
            |best, cur| if cur.1 > best.1 { cur } else { best },
        )
        .0
}

// Skew correction in degrees: rotate the image clockwise by this to
// straighten it. Coarse 0.2° search over ±5°, then 0.05° refinement.
pub fn estimate_skew(ink: &GrayImage) -> f64 {
    let (pts, w, h) = ink_points(ink, 1000);
    if pts.len() < 50 {
        return 0.0;
    }
    let coarse = best_angle(&pts, w, h, 0.0, 5.0, 0.2);
    best_angle(&pts, w, h, coarse, 0.2, 0.05)
}

pub fn rotate(img: &RgbImage, deg: f64) -> RgbImage {
    rotate_about_center(
        img,
        deg.to_radians() as f32,
        Interpolation::Bilinear,
        Rgb([255, 255, 255]),
    )
}

// Straightens the page: returns the corrected image and the angle used.
pub fn deskew(color: &RgbImage) -> (RgbImage, f64) {
    let ink = binarise(&grayscale(color));
    let angle = estimate_skew(&ink);
    if angle.abs() < 0.05 {
        return (color.clone(), 0.0);
    }
    (rotate(color, angle), angle)
}

pub fn prepare(color: RgbImage) -> Prepped {
    let (color, _angle) = deskew(&color);
    let ink = binarise(&grayscale(&color));
    Prepped { color, ink }
}

#[cfg(test)]
pub mod testing {
    use image::{Rgb, RgbImage};

    // A white page with several black "text lines" (filled bars).
    pub fn lined_page(w: u32, h: u32, bar_h: u32, gap: u32) -> RgbImage {
        let mut img = RgbImage::from_pixel(w, h, Rgb([255, 255, 255]));
        let margin = w / 10;
        let mut y = h / 8;
        while y + bar_h < h - h / 8 {
            for yy in y..y + bar_h {
                for x in margin..w - margin {
                    img.put_pixel(x, yy, Rgb([0, 0, 0]));
                }
            }
            y += bar_h + gap;
        }
        img
    }
}

#[cfg(test)]
mod tests {
    use super::testing::lined_page;
    use super::*;

    #[test]
    fn otsu_separates_two_tones() {
        let gray = GrayImage::from_fn(100, 100, |x, _| Luma([if x < 30 { 40 } else { 220 }]));
        let t = otsu_threshold(&gray);
        assert!((40..220).contains(&t), "threshold {t}");
        let ink = binarise(&gray);
        assert_eq!(ink.get_pixel(10, 10).0[0], 255);
        assert_eq!(ink.get_pixel(80, 10).0[0], 0);
    }

    #[test]
    fn blank_page_has_no_ink() {
        let gray = GrayImage::from_fn(50, 50, |x, y| Luma([250 - ((x + y) % 3) as u8]));
        assert!(binarise(&gray).pixels().all(|p| p.0[0] == 0));
    }

    #[test]
    fn deskew_recovers_two_degree_rotation() {
        let page = lined_page(800, 600, 12, 20);
        let skewed = rotate(&page, 2.0);
        let (_, angle) = deskew(&skewed);
        assert!((angle + 2.0).abs() <= 0.3, "angle {angle}");
        let skewed = rotate(&page, -1.4);
        let (fixed, angle) = deskew(&skewed);
        assert!((angle - 1.4).abs() <= 0.3, "angle {angle}");
        let straight = estimate_skew(&binarise(&grayscale(&fixed)));
        assert!(straight.abs() <= 0.3, "residual {straight}");
    }
}
