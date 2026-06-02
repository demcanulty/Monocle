# Monocle

A standalone markdown viewer with live reload, built with [Tauri](https://tauri.app/).

Renders `.md` files with GitHub-style formatting. Watches the file for changes and auto-reloads on save — useful for previewing documentation as you write it.

## Features

- Live reload on file save (debounced file watcher)
- Native macOS file dialogs (Cmd+O)
- Drag-and-drop `.md` files onto the window or dock icon
- Dark mode (follows system preference)
- Custom CSS via `~/.config/monocle/custom.css`
- Registered as a macOS handler for `.md` files
- Headless PDF export that matches the on-screen view (`bin/monocle-render`)

## Usage

Open a file from the welcome screen, or from the command line:

```
monocle path/to/file.md
```

You can also drag and drop `.md` files onto the window, or right-click a markdown file in Finder and choose Open With → Monocle.

## Headless PDF export

Render a `.md` file to a PDF that matches the on-screen view — no window, no
print dialog — so it can be scripted and batched:

```
bin/monocle-render FILE.md [MORE.md ...]   # -> <dir>/pdf/<name>.md.pdf
bin/monocle-render -o OUT.pdf FILE.md      # explicit output path
bin/monocle-render *.md                     # batch via shell glob
```

It reuses Monocle's own markdown→HTML conversion and `styles.css` (including the
`@media print` rules), and keeps cross-document links between rendered PDFs
clickable. Requires Google Chrome (or Chromium; set `$CHROME`) and `cargo`. See
[`render/README.md`](render/README.md) for how it works and the design notes.

## Custom Styles

Create `~/.config/monocle/custom.css` to override the default rendering. Changes are applied live. All CSS variables and `#content` selectors from the built-in stylesheet can be overridden.

## Building

Requires [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/).

```
npm install
npm run tauri build
```

The built app is at `src-tauri/target/release/bundle/macos/Monocle.app`.

## Stack

- **Backend**: Rust — file I/O, markdown parsing ([pulldown-cmark](https://crates.io/crates/pulldown-cmark)), file watching ([notify](https://crates.io/crates/notify))
- **Frontend**: Vanilla HTML/CSS/JS in macOS WebKit
- **Framework**: Tauri v2
