# Headless PDF rendering

Render a Markdown file to a **PDF that matches what Monocle shows on screen**,
with no GUI window and no print dialog — so it can be scripted and batched.

Run it with [`../bin/monocle-render`](../bin/monocle-render):

```sh
bin/monocle-render FILE.md [FILE2.md ...]   # -> <dir>/pdf/<name>.md.pdf
bin/monocle-render --out-dir DIR FILE.md…   # write PDFs into DIR
bin/monocle-render -o OUT.pdf FILE.md       # explicit output (single input)
bin/monocle-render *.md                      # batch via shell glob
```

By default the PDFs land in a `pdf/` subfolder next to each source.

## How it works

```
FILE.md ──▶ render_html (Rust) ──▶ FILE.html ──▶ headless Chrome ──▶ FILE.md.pdf
            Monocle's md→HTML                     --print-to-pdf
            + embedded styles.css
```

1. **`render_html`** (this crate) converts Markdown to HTML using the *exact*
   pipeline the viewer uses — the same `pulldown-cmark` options and the project's
   real `src/styles.css`, embedded at build time and wrapped in the same
   `<article class="md-rendered">` element the GUI renders into. Because the
   viewer applies no run-time transforms to the rendered Markdown (no syntax
   highlighter, KaTeX, etc. — the CodeMirror editor only edits raw text), this
   static HTML is a faithful copy of the on-screen content.
2. **Headless Chrome** (`--headless=new --print-to-pdf`) turns that HTML into a
   paginated Letter PDF, honouring `styles.css`'s `@media print` block (light
   theme, `@page` margins, page-break hints).

## Why this approach

Fidelity comes from reusing Monocle's own renderer + CSS, so the only real
choice is the HTML→PDF engine. Three were evaluated:

| Engine | Visual fidelity | Paginated | Clickable links | Notes |
|---|---|---|---|---|
| **WKWebView + `NSPrintOperation`** | exact (WebKit, same as viewer) | yes | **no** | the macOS print path **drops all hyperlink annotations** |
| **WKWebView `createPDF`** | exact (WebKit) | **no** | yes | captures one tall page, not paginated |
| **Headless Chrome `--print-to-pdf`** | near-identical (Blink, same CSS) | yes | **yes** | ✅ chosen |

The deciding requirement is **working cross-document links**: the client docs
link to each other and clicking a link should open the sibling PDF. Only Chrome
delivers pagination *and* clickable links while still rendering Monocle's exact
HTML + CSS. The cost is Blink instead of WebKit, but for this GitHub-style CSS
the two are visually indistinguishable (system fonts, simple tables, code
blocks); only minor line-breaking/pagination differs.

A `<base href="file://<source-dir>/">` is injected so relative images/resources
resolve to the source directory, and Markdown cross-links (`other.md`,
`other.md.pdf`) are pointed at the output directory (`--link-dir`) where the
sibling PDFs are written.

## Requirements (macOS)

- **Google Chrome** (or any Chromium-based browser; override with `$CHROME`).
- **Rust / cargo** to build `render_html`.
- The arrow/box-drawing alignment fix in `styles.css` uses the system **Menlo**
  font (macOS). Elsewhere it has no effect and the normal font stack applies.

## Limitations

- In-page heading anchors (`[x](#heading)`) do not resolve, because the viewer's
  `pulldown-cmark` config emits no heading `id`s — so those links are inert in
  Monocle too. This is intentional fidelity, not a regression.
- Cross-links are baked as **absolute** `file://` paths to the output directory:
  clicking opens the sibling PDF on this machine. Moving the PDFs elsewhere
  breaks the links; re-render in place.

## Tests

- `cargo test` — unit tests for the link-rewriting rules.
- `../tests/print/run.sh` — builds this renderer, prints the fixture through the
  WebKit path, and asserts the text survives pagination.
