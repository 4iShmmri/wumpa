// Glyph outlines as PDF path operators, for glyphs that stand for no text
// (the Naskh dots) and so must stay out of the text objects.
use lopdf::content::Operation;
use lopdf::Object;
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};

use super::fonts::Face;

struct Sink {
    ops: Vec<Operation>,
    m: [f64; 6],
    at: (f64, f64),
}

impl Sink {
    fn map(&self, x: f32, y: f32) -> (f64, f64) {
        let [a, b, c, d, e, f] = self.m;
        let (x, y) = (x as f64, y as f64);
        (a * x + c * y + e, b * x + d * y + f)
    }

    fn push(&mut self, name: &str, pts: &[(f64, f64)]) {
        let args = pts
            .iter()
            .flat_map(|&(x, y)| [Object::Real(x as f32), Object::Real(y as f32)])
            .collect();
        self.ops.push(Operation::new(name, args));
    }
}

impl OutlineBuilder for Sink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.at = self.map(x, y);
        self.push("m", &[self.at]);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.at = self.map(x, y);
        self.push("l", &[self.at]);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let q = self.map(x1, y1);
        let p1 = self.map(x, y);
        let p0 = self.at;
        let c1 = (
            p0.0 + 2.0 / 3.0 * (q.0 - p0.0),
            p0.1 + 2.0 / 3.0 * (q.1 - p0.1),
        );
        let c2 = (
            p1.0 + 2.0 / 3.0 * (q.0 - p1.0),
            p1.1 + 2.0 / 3.0 * (q.1 - p1.1),
        );
        self.at = p1;
        self.push("c", &[c1, c2, p1]);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.map(x1, y1);
        let c2 = self.map(x2, y2);
        self.at = self.map(x, y);
        self.push("c", &[c1, c2, self.at]);
    }

    fn close(&mut self) {
        self.ops.push(Operation::new("h", vec![]));
    }
}

// Path operators filling `gid` under the matrix `m` (font units to page).
pub fn glyph_path(face: &Face, gid: u16, m: [f64; 6]) -> Vec<Operation> {
    let mut sink = Sink {
        ops: Vec::new(),
        m,
        at: (0.0, 0.0),
    };
    if face.hb.outline_glyph(GlyphId(gid), &mut sink).is_some() {
        sink.ops.push(Operation::new("f", vec![]));
    }
    sink.ops
}
