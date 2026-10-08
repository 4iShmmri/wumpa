mod pdf_ops;

use pdf_ops::{Highlight, Note, PageSpec};
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

#[tauri::command]
async fn save_document(
    src_path: String,
    out_path: String,
    pages: Vec<PageSpec>,
    highlights: Vec<Highlight>,
    notes: Vec<Note>,
) -> Result<(), String> {
    let mut doc = lopdf::Document::load(&src_path).map_err(|e| e.to_string())?;
    pdf_ops::rebuild(&mut doc, &pages, &highlights, &notes)?;
    doc.save(&out_path).map_err(|e| e.to_string())?;
    Ok(())
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
            .map(|&p| PageSpec { src: p, rotation: 0 })
            .collect();
        pdf_ops::rebuild(&mut doc, &specs, &[], &[])?;
        doc.save(&part.out_path).map_err(|e| e.to_string())?;
    }
    Ok(())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(PendingFiles(Mutex::new(Vec::new())))
        .setup(|app| {
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
            exit_app
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
