mod annots;
mod common;
mod compose;
mod editor;
mod export;
mod links;
mod media;
mod merge;
mod ocr;
mod outline;
mod pages;
mod scan;
mod security;
mod stamps;

use annots::{Highlight, Note, Shape};
use editor::ObjectEdit;
use links::{LinkRemoval, LinkSpec};
use outline::OutlineNode;
use pages::{Extras, PageSpec};
use serde::Deserialize;
use stamps::Stamp;
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
    let mut doc = common::load_plain(src_path)?;
    editor::apply_edits(&mut doc, edits)?;
    let extras = Extras { stamps, ..*extras };
    pages::rebuild_with(&mut doc, pages, highlights, notes, shapes, &extras)?;
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
        stamps: &stamps,
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
        &src_path,
        &path,
        &pages,
        &highlights,
        &notes,
        &edits,
        &stamps,
        &shapes,
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
async fn list_page_objects(src_path: String, page: i32) -> Result<Vec<editor::ObjDesc>, String> {
    // Blank pages (page <= 0) only exist in the frontend and hold no objects.
    if page <= 0 {
        return Ok(Vec::new());
    }
    let doc = common::load_plain(&src_path)?;
    let pages = doc.get_pages();
    let pid = *pages
        .get(&(page as u32))
        .ok_or_else(|| format!("page {page} not found"))?;
    Ok(editor::enumerate(&doc, pid)?
        .iter()
        .map(editor::ObjDesc::from)
        .collect())
}

fn preview_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("wumpa-preview")
}

#[tauri::command]
async fn apply_edits_preview(src_path: String, edits: Vec<ObjectEdit>) -> Result<String, String> {
    let mut doc = common::load_plain(&src_path)?;
    editor::apply_edits(&mut doc, &edits)?;
    let dir = preview_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(unique_pdf_name());
    doc.save(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

// Previews are replaced on every edit; the frontend hands back the one it no longer shows.
#[tauri::command]
async fn discard_preview(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if p.parent() != Some(preview_dir().as_path()) {
        return Err("not a preview file".into());
    }
    let _ = std::fs::remove_file(p);
    Ok(())
}

#[tauri::command]
async fn merge_documents(paths: Vec<String>, out_path: String) -> Result<(), String> {
    let mut doc = merge::merge(&paths)?;
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
        let mut doc = common::load_plain(&src_path)?;
        let specs: Vec<PageSpec> = part
            .pages
            .iter()
            .map(|&p| PageSpec::page(p as i32, 0))
            .collect();
        pages::rebuild(&mut doc, &specs, &[], &[], &[])?;
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
    let mut doc = media::blank_document(
        w.clamp(72.0, 5000.0),
        h.clamp(72.0, 5000.0),
        pages.clamp(1, 50),
    );
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
async fn image_only_pages(path: String) -> Result<Vec<u32>, String> {
    let doc = common::load_plain(&path)?;
    Ok(scan::page_image::image_only_pages(&doc))
}

#[tauri::command]
async fn rebuild_scanned(
    app: tauri::AppHandle,
    src_path: String,
    out_path: String,
    pages: Vec<u32>,
    langs: Vec<String>,
) -> Result<usize, String> {
    use tauri::Emitter;
    scan::rebuild::rebuild_scanned(&src_path, &out_path, &pages, langs, &mut |p| {
        let _ = app.emit("scan-progress", p);
    })
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ExportProgress {
    page: u32,
    stage: String,
}

// Analyses one page for export; `scan-progress`-style stages go out as
// `export-progress` so the UI can show what is happening on slow pages.
#[tauri::command]
async fn export_page(
    app: tauri::AppHandle,
    src_path: String,
    input: export::PageInput,
    langs: Vec<String>,
) -> Result<export::model::Page, String> {
    use tauri::Emitter;
    let page = match &input {
        export::PageInput::Native { page, .. } | export::PageInput::Scanned { page } => *page,
    };
    let mut cb = |stage: &str, _: usize, _: usize| {
        let _ = app.emit(
            "export-progress",
            ExportProgress {
                page,
                stage: stage.to_string(),
            },
        );
    };
    export::analyse_input(&src_path, input, langs, &mut scan::Progress(&mut cb))
}

#[tauri::command]
async fn export_write(
    pages: Vec<export::model::Page>,
    format: String,
    out_path: String,
    lang: String,
) -> Result<(), String> {
    export::write(&pages, export::Format::parse(&format)?, &out_path, &lang)
}

#[tauri::command]
async fn compose_pdf(pages: Vec<compose::PageBlocks>, out_path: String) -> Result<(), String> {
    let mut doc = compose::compose_document(&pages)?;
    doc.save(&out_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn encrypt_pdf(
    src_path: String,
    out_path: String,
    user_password: String,
    owner_password: String,
    permissions: security::Permissions,
    policy: Option<security::Policy>,
) -> Result<(), String> {
    security::encrypt_file(
        &src_path,
        &out_path,
        &user_password,
        &owner_password,
        permissions,
        policy,
    )
}

fn ledger_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[tauri::command]
async fn password_status(
    app: tauri::AppHandle,
    path: String,
) -> Result<security::AttemptStatus, String> {
    security::attempt_status(&ledger_dir(&app)?, &path)
}

#[tauri::command]
async fn password_attempt(
    app: tauri::AppHandle,
    path: String,
    ok: bool,
) -> Result<security::AttemptStatus, String> {
    security::record_attempt(&ledger_dir(&app)?, &path, ok)
}

#[tauri::command]
async fn security_info(path: String) -> Result<security::SecurityInfo, String> {
    security::info(&path)
}

#[tauri::command]
async fn remove_security(
    src_path: String,
    out_path: String,
    password: String,
) -> Result<(), String> {
    security::remove_file(&src_path, &out_path, &password)
}

#[derive(serde::Serialize)]
struct AutotestPlan {
    src: String,
    out: String,
}

// Debug builds only: with WUMPA_AUTOTEST_SRC and WUMPA_AUTOTEST_OUT set,
// the window opens the file, exports it and quits (see tools/autotest.sh).
#[tauri::command]
fn autotest_plan() -> Option<AutotestPlan> {
    if !cfg!(debug_assertions) {
        return None;
    }
    Some(AutotestPlan {
        src: std::env::var("WUMPA_AUTOTEST_SRC").ok()?,
        out: std::env::var("WUMPA_AUTOTEST_OUT").ok()?,
    })
}

#[tauri::command]
fn take_pending_files(state: tauri::State<'_, PendingFiles>) -> Vec<String> {
    state
        .0
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

// Appends a line to the app's log file, so failures seen in the window
// can be read back later.
#[tauri::command]
fn log_line(app: tauri::AppHandle, line: String) {
    use tauri::Manager;
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("wumpa.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "{stamp} {line}");
    }
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn is_pdf_path(arg: &str) -> bool {
    arg.to_lowercase().ends_with(".pdf") && std::path::Path::new(arg).exists()
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
            let args: Vec<String> = std::env::args()
                .skip(1)
                .filter(|a| is_pdf_path(a))
                .collect();
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
            autotest_plan,
            exit_app,
            list_page_objects,
            apply_edits_preview,
            discard_preview,
            print_document,
            save_png,
            compress_pdf,
            create_blank_pdf,
            read_image_b64,
            compose_pdf,
            image_only_pages,
            rebuild_scanned,
            export_page,
            export_write,
            log_line,
            encrypt_pdf,
            security_info,
            remove_security,
            password_status,
            password_attempt,
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
