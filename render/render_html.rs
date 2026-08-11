// render_html — Monocle's headless markdown -> HTML step for PDF export.
//
// It produces the SAME HTML the Monocle viewer shows on screen:
//   * the identical pulldown-cmark options the GUI uses,
//   * the project's real `src/styles.css` (embedded at build time) followed by
//     the user's `~/.config/monocle/custom.css` when it exists — the same two
//     stylesheets, in the same order, that the viewer loads (styles.css via
//     <link>, custom.css appended to <head> by `loadCustomCss` in
//     src/main.js), and
//   * the same `<article class="md-rendered">` element the GUI renders into.
//
// The output is then handed to a headless browser to make the PDF
// (see ../bin/monocle-render). Two PDF-specific additions are made here, both
// invisible on screen:
//   * a `<base href>` so relative links/images resolve to the source file's
//     directory, and
//   * relative `.md` / `.markdown` links rewritten to `.md.pdf` so cross-
//     document links open the sibling PDFs.
//
// FIDELITY NOTE: the pulldown-cmark options below MUST stay in sync with
// `monocle_lib::render_to_html`, and `custom_css_path` with
// `monocle_lib::custom_css_path` (both src-tauri/src/lib.rs). Those, plus the
// shared styles.css, are what make the PDF look like the viewer.

use pulldown_cmark::{html, Options, Parser};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const EMBEDDED_CSS: &str = include_str!("../src/styles.css");

/// Where the user's optional overlay stylesheet lives.
///
/// FIDELITY NOTE: must resolve to exactly the same path as
/// `monocle_lib::custom_css_path` (src-tauri/src/lib.rs) — if the viewer and
/// the PDF ever disagreed about this location, a document could look correct on
/// screen and export with different styling. Same crate, same fallback.
fn custom_css_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("monocle")
        .join("custom.css")
}

include!("../shared/frontmatter.rs");

/// Markdown -> body HTML, using the exact options Monocle's viewer uses.
fn render_body(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    // Front matter is metadata, not content: push_html treats a metadata block
    // as non-writing, so enabling this both strips the `---` block from the
    // output and lets `doc_css` read the `css:` key out of it.
    options.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    let parser = Parser::new_ext(md, options);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

/// Rewrite relative links to local markdown documents so they point at the
/// rendered PDF. A `foo.md` / `foo.markdown` link becomes `foo.md.pdf`; a link
/// already written as `foo.md.pdf` is treated as the same target.
///
/// When `link_dir` is `None` the rewritten target stays relative (the PDFs sit
/// beside the source). When `link_dir` is set — the directory the PDFs are
/// actually written to, e.g. a `pdf/` subfolder — the target becomes an
/// absolute `file://` link into that directory, since a relative link would no
/// longer resolve from the new location. Either way the PDFs are assumed to
/// share one flat output directory, so only the file name is used.
///
/// Left untouched: absolute URLs (`http(s)://…`), `mailto:` / `tel:`,
/// root-absolute paths (`/…`), pure `#anchors`, and any non-markdown target
/// (images, etc.), which keep resolving against `<base href>` (the source dir).
fn rewrite_md_links(html: &str, link_dir: Option<&str>) -> String {
    const NEEDLE: &str = "href=\"";
    let mut out = String::with_capacity(html.len() + 64);
    let mut rest = html;
    while let Some(i) = rest.find(NEEDLE) {
        out.push_str(&rest[..i + NEEDLE.len()]);
        rest = &rest[i + NEEDLE.len()..];
        match rest.find('"') {
            Some(end) => {
                out.push_str(&rewrite_one(&rest[..end], link_dir));
                out.push('"');
                rest = &rest[end + 1..];
            }
            None => {
                out.push_str(rest); // malformed; emit verbatim
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn rewrite_one(val: &str, link_dir: Option<&str>) -> String {
    let (path, frag) = match val.find('#') {
        Some(p) => (&val[..p], &val[p..]),
        None => (val, ""),
    };
    if path.is_empty()                                   // pure #anchor
        || path.contains("://")                          // http(s), file, …
        || path.starts_with("mailto:")
        || path.starts_with("tel:")
        || path.starts_with('/')                         // root-absolute
    {
        return val.to_string();
    }
    let lower = path.to_ascii_lowercase();
    // The rendered-PDF target this link refers to, if it is a markdown doc link.
    let pdf_rel = if lower.ends_with(".md") || lower.ends_with(".markdown") {
        format!("{path}.pdf")
    } else if lower.ends_with(".md.pdf") || lower.ends_with(".markdown.pdf") {
        path.to_string()
    } else {
        return val.to_string(); // not a markdown doc link
    };

    match link_dir {
        None => format!("{pdf_rel}{frag}"), // beside the source — keep relative
        Some(dir) => {
            let name = pdf_rel.rsplit('/').next().unwrap_or(&pdf_rel);
            format!("file://{}/{}{}", encode_path_for_url(dir), name, frag)
        }
    }
}

/// Minimal percent-encoding for the characters that would otherwise break a
/// `file://` URL used in `<base href>`. Slashes are preserved.
fn encode_path_for_url(p: &str) -> String {
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        match c {
            ' ' => s.push_str("%20"),
            '#' => s.push_str("%23"),
            '?' => s.push_str("%3F"),
            '%' => s.push_str("%25"),
            _ => s.push(c),
        }
    }
    s
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render_html <input.md> <output.html> [--link-dir DIR] [--css FILE]");
        eprintln!("                   [--custom-css FILE | --no-custom-css] [--no-doc-css]");
        eprintln!("  --link-dir DIR     directory the rendered PDFs live in; markdown");
        eprintln!("                     cross-links are pointed there (default: beside source).");
        eprintln!("  --css FILE         REPLACE the base stylesheet (default: embedded");
        eprintln!("                     src/styles.css).");
        eprintln!("  --custom-css FILE  OVERLAY this stylesheet on top of the base, the way");
        eprintln!("                     the viewer layers ~/.config/monocle/custom.css.");
        eprintln!("  --no-custom-css    skip the default ~/.config/monocle/custom.css overlay.");
        eprintln!("  --no-doc-css       ignore the `css:` key in the document's front matter.");
        eprintln!();
        eprintln!("Stylesheets cascade least- to most-specific:");
        eprintln!("  1. src/styles.css            built in, or --css FILE");
        eprintln!("  2. ~/.config/monocle/custom.css   your machine; or --custom-css FILE");
        eprintln!("  3. the document's front-matter `css:`   ships with the document");
        eprintln!("--css suppresses layer 2 by default (it means \"I control the stylesheet\");");
        eprintln!("pass --custom-css alongside it to opt back in. --no-custom-css drops layer 2,");
        eprintln!("--no-doc-css drops layer 3; pass both for stock styling.");
        return ExitCode::from(2);
    }
    let md_path = &args[1];
    let out_path = &args[2];

    let mut link_dir: Option<String> = None;
    let mut css_path: Option<String> = None;
    let mut custom_css_arg: Option<String> = None;
    let mut no_custom_css = false;
    let mut no_doc_css = false;
    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--link-dir" => match args.get(i + 1) {
                Some(v) => { link_dir = Some(v.clone()); i += 2; }
                None => { eprintln!("render_html: --link-dir needs a value"); return ExitCode::from(2); }
            },
            "--css" => match args.get(i + 1) {
                Some(v) => { css_path = Some(v.clone()); i += 2; }
                None => { eprintln!("render_html: --css needs a value"); return ExitCode::from(2); }
            },
            "--custom-css" => match args.get(i + 1) {
                Some(v) => { custom_css_arg = Some(v.clone()); i += 2; }
                None => { eprintln!("render_html: --custom-css needs a value"); return ExitCode::from(2); }
            },
            "--no-custom-css" => { no_custom_css = true; i += 1; }
            "--no-doc-css" => { no_doc_css = true; i += 1; }
            other => { eprintln!("render_html: unknown argument: {other}"); return ExitCode::from(2); }
        }
    }

    if no_custom_css && custom_css_arg.is_some() {
        eprintln!("render_html: --custom-css and --no-custom-css are mutually exclusive");
        return ExitCode::from(2);
    }

    let md = match fs::read_to_string(md_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("render_html: cannot read {md_path}: {e}");
            return ExitCode::from(1);
        }
    };

    let css = match &css_path {
        Some(p) => match fs::read_to_string(p) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("render_html: cannot read css {p}: {e}");
                return ExitCode::from(1);
            }
        },
        None => EMBEDDED_CSS.to_string(),
    };

    // The user overlay, resolved in this order:
    //   --no-custom-css   -> never any overlay
    //   --custom-css FILE -> exactly that file; unreadable is an error, since it
    //                        was asked for by name
    //   --css FILE        -> no default overlay. `--css` means "I am supplying
    //                        the stylesheet", and quietly appending a
    //                        machine-local file on top would break the callers
    //                        that rely on it being a full replacement.
    //   (nothing)         -> ~/.config/monocle/custom.css when it exists
    // Held as (path_for_logging, contents).
    let custom_css: Option<(String, String)> = if no_custom_css {
        None
    } else if let Some(p) = &custom_css_arg {
        match fs::read_to_string(p) {
            Ok(s) => Some((p.clone(), s)),
            Err(e) => {
                eprintln!("render_html: cannot read custom css {p}: {e}");
                return ExitCode::from(1);
            }
        }
    } else if css_path.is_some() {
        None
    } else {
        let p = custom_css_path();
        match fs::read_to_string(&p) {
            Ok(s) => Some((p.to_string_lossy().into_owned(), s)),
            // An absent custom.css is the normal case, not a failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("render_html: ignoring unreadable {}: {e}", p.display());
                None
            }
        }
    };

    // Say which stylesheets this PDF was built with — otherwise a machine-local
    // custom.css silently changes the output and there is no way to tell.
    eprintln!(
        "[monocle-render] custom CSS: {}",
        custom_css.as_ref().map_or("none", |(p, _)| p.as_str())
    );

    // Absolute directory of the source, for <base> so relative links/images
    // resolve to the siblings of the .md file.
    let abs = match fs::canonicalize(md_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("render_html: cannot resolve {md_path}: {e}");
            return ExitCode::from(1);
        }
    };
    // Third and last layer: the stylesheet this document itself names in its
    // front matter. Resolved against the canonical document path so a relative
    // `css:` follows the document rather than the working directory.
    //
    // Suppressed only by --no-doc-css, NOT by --no-custom-css: the user layer is
    // machine-local state, whereas a document's own stylesheet ships with the
    // document, so a render that honours it is still reproducible anywhere. The
    // two flags each turn off exactly one layer.
    let document_css: Option<(std::path::PathBuf, String)> = if no_doc_css {
        None
    } else {
        doc_css(&abs, &md)
    };
    eprintln!(
        "[monocle-render] document CSS: {}",
        document_css
            .as_ref()
            .map_or("none".to_string(), |(p, _)| p.display().to_string())
    );

    let dir = abs.parent().unwrap_or_else(|| Path::new("/"));
    let base_href = format!("file://{}/", encode_path_for_url(&dir.to_string_lossy()));
    let title = abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let body = rewrite_md_links(&render_body(&md), link_dir.as_deref());

    // Overlay <style> blocks, emitted only when there is something to apply.
    let custom_style = match &custom_css {
        Some((_, c)) => format!("<style>{c}</style>\n"),
        None => String::new(),
    };
    let document_style = match &document_css {
        Some((_, c)) => format!("<style>{c}</style>\n"),
        None => String::new(),
    };

    // The <article …> opening tag is kept byte-for-byte identical to the one
    // the GUI builds (src/index.html + main.js) so styles.css selectors apply,
    // and so tests/print/html_to_pdf.swift can still locate the content.
    //
    // CASCADE: base sheet, then the user's ~/.config overlay, then the sheet the
    // document itself names — least specific to most, so a docs folder's house
    // style wins over a personal tweak, which wins over the built-in defaults.
    //
    // ORDERING DECISION: the user overlay is emitted after the base sheet, which
    // puts it after styles.css's `@media print` block, so an equal-specificity
    // user rule wins over it. That is deliberate, for two reasons. First, it is
    // exactly what the viewer does — main.js appends <style id="custom-css"> to
    // <head> after the styles.css <link> — and if the order differed here, this
    // tool and the viewer's own ⌘P would produce different PDFs from the same
    // document, which is a worse failure than any override. Second, `@media`
    // contributes no specificity, but the print rules that actually keep the
    // page laid out correctly (hiding chrome, flattening #content, forcing the
    // white page) are all `!important`, so a plain user rule cannot break them.
    // What stays overridable is the discretionary half — orphans/widows, the
    // `break-inside: avoid` hints and `.page-break` — which is precisely what
    // someone hand-tuning their own print output should be able to change.
    let doc = format!(
        "<!doctype html>\n\
         <html lang=\"en\"><head>\n\
         <meta charset=\"UTF-8\">\n\
         <base href=\"{base}\">\n\
         <title>{title}</title>\n\
         <style>{css}</style>\n\
         {custom_style}\
         {document_style}\
         </head><body>\n\
         <article id=\"content\" class=\"md-rendered\" style=\"display:block\">{body}</article>\n\
         </body></html>\n",
        base = base_href,
        title = title,
        css = css,
        custom_style = custom_style,
        document_style = document_style,
        body = body,
    );

    if let Err(e) = fs::write(out_path, doc) {
        eprintln!("render_html: cannot write {out_path}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_plain_md_link_beside_source() {
        assert_eq!(rewrite_one("guide.md", None), "guide.md.pdf");
        assert_eq!(rewrite_one("../a/guide.markdown", None), "../a/guide.markdown.pdf");
    }

    #[test]
    fn keeps_fragment_on_rewrite() {
        assert_eq!(rewrite_one("guide.md#intro", None), "guide.md.pdf#intro");
    }

    #[test]
    fn leaves_non_md_alone() {
        assert_eq!(rewrite_one("https://example.com/x.md", None), "https://example.com/x.md");
        assert_eq!(rewrite_one("#section", None), "#section");
        assert_eq!(rewrite_one("mailto:a@b.com", None), "mailto:a@b.com");
        assert_eq!(rewrite_one("/abs/path.md", None), "/abs/path.md");
        assert_eq!(rewrite_one("photo.png", None), "photo.png");
    }

    #[test]
    fn already_pdf_link_is_a_doc_target() {
        // beside-source: kept as-is (relative)
        assert_eq!(rewrite_one("already.md.pdf", None), "already.md.pdf");
        // with a link dir: pointed into it by file name
        assert_eq!(
            rewrite_one("already.md.pdf", Some("/out/pdf")),
            "file:///out/pdf/already.md.pdf"
        );
    }

    #[test]
    fn link_dir_makes_absolute_target_by_filename() {
        assert_eq!(
            rewrite_one("guide.md#x", Some("/out/pdf")),
            "file:///out/pdf/guide.md.pdf#x"
        );
        // nested relative path collapses to the flat output file name
        assert_eq!(
            rewrite_one("../sub/guide.md", Some("/out/pdf")),
            "file:///out/pdf/guide.md.pdf"
        );
        // non-doc links are still untouched even with a link dir
        assert_eq!(rewrite_one("photo.png", Some("/out/pdf")), "photo.png");
    }

    /// Guards the FIDELITY NOTE on `custom_css_path`: the viewer looks in
    /// `~/.config/monocle/custom.css` (monocle_lib::custom_css_path), and if
    /// this drifted the PDF would silently pick up a different file, or none.
    #[test]
    fn custom_css_path_matches_the_viewer() {
        let p = custom_css_path();
        assert!(
            p.ends_with(".config/monocle/custom.css"),
            "unexpected custom css path: {}",
            p.display()
        );
        assert!(p.is_absolute() || p.starts_with("."), "{}", p.display());
        assert_eq!(p.parent().unwrap().file_name().unwrap(), "monocle");
        assert_eq!(
            p.parent().unwrap().parent().unwrap().file_name().unwrap(),
            ".config"
        );
    }

    // Front matter. These cover shared/frontmatter.rs, which is include!d by
    // both this crate and src-tauri — so testing it here tests the viewer's copy
    // too, because there is only one copy.

    #[test]
    fn reads_css_key_from_front_matter() {
        assert_eq!(
            front_matter_css("---\ntitle: X\ncss: ./house.css\n---\n\n# H\n").as_deref(),
            Some("./house.css")
        );
        assert_eq!(
            front_matter_css("---\r\ncss: a.css\r\n---\r\n").as_deref(),
            Some("a.css")
        );
        // quoted, and with a trailing comment
        assert_eq!(
            front_matter_css("---\ncss: \"my sheet.css\"\n---\n").as_deref(),
            Some("my sheet.css")
        );
        assert_eq!(
            front_matter_css("---\ncss: a.css  # the house style\n---\n").as_deref(),
            Some("a.css")
        );
    }

    #[test]
    fn ignores_css_that_is_not_top_level_front_matter() {
        // no front matter at all
        assert_eq!(front_matter_css("# H\n\ncss: a.css\n"), None);
        // front matter without the key
        assert_eq!(front_matter_css("---\ntitle: X\n---\n"), None);
        // nested under another key
        assert_eq!(front_matter_css("---\ntheme:\n  css: a.css\n---\n"), None);
        // after the block has closed
        assert_eq!(front_matter_css("---\ntitle: X\n---\ncss: a.css\n"), None);
        // empty value
        assert_eq!(front_matter_css("---\ncss:\n---\n"), None);
        // a leading `---` with no closing fence is a thematic break, not metadata
        assert_eq!(front_matter_css("---\n\nBody\n"), None);
    }

    #[test]
    fn resolves_doc_css_against_the_document_not_the_cwd() {
        let doc = Path::new("/docs/guide/a.md");
        assert_eq!(
            resolve_doc_css(doc, "./house.css").unwrap(),
            Path::new("/docs/guide/./house.css")
        );
        assert_eq!(
            resolve_doc_css(doc, "../shared/x.css").unwrap(),
            Path::new("/docs/guide/../shared/x.css")
        );
        assert_eq!(
            resolve_doc_css(doc, "/abs/x.css").unwrap(),
            Path::new("/abs/x.css")
        );
        assert!(resolve_doc_css(doc, "~/x.css").unwrap().is_absolute());
    }

    /// A document names the stylesheet that gets inlined into the page, so it
    /// must not be able to pull in an arbitrary file.
    #[test]
    fn refuses_doc_css_that_is_not_a_stylesheet() {
        let doc = Path::new("/docs/a.md");
        assert_eq!(resolve_doc_css(doc, "/etc/passwd"), None);
        assert_eq!(resolve_doc_css(doc, "~/.ssh/id_rsa"), None);
        assert_eq!(resolve_doc_css(doc, "../../secrets.env"), None);
        // case-insensitive on the extension
        assert!(resolve_doc_css(doc, "House.CSS").is_some());
    }

    #[test]
    fn rewrites_inside_anchor_tag() {
        let got = rewrite_md_links(r##"<a href="g.md">x</a> <a href="#a">y</a>"##, None);
        assert_eq!(got, r##"<a href="g.md.pdf">x</a> <a href="#a">y</a>"##);
    }
}
