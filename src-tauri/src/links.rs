use lopdf::{dictionary, Document, Object, ObjectId};
use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpec {
    /// 0-based index into the output page order: the page the link sits on.
    pub page: u32,
    /// x1, y1, x2, y2 in that page's PDF user space.
    pub rect: [f64; 4],
    pub url: Option<String>,
    /// 0-based index into the output page order.
    pub dest_page: Option<u32>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct LinkRemoval {
    pub page: u32,
    pub rect: [f64; 4],
}

fn rect_near(a: &[f64; 4], b: &[f64; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.5)
}

fn annot_rect(doc: &Document, annot: &Object) -> Option<[f64; 4]> {
    let d = doc.dereference(annot).ok()?.1.as_dict().ok()?;
    let is_link = d.get(b"Subtype").and_then(|o| o.as_name()).map(|n| n == b"Link").unwrap_or(false);
    if !is_link {
        return None;
    }
    let r = d.get(b"Rect").and_then(|o| o.as_array()).ok()?;
    let v: Vec<f64> = r.iter().filter_map(|o| o.as_float().ok().map(|f| f as f64).or_else(|| o.as_i64().ok().map(|i| i as f64))).collect();
    if v.len() == 4 { Some([v[0], v[1], v[2], v[3]]) } else { None }
}

fn remove_links(doc: &mut Document, page_id: ObjectId, rects: &[[f64; 4]]) -> Result<(), String> {
    let annots = match doc.get_object(page_id).and_then(|o| o.as_dict()).and_then(|d| d.get(b"Annots")) {
        Ok(Object::Array(a)) => a.clone(),
        Ok(Object::Reference(r)) => doc.get_object(*r).and_then(|o| o.as_array()).cloned().unwrap_or_default(),
        _ => return Ok(()),
    };
    let kept: Vec<Object> = annots
        .into_iter()
        .filter(|a| match annot_rect(doc, a) {
            Some(r) => !rects.iter().any(|t| rect_near(&r, t)),
            None => true,
        })
        .collect();
    let page = doc.get_object_mut(page_id).and_then(|o| o.as_dict_mut()).map_err(|e| e.to_string())?;
    page.set("Annots", kept);
    Ok(())
}

pub fn apply(
    doc: &mut Document,
    kids: &[ObjectId],
    links: &[LinkSpec],
    removals: &[LinkRemoval],
) -> Result<(), String> {
    for (pi, &page_id) in kids.iter().enumerate() {
        let rects: Vec<[f64; 4]> = removals.iter().filter(|r| r.page as usize == pi).map(|r| r.rect).collect();
        if !rects.is_empty() {
            remove_links(doc, page_id, &rects)?;
        }
    }

    for link in links {
        let Some(&page_id) = kids.get(link.page as usize) else { continue };
        let action = if let Some(url) = link.url.as_deref().filter(|u| !u.is_empty()) {
            dictionary! { "S" => "URI", "URI" => Object::string_literal(url.as_bytes().to_vec()) }
        } else if let Some(dest) = link.dest_page.and_then(|p| kids.get(p as usize).copied()) {
            dictionary! { "S" => "GoTo", "D" => vec![dest.into(), "Fit".into()] }
        } else {
            continue;
        };
        let action_id = doc.add_object(action);
        let r = link.rect;
        let annot = dictionary! {
            "Type" => "Annot",
            "Subtype" => "Link",
            "Rect" => vec![r[0].into(), r[1].into(), r[2].into(), r[3].into()],
            "Border" => vec![0.into(), 0.into(), 0.into()],
            "A" => Object::Reference(action_id),
        };
        let annot_id = doc.add_object(annot);
        crate::pdf_ops::append_annot(doc, page_id, annot_id)?;
    }
    Ok(())
}
