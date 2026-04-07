use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use pulldown_cmark::{html, Options, Parser};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

static WINDOW_ID: AtomicU32 = AtomicU32::new(0);

/// True until the main window loads a file (via any mechanism).
/// Lock-free — avoids the Mutex timing issues that plagued idle_windows.
static MAIN_AVAILABLE: AtomicBool = AtomicBool::new(true);

struct AppState {
    watchers: Mutex<HashMap<String, RecommendedWatcher>>,
    css_watcher: Mutex<Option<RecommendedWatcher>>,
    pending_files: Mutex<HashMap<String, String>>,
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

fn render_to_html(content: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let parser = Parser::new_ext(content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    html_output
}

#[tauri::command]
fn render_markdown(path: &str) -> Result<String, String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    Ok(render_to_html(&content))
}

#[tauri::command]
fn render_markdown_text(text: &str) -> String {
    render_to_html(text)
}

#[tauri::command]
fn read_file_text(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))
}

#[tauri::command]
fn write_file(path: &str, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("Failed to write file: {}", e))
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

/// Called by the frontend when it loads a file — marks the main window as occupied.
#[tauri::command]
fn mark_window_occupied(window: tauri::Window) {
    if window.label() == "main" {
        MAIN_AVAILABLE.store(false, Ordering::SeqCst);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .menu(|handle| {
            let about = PredefinedMenuItem::about(
                handle,
                Some("About Monocle"),
                Some(AboutMetadata {
                    copyright: Some("Dan McAnulty — demcanulty@gmail.com".into()),
                    icon: Some(Image::from_bytes(include_bytes!("../icons/icon.png")).unwrap()),
                    ..Default::default()
                }),
            )?;
            let app_menu = Submenu::with_items(
                handle,
                "Monocle",
                true,
                &[
                    &about,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::services(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::hide(handle, None)?,
                    &PredefinedMenuItem::hide_others(handle, None)?,
                    &PredefinedMenuItem::show_all(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::quit(handle, None)?,
                ],
            )?;
            let file_menu = Submenu::with_items(
                handle,
                "File",
                true,
                &[&PredefinedMenuItem::close_window(handle, None)?],
            )?;
            let edit_menu = Submenu::with_items(
                handle,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(handle, None)?,
                    &PredefinedMenuItem::redo(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::cut(handle, None)?,
                    &PredefinedMenuItem::copy(handle, None)?,
                    &PredefinedMenuItem::paste(handle, None)?,
                    &PredefinedMenuItem::select_all(handle, None)?,
                ],
            )?;
            let window_menu = Submenu::with_items(
                handle,
                "Window",
                true,
                &[
                    &PredefinedMenuItem::minimize(handle, None)?,
                    &PredefinedMenuItem::maximize(handle, None)?,
                    &PredefinedMenuItem::fullscreen(handle, None)?,
                ],
            )?;
            Menu::with_items(handle, &[&app_menu, &file_menu, &edit_menu, &window_menu])
        })
        .manage(AppState {
            watchers: Mutex::new(HashMap::new()),
            css_watcher: Mutex::new(None),
            pending_files: Mutex::new(HashMap::new()),
        })
        .invoke_handler(tauri::generate_handler![
            render_markdown,
            render_markdown_text,
            read_file_text,
            write_file,
            watch_file,
            pick_file,
            get_initial_file,
            get_window_file,
            open_in_new_window,
            mark_window_occupied,
            load_custom_css,
            get_custom_css_path,
            watch_custom_css,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, _event| {
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Opened { urls } = _event {
            let state = _app_handle.state::<AppState>();
            for url in urls {
                if url.scheme() == "file" {
                    if let Ok(path) = url.to_file_path() {
                        let path_str = path.to_string_lossy().to_string();

                        // Try to reuse the main window if it hasn't loaded a file yet
                        if MAIN_AVAILABLE.swap(false, Ordering::SeqCst) {
                            state
                                .pending_files
                                .lock()
                                .unwrap()
                                .insert("main".to_string(), path_str);
                            let _ = _app_handle
                                .emit_to("main", "check-pending-file", ());
                        } else {
                            let _ = create_file_window(_app_handle, &path_str);
                        }
                    }
                }
            }
        }
    });
}
