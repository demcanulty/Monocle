// frontmatter.rs — document-level stylesheet lookup.
//
// INCLUDED VERBATIM by both `src-tauri/src/lib.rs` (the viewer) and
// `render/render_html.rs` (the PDF exporter) via `include!`. They are separate
// crates, and a document that resolved to different CSS on screen than in its
// PDF would defeat the whole point of the export path — so rather than write
// this twice with a "keep these in sync" comment, there is one copy.
//
// Everything here is fully qualified (`std::path::Path`, not an imported
// `Path`) so it cannot collide with the imports of whichever file includes it.

/// The `css:` value from a document's YAML front matter, if it has one.
///
/// Deliberately NOT a YAML parser. Monocle understands exactly one key, and
/// pulling in a YAML dependency to read it would bloat the render CLI for no
/// benefit. Only the outermost `key: value` lines are considered: a `css:`
/// nested under another key is ignored, as is anything after a `#` comment.
/// Surrounding single or double quotes are stripped.
///
/// Mirrors pulldown-cmark's own rule for a YAML metadata block: it counts only
/// when `---` is the very first line of the document.
fn front_matter_css(md: &str) -> Option<String> {
    let body = md.strip_prefix("---\n").or_else(|| md.strip_prefix("---\r\n"))?;

    for line in body.lines() {
        let trimmed = line.trim_end();
        // Closing fence ends the block; `...` is YAML's explicit end marker.
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        // Indented => nested under some other key, not a top-level `css:`.
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some(rest) = line.strip_prefix("css:") else {
            continue;
        };

        // Strip a trailing `# comment`, but not a `#` inside quotes.
        let mut value = rest.trim();
        if !value.starts_with('"') && !value.starts_with('\'') {
            if let Some(hash) = value.find('#') {
                value = value[..hash].trim_end();
            }
        }
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value)
            .trim();

        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

/// Turn a document's `css:` value into a path on disk.
///
/// Relative values resolve against the directory holding the document, never
/// the process's working directory — that is what lets a folder of docs plus
/// its stylesheet be moved or handed to someone else and still render the same.
/// `~/` expands to the home directory; absolute paths are taken as-is.
///
/// Returns `None` for anything not ending in `.css`. A markdown file can name
/// the stylesheet to inline into the page, so this keeps a document from
/// slurping an unrelated file (`css: ~/.ssh/id_rsa`) into a `<style>` block.
fn resolve_doc_css(doc_path: &std::path::Path, value: &str) -> Option<std::path::PathBuf> {
    if !value.to_ascii_lowercase().ends_with(".css") {
        return None;
    }

    if let Some(rest) = value.strip_prefix("~/") {
        return Some(
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(rest),
        );
    }

    let candidate = std::path::Path::new(value);
    if candidate.is_absolute() {
        return Some(candidate.to_path_buf());
    }
    let dir = doc_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    Some(dir.join(candidate))
}

/// The stylesheet a document asks for, as `(path, contents)`.
///
/// `md` is the document's text — passed in rather than re-read so the editor
/// pane can resolve the front matter of unsaved buffers. A `css:` naming a file
/// that does not exist is not an error: documents get shared around, and a
/// missing stylesheet should degrade to stock styling rather than refuse to
/// render.
fn doc_css(doc_path: &std::path::Path, md: &str) -> Option<(std::path::PathBuf, String)> {
    let value = front_matter_css(md)?;
    let path = resolve_doc_css(doc_path, &value)?;
    let contents = std::fs::read_to_string(&path).ok()?;
    Some((path, contents))
}
