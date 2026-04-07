use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pulldown_cmark::{html, Options, Parser};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static WINDOW_ID: AtomicU32 = AtomicU32::new(0);

struct AppState {
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
    css_watcher: Mutex<Option<RecommendedWatcher>>,
    pending_files: Mutex<HashMap<String, String>>,
    idle_windows: Mutex<HashSet<String>>,
}

fn custom_css_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("monocle")
        .join("custom.css")
}

fn create_file_window(app: &AppHandle, path: &str) -> Result<(), String> {
    let id = WINDOW_ID.fetch_add(1, Ordering::Relaxed);
    let label = format!("monocle-{}", id);
    let file_name = Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();

    let state = app.state::<AppState>();
    state
        .pending_files
        .lock()
        .unwrap()
        .insert(label.clone(), path.to_string());

    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("Monocle — {}", file_name))
        .inner_size(900.0, 700.0)
        .min_inner_size(400.0, 300.0)
        .build()
        .map_err(|e| e.to_string())?;

    Ok(())
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
    window: tauri::Window,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let canonical = fs::canonicalize(&path).map_err(|e| e.to_string())?;
    let label = window.label().to_string();

    // Remove existing watcher for this window
    state.watchers.lock().unwrap().remove(&label);

    let file_name: OsString = canonical
        .file_name()
        .ok_or("No file name")?
        .to_os_string();
    let watch_dir = canonical
        .parent()
        .ok_or("No parent directory")?
        .to_path_buf();

    let app_handle = app.clone();
    let target_label = label.clone();
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
                            let _ = app_handle.emit_to(&target_label, "file-changed", ());
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

    state.watchers.lock().unwrap().insert(label, watcher);

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

#[tauri::command]
fn get_window_file(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
) -> Option<String> {
    state
        .pending_files
        .lock()
        .unwrap()
        .remove(window.label())
}

#[tauri::command]
fn open_in_new_window(path: String, app: AppHandle) -> Result<(), String> {
    create_file_window(&app, &path)
}

#[tauri::command]
fn register_idle(window: tauri::Window, state: tauri::State<'_, AppState>) {
    state
        .idle_windows
        .lock()
        .unwrap()
        .insert(window.label().to_string());
}

#[tauri::command]
fn unregister_idle(window: tauri::Window, state: tauri::State<'_, AppState>) {
    state
        .idle_windows
        .lock()
        .unwrap()
        .remove(window.label());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AppState {
            watchers: Mutex::new(HashMap::new()),
            css_watcher: Mutex::new(None),
            pending_files: Mutex::new(HashMap::new()),
            idle_windows: Mutex::new(HashSet::new()),
        })
        .setup(|app| {
            // Pre-register the main window as idle so RunEvent::Opened
            // can target it before the frontend has loaded
            let state = app.state::<AppState>();
            state
                .idle_windows
                .lock()
                .unwrap()
                .insert("main".to_string());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            render_markdown,
            watch_file,
            pick_file,
            get_initial_file,
            get_window_file,
            open_in_new_window,
            register_idle,
            unregister_idle,
            load_custom_css,
            get_custom_css_path,
            watch_custom_css,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Opened { urls } = event {
            let state = app_handle.state::<AppState>();
            for url in urls {
                if url.scheme() == "file" {
                    if let Ok(path) = url.to_file_path() {
                        let path_str = path.to_string_lossy().to_string();

                        // Reuse an idle window if one exists
                        let idle_label = {
                            let mut idle = state.idle_windows.lock().unwrap();
                            let label = idle.iter().next().cloned();
                            if let Some(ref l) = label {
                                idle.remove(l);
                            }
                            label
                        };

                        if let Some(label) = idle_label {
                            // Store for the frontend to pick up on init (cold start)
                            state
                                .pending_files
                                .lock()
                                .unwrap()
                                .insert(label.clone(), path_str.clone());
                            // Also emit for the already-running case
                            let _ = app_handle.emit_to(&label, "file-opened", &path_str);
                        } else {
                            let _ = create_file_window(app_handle, &path_str);
                        }
                    }
                }
            }
        }
    });
}
