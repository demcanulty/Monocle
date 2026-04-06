use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pulldown_cmark::{html, Options, Parser};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

struct AppState {
    watcher: Mutex<Option<RecommendedWatcher>>,
    css_watcher: Mutex<Option<RecommendedWatcher>>,
}

fn custom_css_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("monocle")
        .join("custom.css")
}

#[tauri::command]
fn render_markdown(path: &str) -> Result<String, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    Ok(html_output)
}

#[tauri::command]
fn watch_file(
    path: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let canonical = fs::canonicalize(&path).map_err(|e| e.to_string())?;

    // Stop existing watcher
    {
        let mut w = state.watcher.lock().unwrap();
        *w = None;
    }

    let file_name: OsString = canonical
        .file_name()
        .ok_or("No file name")?
        .to_os_string();
    let watch_dir = canonical
        .parent()
        .ok_or("No parent directory")?
        .to_path_buf();

    let app_handle = app.clone();
    let target_name = file_name;

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) => {
                        let matches = event
                            .paths
                            .iter()
                            .any(|p| p.file_name().map_or(false, |n| n == target_name));
                        if matches {
                            let _ = app_handle.emit("file-changed", ());
                        }
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;

    let mut w = state.watcher.lock().unwrap();
    *w = Some(watcher);

    Ok(())
}

#[tauri::command]
fn load_custom_css() -> Option<String> {
    let path = custom_css_path();
    if path.is_file() {
        fs::read_to_string(&path).ok()
    } else {
        None
    }
}

#[tauri::command]
fn get_custom_css_path() -> String {
    custom_css_path().to_string_lossy().to_string()
}

#[tauri::command]
fn watch_custom_css(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let css_path = custom_css_path();
    let watch_dir = css_path.parent().ok_or("No parent directory")?.to_path_buf();

    // Don't watch if directory doesn't exist
    if !watch_dir.is_dir() {
        return Ok(());
    }

    let mut w = state.css_watcher.lock().unwrap();
    *w = None;

    let target_name: OsString = css_path
        .file_name()
        .ok_or("No file name")?
        .to_os_string();
    let app_handle = app.clone();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) => {
                        let matches = event
                            .paths
                            .iter()
                            .any(|p| p.file_name().map_or(false, |n| n == target_name));
                        if matches {
                            let _ = app_handle.emit("css-changed", ());
                        }
                    }
                    _ => {}
                }
            }
        })
        .map_err(|e| e.to_string())?;

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| e.to_string())?;

    *w = Some(watcher);
    Ok(())
}

#[tauri::command]
fn pick_file() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn get_initial_file() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.get(1)
        .filter(|p| Path::new(p).is_file())
        .cloned()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AppState {
            watcher: Mutex::new(None),
            css_watcher: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            render_markdown,
            watch_file,
            pick_file,
            get_initial_file,
            load_custom_css,
            get_custom_css_path,
            watch_custom_css,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // Handle files opened via dock drag or Finder "Open With"
        if let tauri::RunEvent::Opened { urls } = event {
            for url in urls {
                if url.scheme() == "file" {
                    if let Ok(path) = url.to_file_path() {
                        let path_str = path.to_string_lossy().to_string();
                        let _ = app_handle.emit("file-opened", path_str);
                    }
                }
            }
        }
    });
}
