use lopdf::{dictionary, Document, Object, ObjectId};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
pub struct OutlineNode {
    pub title: String,
    /// 0-based index into the output page order; None = no page target.
    pub page: Option<u32>,
    /// /XYZ top in PDF user space; None = fit the whole page.
    pub top: Option<f64>,
    pub url: Option<String>,
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub children: Vec<OutlineNode>,
}

type R<T> = Result<T, String>;

fn title_obj(title: &str) -> Object {
    if title.is_ascii() {
        Object::string_literal(title.as_bytes().to_vec())
    } else {
        let mut bom = vec![0xFE, 0xFF];
        bom.extend(title.encode_utf16().flat_map(u16::to_be_bytes));
        Object::string_literal(bom)
    }
}

fn action(doc: &mut Document, node: &OutlineNode, kids: &[ObjectId]) -> Option<Object> {
    if let Some(url) = node.url.as_deref().filter(|u| !u.is_empty()) {
        let a = dictionary! { "S" => "URI", "URI" => Object::string_literal(url.as_bytes().to_vec()) };
        return Some(Object::Reference(doc.add_object(a)));
    }
    let page = node.page.and_then(|p| kids.get(p as usize).copied())?;
    let dest = match node.top {
        Some(top) => vec![page.into(), "XYZ".into(), Object::Null, top.into(), Object::Null],
        None => vec![page.into(), "Fit".into()],
    };
    let a = dictionary! { "S" => "GoTo", "D" => dest };
    Some(Object::Reference(doc.add_object(a)))
}

fn visible_count(nodes: &[OutlineNode]) -> i64 {
    nodes
        .iter()
        .map(|n| 1 + if n.open { visible_count(&n.children) } else { 0 })
        .sum()
}

/// Returns (first, last) child object ids after inserting the whole level.
fn build_level(
    doc: &mut Document,
    nodes: &[OutlineNode],
    parent: ObjectId,
    kids: &[ObjectId],
) -> R<(Option<ObjectId>, Option<ObjectId>)> {
    let ids: Vec<ObjectId> = nodes.iter().map(|_| doc.new_object_id()).collect();
    for (i, node) in nodes.iter().enumerate() {
        let id = ids[i];
        let mut dict = dictionary! { "Title" => title_obj(&node.title), "Parent" => parent };
        if let Some(a) = action(doc, node, kids) {
            dict.set("A", a);
        }
        if i > 0 {
            dict.set("Prev", ids[i - 1]);
        }
        if i + 1 < ids.len() {
            dict.set("Next", ids[i + 1]);
        }
        if !node.children.is_empty() {
            let (first, last) = build_level(doc, &node.children, id, kids)?;
            if let Some(f) = first {
                dict.set("First", f);
            }
            if let Some(l) = last {
                dict.set("Last", l);
            }
            let c = visible_count(&node.children);
            dict.set("Count", if node.open { c } else { -c });
        }
        doc.set_object(id, dict);
    }
    Ok((ids.first().copied(), ids.last().copied()))
}

/// `None` keeps the file's existing outline; `Some` replaces it entirely.
pub fn apply(doc: &mut Document, kids: &[ObjectId], outline: Option<&[OutlineNode]>) -> R<()> {
    let Some(nodes) = outline else { return Ok(()) };
    let cat_id = crate::pdf_ops::catalog_id(doc)?;

    // Drop whatever outline the file had; we rebuild from scratch.
    if let Ok(cat) = doc.get_object_mut(cat_id).and_then(|o| o.as_dict_mut()) {
        cat.remove(b"Outlines");
    }
    if nodes.is_empty() {
        return Ok(());
    }

    let root_id = doc.new_object_id();
    let (first, last) = build_level(doc, nodes, root_id, kids)?;
    let mut root = dictionary! { "Type" => "Outlines", "Count" => visible_count(nodes) };
    if let Some(f) = first {
        root.set("First", f);
    }
    if let Some(l) = last {
        root.set("Last", l);
    }
    doc.set_object(root_id, root);

    let cat = doc.get_object_mut(cat_id).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?;
    cat.set("Outlines", Object::Reference(root_id));
    Ok(())
}
