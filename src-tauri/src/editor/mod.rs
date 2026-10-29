// Content-stream object editing: enumerate the text/image/form/path objects
// on a page with stable ids, then delete or transform them by id.
mod apply;
mod enumerate;
pub(crate) mod fonts;
pub(crate) mod mat;

pub use apply::apply_edits;
pub use enumerate::enumerate;
pub use mat::Mat;

use lopdf::content::Operation;
use lopdf::{Document, Object};
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ObjDesc {
    pub id: u32,
    pub kind: &'static str,
    pub bbox: [f64; 4],
}

fn one() -> f64 {
    1.0
}

#[derive(Deserialize, Clone)]
pub struct ObjectEdit {
    pub src: i32,
    pub id: u32,
    pub action: String,
    #[serde(default)]
    pub dx: f64,
    #[serde(default)]
    pub dy: f64,
    #[serde(default = "one")]
    pub sx: f64,
    #[serde(default = "one")]
    pub sy: f64,
    #[serde(default)]
    pub ax: f64,
    #[serde(default)]
    pub ay: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Text,
    Image,
    Form,
    Path,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Form => "form",
            Kind::Path => "path",
        }
    }
}

pub struct PageObj {
    pub id: u32,
    pub kind: Kind,
    pub bbox: [f64; 4],
    // Inclusive index range of the operations that draw the object.
    pub op_range: (usize, usize),
    // CTM in effect when the object was drawn.
    pub ctm: Mat,
}

impl From<&PageObj> for ObjDesc {
    fn from(o: &PageObj) -> Self {
        ObjDesc {
            id: o.id,
            kind: o.kind.as_str(),
            bbox: o.bbox,
        }
    }
}

pub(crate) fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        Object::Real(r) => Some(*r as f64),
        _ => None,
    }
}

pub(crate) fn opnum(op: &Operation, i: usize) -> f64 {
    op.operands.get(i).and_then(num).unwrap_or(0.0)
}

pub(crate) fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    match o {
        Object::Reference(r) => doc.get_object(*r).unwrap_or(o),
        _ => o,
    }
}

// A one-page document with a text run, an image and a filled rectangle.
#[cfg(test)]
pub(crate) mod testing {
    use super::ObjectEdit;
    use lopdf::content::{Content, Operation};
    use lopdf::{dictionary, Document, Object, ObjectId, Stream};

    pub fn sample_doc() -> Document {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let img_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => 1,
                "Height" => 1,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
            },
            vec![255, 0, 0],
        ));
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
            "XObject" => dictionary! { "Im0" => img_id },
        });
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 24.into()]),
                Operation::new("Td", vec![72.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal("Hello world")]),
                Operation::new("ET", vec![]),
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        100.into(),
                        0.into(),
                        0.into(),
                        50.into(),
                        100.into(),
                        600.into(),
                    ],
                ),
                Operation::new("Do", vec!["Im0".into()]),
                Operation::new("Q", vec![]),
                Operation::new("re", vec![50.into(), 50.into(), 200.into(), 30.into()]),
                Operation::new("f", vec![]),
            ],
        };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    pub fn page1(doc: &Document) -> ObjectId {
        doc.get_pages()[&1]
    }

    pub fn edit(id: u32, action: &str) -> ObjectEdit {
        ObjectEdit {
            src: 1,
            id,
            action: action.into(),
            dx: 0.0,
            dy: 0.0,
            sx: 1.0,
            sy: 1.0,
            ax: 0.0,
            ay: 0.0,
        }
    }

    pub fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1.0
    }
}
