// Concatenates several files into one document, pages in the given order.
use lopdf::{dictionary, Document, Object, ObjectId};
use std::collections::BTreeSet;

use crate::common::{err, load_plain, R};
use crate::pages::{pull_down_inherited, strip_signature_widgets};

pub fn merge(paths: &[String]) -> R<Document> {
    if paths.is_empty() {
        return Err("no files to merge".into());
    }
    let mut max_id = 1u32;
    let mut all_pages: Vec<(ObjectId, Object)> = Vec::new();
    let mut all_objects: Vec<(ObjectId, Object)> = Vec::new();
    let mut merged = Document::with_version("1.7");

    for path in paths {
        let mut doc = load_plain(path)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        for (_, oid) in doc.get_pages() {
            pull_down_inherited(&mut doc, oid)?;
            let obj = doc.get_object(oid).map_err(err)?.to_owned();
            all_pages.push((oid, obj));
        }
        all_objects.extend(doc.objects);
    }

    let mut catalog_id: Option<ObjectId> = None;
    let mut pages_id: Option<ObjectId> = None;
    let page_ids: BTreeSet<ObjectId> = all_pages.iter().map(|(id, _)| *id).collect();

    // The first file's Catalog and Pages node become the merged document's;
    // every other file's structural nodes are dropped, their pages reparented.
    for (oid, obj) in all_objects {
        let type_name = obj.type_name().map(|n| n.to_vec()).unwrap_or_default();
        match type_name.as_slice() {
            b"Catalog" => {
                if catalog_id.is_none() {
                    catalog_id = Some(oid);
                    merged.objects.insert(oid, obj);
                }
            }
            b"Pages" => {
                if pages_id.is_none() {
                    pages_id = Some(oid);
                }
            }
            b"Outlines" | b"Outline" => {}
            _ => {
                if !page_ids.contains(&oid) {
                    merged.objects.insert(oid, obj);
                }
            }
        }
    }

    let pages_id = pages_id.ok_or("no Pages node found")?;
    let catalog_id = catalog_id.ok_or("no Catalog found")?;

    for (oid, obj) in &all_pages {
        let mut dict = obj.as_dict().map_err(err)?.clone();
        dict.set("Parent", Object::Reference(pages_id));
        merged.objects.insert(*oid, Object::Dictionary(dict));
    }

    merged.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => all_pages.len() as i64,
            "Kids" => all_pages.iter().map(|(id, _)| Object::Reference(*id)).collect::<Vec<_>>(),
        }),
    );

    {
        let catalog = merged
            .get_object_mut(catalog_id)
            .and_then(|o| o.as_dict_mut())
            .map_err(err)?;
        catalog.set("Pages", Object::Reference(pages_id));
        catalog.remove(b"Outlines");
        catalog.remove(b"Names");
        catalog.remove(b"OpenAction");
        catalog.remove(b"AcroForm");
        catalog.remove(b"PageLabels");
        catalog.remove(b"StructTreeRoot");
        catalog.remove(b"Dests");
    }
    merged.trailer.set("Root", Object::Reference(catalog_id));
    strip_signature_widgets(&mut merged);

    merged.max_id = max_id;
    merged.prune_objects();
    merged.renumber_objects();
    merged.compress();
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::testing::{roundtrip, sample_doc};

    #[test]
    fn merge_concatenates() {
        let dir = std::env::temp_dir();
        let p1 = dir.join("wumpa_test_merge_a.pdf");
        let p2 = dir.join("wumpa_test_merge_b.pdf");
        sample_doc(2).save(&p1).unwrap();
        sample_doc(3).save(&p2).unwrap();
        let mut merged = merge(&[
            p1.to_string_lossy().to_string(),
            p2.to_string_lossy().to_string(),
        ])
        .unwrap();
        let out = roundtrip(&mut merged);
        assert_eq!(out.get_pages().len(), 5);
        let _ = std::fs::remove_file(&p1);
        let _ = std::fs::remove_file(&p2);
    }
}
