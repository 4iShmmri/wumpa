// 2D affine matrices in PDF [a b c d e f] layout.

pub type Mat = [f64; 6];

pub const IDENT: Mat = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

// `first` then `second`, i.e. first × second in PDF row-vector convention.
pub fn compose(first: Mat, second: Mat) -> Mat {
    let [a1, b1, c1, d1, e1, f1] = first;
    let [a2, b2, c2, d2, e2, f2] = second;
    [
        a1 * a2 + b1 * c2,
        a1 * b2 + b1 * d2,
        c1 * a2 + d1 * c2,
        c1 * b2 + d1 * d2,
        e1 * a2 + f1 * c2 + e2,
        e1 * b2 + f1 * d2 + f2,
    ]
}

pub fn transform(m: Mat, p: (f64, f64)) -> (f64, f64) {
    (
        p.0 * m[0] + p.1 * m[2] + m[4],
        p.0 * m[1] + p.1 * m[3] + m[5],
    )
}

pub fn translate(tx: f64, ty: f64) -> Mat {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

pub fn scale_about(sx: f64, sy: f64, ax: f64, ay: f64) -> Mat {
    compose(
        compose(translate(-ax, -ay), [sx, 0.0, 0.0, sy, 0.0, 0.0]),
        translate(ax, ay),
    )
}

pub fn invert(m: Mat) -> Option<Mat> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det.abs() < 1e-9 {
        return None;
    }
    let (a, b, c, d, e, f) = (m[0], m[1], m[2], m[3], m[4], m[5]);
    Some([
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invert_undoes_compose() {
        let m = compose(scale_about(2.0, 3.0, 10.0, 20.0), translate(5.0, -7.0));
        let inv = invert(m).unwrap();
        let p = transform(compose(m, inv), (1.5, -2.5));
        assert!((p.0 - 1.5).abs() < 1e-9 && (p.1 + 2.5).abs() < 1e-9);
        assert!(invert([1.0, 2.0, 2.0, 4.0, 0.0, 0.0]).is_none());
    }
}
