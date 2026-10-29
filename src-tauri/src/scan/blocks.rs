// The page description handed to the composer. Coordinates are PDF points
// with the origin at the bottom-left corner of the page.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    #[default]
    Auto,
    Rtl,
    Ltr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub bbox: [f64; 4],
    pub text: String,
    pub size: Option<f64>,
    pub align: Align,
    pub dir: Dir,
    pub bold: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Block {
    Text {
        bbox: [f64; 4],
        text: String,
        size: Option<f64>,
        // The box hugs the glyph ink rather than the font's line box.
        tight: bool,
        bold: bool,
        italic: bool,
        align: Align,
        dir: Dir,
        color: [f64; 3],
    },
    Rule {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        width: f64,
        color: [f64; 3],
    },
    Rect {
        bbox: [f64; 4],
        stroke: Option<f64>,
        fill: Option<[f64; 3]>,
    },
    Image {
        bbox: [f64; 4],
        png_b64: String,
    },
    Table {
        bbox: [f64; 4],
        rows: Vec<Vec<Cell>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBlocks {
    pub width: f64,
    pub height: f64,
    pub blocks: Vec<Block>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialises_with_kind_tag_and_camel_case() {
        let page = PageBlocks {
            width: 10.0,
            height: 20.0,
            blocks: vec![
                Block::Image {
                    bbox: [0.0; 4],
                    png_b64: "AA==".into(),
                },
                Block::Text {
                    bbox: [0.0; 4],
                    text: "x".into(),
                    size: None,
                    tight: false,
                    bold: false,
                    italic: false,
                    align: Align::Justify,
                    dir: Dir::Rtl,
                    color: [0.0; 3],
                },
            ],
        };
        let json = serde_json::to_value(&page).unwrap();
        assert_eq!(json["blocks"][0]["kind"], "image");
        assert_eq!(json["blocks"][0]["pngB64"], "AA==");
        assert_eq!(json["blocks"][1]["align"], "justify");
        assert_eq!(json["blocks"][1]["dir"], "rtl");
        let back: PageBlocks = serde_json::from_value(json).unwrap();
        assert_eq!(back, page);
    }
}
