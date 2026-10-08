mod editor;
mod links;
mod media;
mod outline;
mod pdf_ops;
mod security;
mod signing;
mod stamps;

use editor::ObjectEdit;
use links::{LinkRemoval, LinkSpec};
use outline::OutlineNode;
use pdf_ops::{Extras, Highlight, Note, PageSpec, Shape};
use stamps::Stamp;
use serde::Deserialize;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

struct PendingFiles(Mutex<Vec<String>>);

#[tauri::command]
async fn load_pdf(path: String) -> Result<tauri::ipc::Response, String> {
    std::fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
fn apply_all_and_save(
    src_path: &str,
    out_path: &std::path::Path,
    pages: &[PageSpec],
    highlights: &[Highlight],
    notes: &[Note],
    edits: &[ObjectEdit],
    stamps: &[Stamp],
    shapes: &[Shape],
    extras: &Extras,
) -> Result<(), String> {
    let mut doc = lopdf::Document::load(src_path).map_err(|e| e.to_string())?;
    editor::apply_edits(&mut doc, edits)?;
    stamps::apply_stamps(&mut doc, stamps)?;
    pdf_ops::rebuild_with(&mut doc, pages, highlights, notes, shapes, extras)?;
    doc.save(out_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn save_document(
    src_path: String,
    out_path: String,
    pages: Vec<PageSpec>,
    highlights: Vec<Highlight>,
    notes: Vec<Note>,
    edits: Vec<ObjectEdit>,
    stamps: Vec<Stamp>,
    shapes: Vec<Shape>,
    outline: Option<Vec<OutlineNode>>,
    links: Option<Vec<LinkSpec>>,
    link_removals: Option<Vec<LinkRemoval>>,
) -> Result<(), String> {
    let links = links.unwrap_or_default();
    let link_removals = link_removals.unwrap_or_default();
    let extras = Extras {
        outline: outline.as_deref(),
        links: &links,
        link_removals: &link_removals,
    };
    apply_all_and_save(
        &src_path,
        std::path::Path::new(&out_path),
        &pages,
        &highlights,
        &notes,
        &edits,
        &stamps,
        &shapes,
        &extras,
    )
}

fn print_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("wumpa-print")
}

fn unique_pdf_name() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}.pdf", std::process::id(), nanos)
}

#[tauri::command]
async fn print_document(
    src_path: String,
    pages: Vec<PageSpec>,
    highlights: Vec<Highlight>,
    notes: Vec<Note>,
    edits: Vec<ObjectEdit>,
    stamps: Vec<Stamp>,
    shapes: Vec<Shape>,
) -> Result<String, String> {
    let dir = print_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(unique_pdf_name());
    apply_all_and_save(
        &src_path, &path, &pages, &highlights, &notes, &edits, &stamps, &shapes,
        &Extras::default(),
    )?;
    tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn save_png(path: String, data_b64: String) -> Result<(), String> {
    if !path.to_lowercase().ends_with(".png") {
        return Err("path must end in .png".into());
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_b64.as_bytes())
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_page_objects(src_path: String, page: u32) -> Result<Vec<editor::ObjDesc>, String> {
    let doc = lopdf::Document::load(&src_path).map_err(|e| e.to_string())?;
    let pages = doc.get_pages();
    let pid = *pages
        .get(&page)
        .ok_or_else(|| format!("page {page} not found"))?;
    Ok(editor::enumerate(&doc, pid)?.iter().map(editor::ObjDesc::from).collect())
}

fn preview_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("pdf-editor-preview")
}

#[tauri::command]
async fn apply_edits_preview(src_path: String, edits: Vec<ObjectEdit>) -> Result<String, String> {
    let mut doc = lopdf::Document::load(&src_path).map_err(|e| e.to_string())?;
    editor::apply_edits(&mut doc, &edits)?;
    let dir = preview_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = dir.join(format!("{}-{}.pdf", std::process::id(), nanos));
    doc.save(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn merge_documents(paths: Vec<String>, out_path: String) -> Result<(), String> {
    let mut doc = pdf_ops::merge(&paths)?;
    doc.save(&out_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Deserialize)]
struct SplitPart {
    pages: Vec<u32>,
    out_path: String,
}

#[tauri::command]
async fn split_document(src_path: String, parts: Vec<SplitPart>) -> Result<(), String> {
    for part in parts {
        let mut doc = lopdf::Document::load(&src_path).map_err(|e| e.to_string())?;
        let specs: Vec<PageSpec> = part
            .pages
            .iter()
            .map(|&p| PageSpec::page(p, 0))
            .collect();
        pdf_ops::rebuild(&mut doc, &specs, &[], &[], &[])?;
        doc.save(&part.out_path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn compress_pdf(
    src_path: String,
    out_path: String,
    quality: u8,
    max_dim: u32,
) -> Result<[u64; 2], String> {
    media::compress_file(&src_path, &out_path, quality, max_dim)
}

fn new_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("wumpa-new")
}

#[tauri::command]
async fn create_blank_pdf(w: f64, h: f64, pages: u32) -> Result<String, String> {
    let mut doc = media::blank_document(w.clamp(72.0, 5000.0), h.clamp(72.0, 5000.0), pages.clamp(1, 50));
    let dir = new_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(unique_pdf_name());
    doc.save(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
async fn read_image_b64(path: String) -> Result<String, String> {
    media::image_file_to_data_url(&path)
}

#[tauri::command]
async fn encrypt_pdf(
    src_path: String,
    out_path: String,
    user_password: String,
    owner_password: String,
    permissions: security::Permissions,
) -> Result<(), String> {
    security::encrypt_file(&src_path, &out_path, &user_password, &owner_password, permissions)
}

#[tauri::command]
async fn security_info(path: String) -> Result<security::SecurityInfo, String> {
    security::info(&path)
}

#[tauri::command]
async fn remove_security(src_path: String, out_path: String, password: String) -> Result<(), String> {
    security::remove_file(&src_path, &out_path, &password)
}

#[tauri::command]
async fn create_digital_id(req: signing::NewIdRequest) -> Result<signing::IdInfo, String> {
    signing::create_id(&req)
}

#[tauri::command]
async fn digital_id_info(p12_path: String, password: String) -> Result<signing::IdInfo, String> {
    signing::id_info(&p12_path, &password)
}

#[tauri::command]
async fn sign_pdf(req: signing::SignRequest) -> Result<(), String> {
    signing::sign_file(&req)
}

#[tauri::command]
async fn verify_signatures(path: String) -> Result<Vec<signing::SigInfo>, String> {
    signing::verify_file(&path)
}

#[tauri::command]
fn take_pending_files(state: tauri::State<'_, PendingFiles>) -> Vec<String> {
    state
        .0
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn is_pdf_path(arg: &str) -> bool {
    arg.to_lowercase().ends_with(".pdf") && std::path::Path::new(arg).exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn save_png_roundtrip_and_extension_check() {
        let bytes: Vec<u8> = vec![137, 80, 78, 71, 1, 2, 3, 4];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let path = std::env::temp_dir().join(format!("wumpa-test-{}.png", std::process::id()));
        let path_s = path.to_string_lossy().to_string();
        save_png(path_s.clone(), b64.clone()).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_file(&path);
        assert!(save_png(path_s.replace(".png", ".jpg"), b64).is_err());
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(PendingFiles(Mutex::new(Vec::new())))
        .setup(|app| {
            let _ = std::fs::remove_dir_all(preview_dir());
            let _ = std::fs::remove_dir_all(print_dir());
            let _ = std::fs::remove_dir_all(new_dir());
            let args: Vec<String> = std::env::args().skip(1).filter(|a| is_pdf_path(a)).collect();
            if !args.is_empty() {
                if let Ok(mut v) = app.state::<PendingFiles>().0.lock() {
                    v.extend(args);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.emit("close-requested", ());
            }
        })
        .invoke_handler(tauri::generate_handler![
            load_pdf,
            save_document,
            merge_documents,
            split_document,
            take_pending_files,
            exit_app,
            list_page_objects,
            apply_edits_preview,
            print_document,
            save_png,
            compress_pdf,
            create_blank_pdf,
            read_image_b64,
            encrypt_pdf,
            security_info,
            remove_security,
            create_digital_id,
            digital_id_info,
            sign_pdf,
            verify_signatures
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = _event {
                let paths: Vec<String> = urls
                    .into_iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                if !paths.is_empty() {
                    if let Ok(mut v) = _app.state::<PendingFiles>().0.lock() {
                        v.extend(paths);
                    }
                    if let Some(w) = _app.get_webview_window("main") {
                        let _ = w.emit("open-files-pending", ());
                    }
                }
            }
        });
}
