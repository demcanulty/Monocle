// render_html — Monocle's headless markdown -> HTML step for PDF export.
//
// It produces the SAME HTML the Monocle viewer shows on screen:
//   * the identical pulldown-cmark options the GUI uses, and
//   * the project's real `src/styles.css` (embedded at build time), wrapped in
//     the same `<article class="md-rendered">` element the GUI renders into.
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
// `monocle_lib::render_to_html` (src-tauri/src/lib.rs). That, plus the shared
// styles.css, is what makes the PDF look like the viewer.

use pulldown_cmark::{html, Options, Parser};
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const EMBEDDED_CSS: &str = include_str!("../src/styles.css");

/// Markdown -> body HTML, using the exact options Monocle's viewer uses.
fn render_body(md: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
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
        eprintln!("  --link-dir DIR  directory the rendered PDFs live in; markdown");
        eprintln!("                  cross-links are pointed there (default: beside source).");
        eprintln!("  --css FILE      stylesheet to use (default: embedded src/styles.css).");
        return ExitCode::from(2);
    }
    let md_path = &args[1];
    let out_path = &args[2];

    let mut link_dir: Option<String> = None;
    let mut css_path: Option<String> = None;
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
            other => { eprintln!("render_html: unknown argument: {other}"); return ExitCode::from(2); }
        }
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

    // Absolute directory of the source, for <base> so relative links/images
    // resolve to the siblings of the .md file.
    let abs = match fs::canonicalize(md_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("render_html: cannot resolve {md_path}: {e}");
            return ExitCode::from(1);
        }
    };
    let dir = abs.parent().unwrap_or_else(|| Path::new("/"));
    let base_href = format!("file://{}/", encode_path_for_url(&dir.to_string_lossy()));
    let title = abs
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let body = rewrite_md_links(&render_body(&md), link_dir.as_deref());

    // The <article …> opening tag is kept byte-for-byte identical to the one
    // the GUI builds (src/index.html + main.js) so styles.css selectors apply,
    // and so tests/print/html_to_pdf.swift can still locate the content.
    let doc = format!(
        "<!doctype html>\n\
         <html lang=\"en\"><head>\n\
         <meta charset=\"UTF-8\">\n\
         <base href=\"{base}\">\n\
         <title>{title}</title>\n\
         <style>{css}</style>\n\
         </head><body>\n\
         <article id=\"content\" class=\"md-rendered\" style=\"display:block\">{body}</article>\n\
         </body></html>\n",
        base = base_href,
        title = title,
        css = css,
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

    #[test]
    fn rewrites_inside_anchor_tag() {
        let got = rewrite_md_links(r##"<a href="g.md">x</a> <a href="#a">y</a>"##, None);
        assert_eq!(got, r##"<a href="g.md.pdf">x</a> <a href="#a">y</a>"##);
    }
}
