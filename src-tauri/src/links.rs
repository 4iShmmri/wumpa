// Link annotations: URL or go-to-page rectangles added and removed on save.
use lopdf::{dictionary, Document, Object, ObjectId};
use serde::Deserialize;

use crate::annots::append_annot;
use crate::common::{err, page_annots, R};

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LinkSpec {
    // 0-based index into the output page order: the page the link sits on.
    pub page: u32,
    // x1, y1, x2, y2 in that page's PDF user space.
    pub rect: [f64; 4],
    pub url: Option<String>,
    // 0-based index into the output page order.
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
    let is_link = d
        .get(b"Subtype")
        .and_then(|o| o.as_name())
        .map(|n| n == b"Link")
        .unwrap_or(false);
    if !is_link {
        return None;
    }
    let r = d.get(b"Rect").and_then(|o| o.as_array()).ok()?;
    let v: Vec<f64> = r
        .iter()
        .filter_map(|o| {
            o.as_float()
                .ok()
                .map(|f| f as f64)
                .or_else(|| o.as_i64().ok().map(|i| i as f64))
        })
        .collect();
    if v.len() == 4 {
        Some([v[0], v[1], v[2], v[3]])
    } else {
        None
    }
}

fn remove_links(doc: &mut Document, page_id: ObjectId, rects: &[[f64; 4]]) -> R<()> {
    let annots = page_annots(doc, page_id);
    if annots.is_empty() {
        return Ok(());
    }
    let kept: Vec<Object> = annots
        .into_iter()
        .filter(|a| match annot_rect(doc, a) {
            Some(r) => !rects.iter().any(|t| rect_near(&r, t)),
            None => true,
        })
        .collect();
    let page = doc
        .get_object_mut(page_id)
        .and_then(|o| o.as_dict_mut())
        .map_err(err)?;
    page.set("Annots", kept);
    Ok(())
}

pub fn apply(
    doc: &mut Document,
    kids: &[ObjectId],
    links: &[LinkSpec],
    removals: &[LinkRemoval],
) -> R<()> {
    for (pi, &page_id) in kids.iter().enumerate() {
        let rects: Vec<[f64; 4]> = removals
            .iter()
            .filter(|r| r.page as usize == pi)
            .map(|r| r.rect)
            .collect();
        if !rects.is_empty() {
            remove_links(doc, page_id, &rects)?;
        }
    }

    for link in links {
        let Some(&page_id) = kids.get(link.page as usize) else {
            continue;
        };
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
        append_annot(doc, page_id, annot_id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};
    use crate::pages::{rebuild_with, Extras, PageSpec};

    #[test]
    fn links_add_and_remove() {
        let mut doc = sample_doc(2);
        let links = vec![
            LinkSpec {
                page: 0,
                rect: [10.0, 10.0, 90.0, 30.0],
                url: Some("https://example.com".into()),
                dest_page: None,
            },
            LinkSpec {
                page: 0,
                rect: [10.0, 40.0, 90.0, 60.0],
                url: None,
                dest_page: Some(1),
            },
        ];
        let specs = [PageSpec::page(1, 0), PageSpec::page(2, 0)];
        let extras = Extras {
            links: &links,
            ..Default::default()
        };
        rebuild_with(&mut doc, &specs, &[], &[], &[], &extras).unwrap();
        let mut out = roundtrip(&mut doc);
        assert_eq!(page_annots(&out, out.get_pages()[&1]).len(), 2);

        // Remove one existing link by its rect on a later save.
        let removals = vec![LinkRemoval {
            page: 0,
            rect: [10.0, 10.0, 90.0, 30.0],
        }];
        let extras2 = Extras {
            link_removals: &removals,
            ..Default::default()
        };
        rebuild_with(&mut out, &specs, &[], &[], &[], &extras2).unwrap();
        let out2 = roundtrip(&mut out);
        let left = page_annots(&out2, out2.get_pages()[&1]);
        assert_eq!(left.len(), 1);
        let d = out2
            .get_object(left[0].as_reference().unwrap())
            .unwrap()
            .as_dict()
            .unwrap();
        let a = out2
            .dereference(d.get(b"A").unwrap())
            .unwrap()
            .1
            .as_dict()
            .unwrap();
        assert_eq!(a.get(b"S").unwrap().as_name().unwrap(), b"GoTo");
    }
}
