// Bookmarks (/Outlines): rebuilt from the frontend's tree when the user
// edited them, otherwise pruned of items whose page no longer exists.
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId};
use serde::Deserialize;
use std::collections::HashSet;

use crate::common::{catalog_id, err, R};

#[derive(Deserialize, Clone, Debug)]
pub struct OutlineNode {
    pub title: String,
    // 0-based index into the output page order; None = no page target.
    pub page: Option<u32>,
    // /XYZ top in PDF user space; None = fit the whole page.
    pub top: Option<f64>,
    pub url: Option<String>,
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub children: Vec<OutlineNode>,
}

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
        let a =
            dictionary! { "S" => "URI", "URI" => Object::string_literal(url.as_bytes().to_vec()) };
        return Some(Object::Reference(doc.add_object(a)));
    }
    let page = node.page.and_then(|p| kids.get(p as usize).copied())?;
    let dest = match node.top {
        Some(top) => vec![
            page.into(),
            "XYZ".into(),
            Object::Null,
            top.into(),
            Object::Null,
        ],
        None => vec![page.into(), "Fit".into()],
    };
    let a = dictionary! { "S" => "GoTo", "D" => dest };
    Some(Object::Reference(doc.add_object(a)))
}

fn visible_count(nodes: &[OutlineNode]) -> i64 {
    nodes
        .iter()
        .map(|n| {
            1 + if n.open {
                visible_count(&n.children)
            } else {
                0
            }
        })
        .sum()
}

// Returns (first, last) child object ids after inserting the whole level.
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

fn outlines_id(doc: &Document, cat_id: ObjectId) -> Option<ObjectId> {
    doc.get_object(cat_id)
        .ok()?
        .as_dict()
        .ok()?
        .get(b"Outlines")
        .ok()?
        .as_reference()
        .ok()
}

fn reference(dict: &Dictionary, key: &[u8]) -> Option<ObjectId> {
    dict.get(key).ok().and_then(|o| o.as_reference().ok())
}

// The page an item jumps to when its destination is an explicit page
// reference. Named destinations, URI actions and remote targets yield None
// and are left alone.
fn dest_page(doc: &Document, item: &Dictionary) -> Option<ObjectId> {
    let dest = match item.get(b"Dest") {
        Ok(d) => d,
        Err(_) => {
            let a = doc
                .dereference(item.get(b"A").ok()?)
                .ok()?
                .1
                .as_dict()
                .ok()?;
            if a.get(b"S").and_then(Object::as_name).ok()? != b"GoTo" {
                return None;
            }
            a.get(b"D").ok()?
        }
    };
    let arr = doc.dereference(dest).ok()?.1.as_array().ok()?;
    arr.first()?.as_reference().ok()
}

// Number of items shown under `id` when it is expanded: its own Count when
// open, nothing when closed.
fn shown_below(doc: &Document, id: ObjectId) -> i64 {
    doc.get_object(id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| d.get(b"Count").ok())
        .and_then(|c| c.as_i64().ok())
        .filter(|c| *c > 0)
        .unwrap_or(0)
}

// Rewrites the sibling links of `children` under `parent` and the parent's
// First/Last/Count accordingly.
fn relink(doc: &mut Document, parent: ObjectId, children: &[ObjectId], open: bool) {
    for (i, &id) in children.iter().enumerate() {
        let prev = i.checked_sub(1).map(|j| children[j]);
        let next = children.get(i + 1).copied();
        let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) else {
            continue;
        };
        d.set("Parent", parent);
        match prev {
            Some(p) => d.set("Prev", p),
            None => {
                d.remove(b"Prev");
            }
        }
        match next {
            Some(n) => d.set("Next", n),
            None => {
                d.remove(b"Next");
            }
        }
    }
    let visible: i64 = children.iter().map(|&c| 1 + shown_below(doc, c)).sum();
    let Ok(d) = doc.get_object_mut(parent).and_then(|o| o.as_dict_mut()) else {
        return;
    };
    match (children.first(), children.last()) {
        (Some(&f), Some(&l)) => {
            d.set("First", f);
            d.set("Last", l);
            d.set("Count", if open { visible } else { -visible });
        }
        _ => {
            d.remove(b"First");
            d.remove(b"Last");
            d.remove(b"Count");
        }
    }
}

// Walks a sibling chain and returns the ids that survive, each with its own
// subtree already pruned. A dropped item's surviving children take its
// place so bookmarks to kept pages never vanish with their parent.
fn prune_level(
    doc: &mut Document,
    first: Option<ObjectId>,
    kids: &[ObjectId],
    seen: &mut HashSet<ObjectId>,
) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut cur = first;
    while let Some(id) = cur {
        // `seen` guards against malformed files whose Next links loop.
        if !seen.insert(id) {
            break;
        }
        let Some(dict) = doc
            .get_object(id)
            .ok()
            .and_then(|o| o.as_dict().ok())
            .cloned()
        else {
            break;
        };
        cur = reference(&dict, b"Next");
        let children = prune_level(doc, reference(&dict, b"First"), kids, seen);
        let dead = dest_page(doc, &dict).is_some_and(|p| !kids.contains(&p));
        if dead {
            out.extend(children);
        } else {
            let open = dict
                .get(b"Count")
                .and_then(|c| c.as_i64())
                .map(|c| c > 0)
                .unwrap_or(false);
            relink(doc, id, &children, open);
            out.push(id);
        }
    }
    out
}

fn prune(doc: &mut Document, kids: &[ObjectId]) -> R<()> {
    let cat_id = catalog_id(doc)?;
    let Some(root_id) = outlines_id(doc, cat_id) else {
        return Ok(());
    };
    let first = doc
        .get_object(root_id)
        .ok()
        .and_then(|o| o.as_dict().ok())
        .and_then(|d| reference(d, b"First"));
    let mut seen = HashSet::from([root_id]);
    let items = prune_level(doc, first, kids, &mut seen);
    if items.is_empty() {
        let cat = doc
            .get_object_mut(cat_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        cat.remove(b"Outlines");
    } else {
        relink(doc, root_id, &items, true);
    }
    Ok(())
}

// `Some` replaces the file's outline entirely; `None` keeps it, minus the
// items whose page is no longer among `kids`.
pub fn apply(doc: &mut Document, kids: &[ObjectId], outline: Option<&[OutlineNode]>) -> R<()> {
    let Some(nodes) = outline else {
        return prune(doc, kids);
    };
    let cat_id = catalog_id(doc)?;

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

    let cat = doc
        .get_object_mut(cat_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    cat.set("Outlines", Object::Reference(root_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};
    use crate::pages::{rebuild_with, Extras, PageSpec};

    fn node(title: &str, page: Option<u32>, children: Vec<OutlineNode>) -> OutlineNode {
        OutlineNode {
            title: title.into(),
            page,
            top: None,
            url: None,
            open: true,
            children,
        }
    }

    fn specs(src: &[i32]) -> Vec<PageSpec> {
        src.iter().map(|&p| PageSpec::page(p, 0)).collect()
    }

    // Top-level items as (title, destination page id).
    fn top_items(doc: &Document) -> Vec<(String, Option<ObjectId>)> {
        let cat_id = catalog_id(doc).unwrap();
        let Some(root) = outlines_id(doc, cat_id) else {
            return Vec::new();
        };
        let root = doc.get_object(root).unwrap().as_dict().unwrap();
        let mut out = Vec::new();
        let mut cur = reference(root, b"First");
        while let Some(id) = cur {
            let d = doc.get_object(id).unwrap().as_dict().unwrap();
            let title =
                String::from_utf8_lossy(d.get(b"Title").unwrap().as_str().unwrap()).to_string();
            out.push((title, dest_page(doc, d)));
            cur = reference(d, b"Next");
        }
        out
    }

    // A: page 1, B: page 2 with child C: page 3, D: a URL.
    fn doc_with_outline() -> Document {
        let mut doc = sample_doc(3);
        let mut d = node("D", None, vec![]);
        d.url = Some("https://example.com".into());
        let nodes = vec![
            node("A", Some(0), vec![]),
            node("B", Some(1), vec![node("C", Some(2), vec![])]),
            d,
        ];
        let extras = Extras {
            outline: Some(&nodes),
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs(&[1, 2, 3]), &[], &[], &[], &extras).unwrap();
        roundtrip(&mut doc)
    }

    #[test]
    fn builds_nested_tree_with_targets() {
        let mut doc = sample_doc(3);
        let nodes = vec![OutlineNode {
            title: "الفصل الأول".into(),
            page: Some(0),
            top: Some(700.0),
            url: None,
            open: true,
            children: vec![node("Section", Some(2), vec![])],
        }];
        let extras = Extras {
            outline: Some(&nodes),
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs(&[1, 2, 3]), &[], &[], &[], &extras).unwrap();
        let out = roundtrip(&mut doc);
        let root = outlines_id(&out, catalog_id(&out).unwrap()).unwrap();
        let first = reference(out.get_object(root).unwrap().as_dict().unwrap(), b"First").unwrap();
        let top = out.get_object(first).unwrap().as_dict().unwrap();
        assert!(top
            .get(b"Title")
            .unwrap()
            .as_str()
            .unwrap()
            .starts_with(&[0xFE, 0xFF]));
        assert!(top.get(b"First").is_ok());
        assert_eq!(top.get(b"Count").unwrap().as_i64().unwrap(), 1);
    }

    #[test]
    fn prune_drops_items_to_deleted_pages() {
        let mut doc = doc_with_outline();
        rebuild_with(&mut doc, &specs(&[2]), &[], &[], &[], &Extras::default()).unwrap();
        let out = roundtrip(&mut doc);
        let page = out.get_pages()[&1];
        let items = top_items(&out);
        assert_eq!(
            items,
            vec![("B".to_string(), Some(page)), ("D".to_string(), None)]
        );
        let root = outlines_id(&out, catalog_id(&out).unwrap()).unwrap();
        let root = out.get_object(root).unwrap().as_dict().unwrap();
        assert_eq!(root.get(b"Count").unwrap().as_i64().unwrap(), 2);
        // B lost its only child, so it is a leaf again.
        let b = out
            .get_object(reference(root, b"First").unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        assert!(b.get(b"First").is_err() && b.get(b"Count").is_err());
        assert!(b.get(b"Prev").is_err());
        let d = out
            .get_object(reference(root, b"Last").unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        assert_eq!(reference(d, b"Prev"), reference(root, b"First"));
        assert!(d.get(b"Next").is_err());
    }

    #[test]
    fn prune_hoists_children_of_dropped_items() {
        let mut doc = doc_with_outline();
        rebuild_with(&mut doc, &specs(&[3]), &[], &[], &[], &Extras::default()).unwrap();
        let out = roundtrip(&mut doc);
        let page = out.get_pages()[&1];
        assert_eq!(
            top_items(&out),
            vec![("C".to_string(), Some(page)), ("D".to_string(), None)]
        );
    }

    #[test]
    fn prune_removes_empty_outline() {
        let mut doc = sample_doc(2);
        let nodes = vec![node("A", Some(0), vec![])];
        let extras = Extras {
            outline: Some(&nodes),
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs(&[1, 2]), &[], &[], &[], &extras).unwrap();
        let mut doc = roundtrip(&mut doc);
        rebuild_with(&mut doc, &specs(&[2]), &[], &[], &[], &Extras::default()).unwrap();
        let out = roundtrip(&mut doc);
        assert!(outlines_id(&out, catalog_id(&out).unwrap()).is_none());
        assert!(top_items(&out).is_empty());
    }
}
