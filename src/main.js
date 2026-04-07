const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

let currentFile = null;
let debounceTimer = null;
let cssDebounceTimer = null;

// Editor state
let editorMode = false;
let pendingSaveAck = false;
let pendingSaveTimeout = null;

const welcomeEl = document.getElementById("welcome");
const toolbarEl = document.getElementById("toolbar");
const filePathEl = document.getElementById("file-path");
const editIndicatorEl = document.getElementById("edit-indicator");
const contentEl = document.getElementById("content");
const splitContainer = document.getElementById("split-container");
const editorPane = document.getElementById("editor-pane");
const editorHost = document.getElementById("editor-host");
const previewPane = document.getElementById("preview-pane");
const previewContent = document.getElementById("preview-content");
const dividerEl = document.getElementById("divider");
const externalChangeBar = document.getElementById("external-change-bar");
const dropOverlay = document.getElementById("drop-overlay");

// ── File loading ──

async function loadFile(path) {
  await invoke("mark_window_occupied").catch(() => {});

  try {
    const html = await invoke("render_markdown", { path });
    currentFile = path;

    contentEl.innerHTML = html;
    welcomeEl.style.display = "none";
    toolbarEl.style.display = "flex";
    contentEl.style.display = "block";

    const fileName = path.split("/").pop();
    filePathEl.textContent = path;
    filePathEl.title = path;

    try {
      await getCurrentWindow().setTitle(`Monocle — ${fileName}`);
    } catch (_) {
      document.title = `Monocle — ${fileName}`;
    }

    await invoke("watch_file", { path });
  } catch (err) {
    contentEl.innerHTML = `<div class="error"><strong>Error:</strong> ${err}</div>`;
    contentEl.style.display = "block";
  }
}

async function reloadFile() {
  if (!currentFile) return;

  // If we just saved, suppress the watcher echo
  if (pendingSaveAck) {
    pendingSaveAck = false;
    clearTimeout(pendingSaveTimeout);
    return;
  }

  if (editorMode) {
    if (MonocleEditor.isDirty()) {
      // External change while user has unsaved edits — show notification
      externalChangeBar.classList.add("visible");
    } else {
      // External change, no unsaved edits — reload into editor silently
      try {
        const text = await invoke("read_file_text", { path: currentFile });
        MonocleEditor.setContent(text);
        const html = await invoke("render_markdown_text", { text });
        previewContent.innerHTML = html;
      } catch (_) {}
    }
    return;
  }

  // Viewer mode — reload from disk
  const scrollY = document.documentElement.scrollTop;
  try {
    const html = await invoke("render_markdown", { path: currentFile });
    contentEl.innerHTML = html;
  } catch (_) {}
  requestAnimationFrame(() => {
    document.documentElement.scrollTop = scrollY;
  });
}

async function openFileDialog() {
  const path = await invoke("pick_file");
  if (path) {
    if (currentFile) {
      await invoke("open_in_new_window", { path });
    } else {
      await loadFile(path);
    }
  }
}

// ── Editor mode ──

async function enterEditMode() {
  if (!currentFile || editorMode) return;

  try {
    const text = await invoke("read_file_text", { path: currentFile });

    // Switch layout
    contentEl.style.display = "none";
    splitContainer.style.display = "flex";

    // Init editor
    MonocleEditor.init(editorHost);
    MonocleEditor.setContent(text);
    MonocleEditor.onContentChange(onEditorChange);
    MonocleEditor.focus();

    // Render initial preview
    const html = await invoke("render_markdown_text", { text });
    previewContent.innerHTML = html;

    editorMode = true;
    editIndicatorEl.classList.add("active");
    editIndicatorEl.textContent = "Editing";
    document.getElementById("edit-toggle").classList.add("active");
    updateDirtyIndicator();
  } catch (err) {
    console.error("Failed to enter edit mode:", err);
  }
}

async function exitEditMode(force) {
  if (!editorMode) return;

  if (!force && MonocleEditor.isDirty()) {
    // Simple confirm — could be replaced with a nicer dialog later
    if (!confirm("You have unsaved changes. Discard?")) {
      return;
    }
  }

  MonocleEditor.destroy();
  externalChangeBar.classList.remove("visible");

  splitContainer.style.display = "none";
  contentEl.style.display = "block";

  editorMode = false;
  editIndicatorEl.classList.remove("active", "dirty");
  document.getElementById("edit-toggle").classList.remove("active");

  // Reload from disk to ensure viewer matches saved state
  if (currentFile) {
    try {
      const html = await invoke("render_markdown", { path: currentFile });
      contentEl.innerHTML = html;
    } catch (_) {}
  }

  const fileName = currentFile ? currentFile.split("/").pop() : "Monocle";
  try {
    await getCurrentWindow().setTitle(`Monocle — ${fileName}`);
  } catch (_) {}
}

async function saveFile() {
  if (!editorMode || !currentFile) return;

  const content = MonocleEditor.getContent();
  try {
    pendingSaveAck = true;
    clearTimeout(pendingSaveTimeout);
    pendingSaveTimeout = setTimeout(() => {
      pendingSaveAck = false;
    }, 500);

    await invoke("write_file", { path: currentFile, content });
    MonocleEditor.markClean();
    updateDirtyIndicator();
  } catch (err) {
    pendingSaveAck = false;
    alert(`Save failed: ${err}`);
  }
}

async function onEditorChange(text) {
  updateDirtyIndicator();
  try {
    const html = await invoke("render_markdown_text", { text });
    previewContent.innerHTML = html;
  } catch (_) {}
}

function updateDirtyIndicator() {
  if (!editorMode) return;
  const dirty = MonocleEditor.isDirty();
  if (dirty) {
    editIndicatorEl.classList.add("dirty");
    editIndicatorEl.textContent = "Editing (unsaved)";
  } else {
    editIndicatorEl.classList.remove("dirty");
    editIndicatorEl.textContent = "Editing";
  }
}

// ── Divider drag ──

let dragging = false;

dividerEl.addEventListener("mousedown", (e) => {
  e.preventDefault();
  dragging = true;
  dividerEl.classList.add("dragging");
  document.body.style.cursor = "col-resize";
  document.body.style.userSelect = "none";
});

document.addEventListener("mousemove", (e) => {
  if (!dragging) return;
  const rect = splitContainer.getBoundingClientRect();
  const ratio = Math.max(0.1, Math.min(0.9, (e.clientX - rect.left) / rect.width));
  editorPane.style.flex = `0 0 ${ratio * 100}%`;
  previewPane.style.flex = `0 0 ${(1 - ratio) * 100 - 1}%`;
});

document.addEventListener("mouseup", () => {
  if (!dragging) return;
  dragging = false;
  dividerEl.classList.remove("dragging");
  document.body.style.cursor = "";
  document.body.style.userSelect = "";
});

// ── Custom CSS ──

async function loadCustomCss() {
  const css = await invoke("load_custom_css");
  let el = document.getElementById("custom-css");
  if (css) {
    if (!el) {
      el = document.createElement("style");
      el.id = "custom-css";
      document.head.appendChild(el);
    }
    el.textContent = css;
  } else if (el) {
    el.remove();
  }
}

// ── External change bar ──

document.getElementById("reload-external").addEventListener("click", async () => {
  externalChangeBar.classList.remove("visible");
  if (!currentFile) return;
  try {
    const text = await invoke("read_file_text", { path: currentFile });
    MonocleEditor.setContent(text);
    const html = await invoke("render_markdown_text", { text });
    previewContent.innerHTML = html;
    updateDirtyIndicator();
  } catch (_) {}
});

document.getElementById("ignore-external").addEventListener("click", () => {
  externalChangeBar.classList.remove("visible");
});

// ── Editor theme toggle ──

const themeToggle = document.getElementById("theme-toggle");
const editorThemes = [
  { id: "light", label: "Light" },
  { id: "solarized-dark", label: "Solarized Dark" },
  { id: "solarized-light", label: "Solarized Light" },
  { id: "tomorrow", label: "Tomorrow" },
  { id: "tomorrow-blue", label: "Tomorrow Blue" },
  { id: "mou-night", label: "Mou Night" },
  { id: "fresh-air", label: "Fresh Air" },
  { id: "writer", label: "Writer" },
];

function applyEditorTheme(themeId) {
  if (themeId && themeId !== "light") {
    editorPane.setAttribute("data-editor-theme", themeId);
  } else {
    editorPane.removeAttribute("data-editor-theme");
  }
  const theme = editorThemes.find((t) => t.id === themeId) || editorThemes[0];
  themeToggle.title = `Editor theme: ${theme.label} (click to cycle)`;
}

function cycleEditorTheme() {
  const current = localStorage.getItem("monocle-editor-theme") || "light";
  const idx = editorThemes.findIndex((t) => t.id === current);
  const next = editorThemes[(idx + 1) % editorThemes.length].id;
  localStorage.setItem("monocle-editor-theme", next);
  applyEditorTheme(next);
}

applyEditorTheme(localStorage.getItem("monocle-editor-theme") || "light");
themeToggle.addEventListener("click", cycleEditorTheme);

// ── Events ──

// Nudge from RunEvent::Opened — re-check pending_files
listen("check-pending-file", async () => {
  if (currentFile) return;
  const path = await invoke("get_window_file");
  if (path) {
    await loadFile(path);
  }
});

// File change watcher
listen("file-changed", () => {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(reloadFile, 150);
});

// CSS change watcher
listen("css-changed", () => {
  clearTimeout(cssDebounceTimer);
  cssDebounceTimer = setTimeout(loadCustomCss, 150);
});

// Drag and drop via Tauri events
let dragDepth = 0;

listen("tauri://drag-enter", () => {
  dragDepth++;
  dropOverlay.classList.add("visible");
});

listen("tauri://drag-leave", () => {
  dragDepth--;
  if (dragDepth <= 0) {
    dragDepth = 0;
    dropOverlay.classList.remove("visible");
  }
});

listen("tauri://drag-drop", async (event) => {
  dragDepth = 0;
  dropOverlay.classList.remove("visible");

  const paths = event.payload.paths || event.payload;
  if (Array.isArray(paths) && paths.length > 0) {
    const mdFile =
      paths.find(
        (p) =>
          p.endsWith(".md") ||
          p.endsWith(".markdown") ||
          p.endsWith(".txt"),
      ) || paths[0];

    if (currentFile) {
      await invoke("open_in_new_window", { path: mdFile });
    } else {
      await loadFile(mdFile);
    }
  }
});

// Keyboard shortcuts
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "o") {
    e.preventDefault();
    openFileDialog();
  }
  if ((e.metaKey || e.ctrlKey) && e.key === "e") {
    e.preventDefault();
    if (editorMode) {
      exitEditMode(false);
    } else {
      enterEditMode();
    }
  }
  if ((e.metaKey || e.ctrlKey) && e.key === "s") {
    if (editorMode) {
      e.preventDefault();
      saveFile();
    }
  }
});

// ── Init ──

window.addEventListener("DOMContentLoaded", async () => {
  document.getElementById("open-btn").addEventListener("click", openFileDialog);
  document.getElementById("edit-toggle").addEventListener("click", () => {
    if (editorMode) {
      exitEditMode(false);
    } else {
      enterEditMode();
    }
  });

  await loadCustomCss();
  await invoke("watch_custom_css").catch(() => {});

  // Check if this window was opened for a specific file
  const windowFile = await invoke("get_window_file");
  if (windowFile) {
    await loadFile(windowFile);
    return;
  }

  // Main window: check for CLI file argument
  const initialFile = await invoke("get_initial_file");
  if (initialFile) {
    await loadFile(initialFile);
    return;
  }

  // No file yet — wait briefly for RunEvent::Opened to deliver one
  setTimeout(() => {
    if (!currentFile) {
      welcomeEl.style.display = "flex";
    }
  }, 200);
});
