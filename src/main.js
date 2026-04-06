const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const { getCurrentWindow } = window.__TAURI__.window;

let currentFile = null;
let debounceTimer = null;
let cssDebounceTimer = null;

const welcomeEl = document.getElementById("welcome");
const toolbarEl = document.getElementById("toolbar");
const filePathEl = document.getElementById("file-path");
const contentEl = document.getElementById("content");
const dropOverlay = document.getElementById("drop-overlay");

async function loadFile(path) {
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
  const scrollY = document.documentElement.scrollTop;
  try {
    const html = await invoke("render_markdown", { path: currentFile });
    contentEl.innerHTML = html;
  } catch (_) {
    // File might be mid-write; ignore and wait for next event
  }
  requestAnimationFrame(() => {
    document.documentElement.scrollTop = scrollY;
  });
}

async function openFileDialog() {
  const path = await invoke("pick_file");
  if (path) {
    await loadFile(path);
  }
}

// Custom CSS
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

// Files opened via dock drag or Finder "Open With"
listen("file-opened", async (event) => {
  const path = event.payload;
  if (path) {
    await loadFile(path);
  }
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
    // Prefer .md files, fall back to first file
    const mdFile =
      paths.find(
        (p) =>
          p.endsWith(".md") ||
          p.endsWith(".markdown") ||
          p.endsWith(".txt"),
      ) || paths[0];
    await loadFile(mdFile);
  }
});

// Keyboard shortcut
document.addEventListener("keydown", (e) => {
  if ((e.metaKey || e.ctrlKey) && e.key === "o") {
    e.preventDefault();
    openFileDialog();
  }
});

// Init
window.addEventListener("DOMContentLoaded", async () => {
  document.getElementById("open-btn").addEventListener("click", openFileDialog);

  // Load custom CSS and start watching it
  await loadCustomCss();
  await invoke("watch_custom_css").catch(() => {});

  // Check for CLI file argument
  const initialFile = await invoke("get_initial_file");
  if (initialFile) {
    await loadFile(initialFile);
  }
});
