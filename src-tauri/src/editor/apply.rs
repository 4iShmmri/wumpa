// Rewrites page content streams to delete or transform enumerated objects.
use lopdf::content::{Content, Operation};
use lopdf::{Document, Object};
use std::collections::{BTreeMap, HashMap, HashSet};

use super::enumerate::enumerate;
use super::mat::{compose, invert, scale_about, translate, Mat, IDENT};
use super::{Kind, ObjectEdit, PageObj};
use crate::common::{err, R};

// Text/graphics state set inside a removed or wrapped range persists afterwards
// per spec, so those operators are re-emitted to keep trailing content intact.
const STICKY_STATE: [&str; 26] = [
    "Tf", "Tc", "Tw", "Tz", "TL", "Ts", "Tr", "gs", "w", "J", "j", "M", "d", "ri", "i", "g", "G",
    "rg", "RG", "k", "K", "cs", "CS", "sc", "scn", "SC",
];

struct Action {
    delete: bool,
    a: Mat,
    transformed: bool,
}

pub fn apply_edits(doc: &mut Document, edits: &[ObjectEdit]) -> R<()> {
    if edits.is_empty() {
        return Ok(());
    }
    let pages = doc.get_pages();
    let mut by_page: BTreeMap<u32, Vec<&ObjectEdit>> = BTreeMap::new();
    // Blank pages (src <= 0) have no objects to edit.
    for e in edits.iter().filter(|e| e.src > 0) {
        by_page.entry(e.src as u32).or_default().push(e);
    }

    for (src, page_edits) in by_page {
        let &pid = pages
            .get(&src)
            .ok_or_else(|| format!("page {src} not found"))?;
        let objs = enumerate(doc, pid)?;
        let by_id: HashMap<u32, &PageObj> = objs.iter().map(|o| (o.id, o)).collect();

        let mut acts: HashMap<u32, Action> = HashMap::new();
        for e in page_edits {
            let obj = by_id
                .get(&e.id)
                .ok_or_else(|| format!("object {} not found on page {src}", e.id))?;
            let act = acts.entry(obj.id).or_insert(Action {
                delete: false,
                a: IDENT,
                transformed: false,
            });
            match e.action.as_str() {
                "delete" => act.delete = true,
                "translate" => {
                    act.a = compose(act.a, translate(e.dx, e.dy));
                    act.transformed = true;
                }
                "scale" => {
                    act.a = compose(act.a, scale_about(e.sx, e.sy, e.ax, e.ay));
                    act.transformed = true;
                }
                other => return Err(format!("unknown action {other}")),
            }
        }

        let data = doc.get_page_content(pid);
        let content = Content::decode(&data).map_err(err)?;
        let n_ops = content.operations.len();
        if n_ops == 0 {
            continue;
        }

        let mut skip = vec![false; n_ops];
        let mut keep_state = vec![false; n_ops];
        let mut starts: HashMap<usize, Mat> = HashMap::new();
        let mut ends: HashSet<usize> = HashSet::new();

        for (id, act) in &acts {
            let obj = by_id[id];
            let (s, e) = obj.op_range;
            if act.delete {
                for i in s..=e.min(n_ops - 1) {
                    skip[i] = true;
                    if obj.kind == Kind::Text {
                        keep_state[i] = true;
                    }
                }
            } else if act.transformed {
                // The edit is expressed in page space; conjugate by the CTM so
                // the inserted `cm` applies it inside the object's own space.
                let inv = invert(obj.ctm).ok_or("object transform not invertible")?;
                starts.insert(s, compose(compose(obj.ctm, act.a), inv));
                ends.insert(e);
            }
        }

        let mut out: Vec<Operation> = Vec::with_capacity(n_ops + acts.len() * 3);
        let mut restate: Vec<Operation> = Vec::new();
        let mut in_wrapped = false;
        for (i, op) in content.operations.into_iter().enumerate() {
            if let Some(m) = starts.get(&i) {
                out.push(Operation::new("q", vec![]));
                out.push(Operation::new(
                    "cm",
                    m.iter().map(|&v| Object::Real(v as f32)).collect(),
                ));
                in_wrapped = true;
                restate.clear();
            }
            let is_end = ends.contains(&i);
            if skip[i] {
                if keep_state[i] && STICKY_STATE.contains(&op.operator.as_str()) {
                    out.push(op);
                }
            } else {
                if in_wrapped && STICKY_STATE.contains(&op.operator.as_str()) {
                    restate.push(op.clone());
                }
                out.push(op);
            }
            if is_end {
                out.push(Operation::new("Q", vec![]));
                out.append(&mut restate);
                in_wrapped = false;
            }
        }

        let encoded = Content { operations: out }.encode().map_err(err)?;
        crate::common::ensure_contents(doc, pid)?;
        doc.change_page_content(pid, encoded).map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::roundtrip;
    use crate::editor::testing::{close, edit, page1, sample_doc};

    #[test]
    fn delete_removes_image() {
        let mut doc = sample_doc();
        let img_id = enumerate(&doc, page1(&doc))
            .unwrap()
            .iter()
            .find(|o| o.kind == Kind::Image)
            .unwrap()
            .id;
        apply_edits(&mut doc, &[edit(img_id, "delete")]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        assert!(objs.iter().all(|o| o.kind != Kind::Image));
        assert_eq!(objs.len(), 2);
    }

    #[test]
    fn translate_moves_text() {
        let mut doc = sample_doc();
        let before = enumerate(&doc, page1(&doc)).unwrap();
        let t = before.iter().find(|o| o.kind == Kind::Text).unwrap();
        let (tid, bb) = (t.id, t.bbox);
        let mut e = edit(tid, "translate");
        e.dx = 10.0;
        e.dy = 20.0;
        apply_edits(&mut doc, &[e]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        let t2 = objs.iter().find(|o| o.kind == Kind::Text).unwrap();
        for k in 0..4 {
            let want = bb[k] + if k % 2 == 0 { 10.0 } else { 20.0 };
            assert!(close(t2.bbox[k], want), "{:?} vs {:?}", t2.bbox, bb);
        }
    }

    #[test]
    fn scale_image_about_corner() {
        let mut doc = sample_doc();
        let img_id = enumerate(&doc, page1(&doc))
            .unwrap()
            .iter()
            .find(|o| o.kind == Kind::Image)
            .unwrap()
            .id;
        let mut e = edit(img_id, "scale");
        e.sx = 2.0;
        e.sy = 1.0;
        e.ax = 100.0;
        e.ay = 600.0;
        apply_edits(&mut doc, &[e]).unwrap();
        let out = roundtrip(&mut doc);
        let objs = enumerate(&out, page1(&out)).unwrap();
        let img = objs.iter().find(|o| o.kind == Kind::Image).unwrap();
        for (got, want) in img.bbox.iter().zip([100.0, 600.0, 300.0, 650.0]) {
            assert!(close(*got, want), "scaled bbox {:?}", img.bbox);
        }
    }

    #[test]
    fn unknown_action_is_rejected() {
        let mut doc = sample_doc();
        assert!(apply_edits(&mut doc, &[edit(0, "rotate")]).is_err());
        assert!(apply_edits(&mut doc, &[edit(99, "delete")]).is_err());
    }

    #[test]
    fn edits_on_blank_pages_are_ignored() {
        let mut doc = sample_doc();
        let mut e = edit(0, "delete");
        e.src = -1;
        apply_edits(&mut doc, &[e]).unwrap();
        assert_eq!(enumerate(&doc, page1(&doc)).unwrap().len(), 3);
    }
}
