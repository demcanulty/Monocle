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
            + ~/.config/monocle/custom.css
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

### Custom CSS

`render_html` emits up to **three** `<style>` blocks, in the same order the
viewer loads them — least specific to most:

| # | Layer | Source |
|---|---|---|
| 1 | base | embedded `src/styles.css`, or `--css FILE` |
| 2 | user | `~/.config/monocle/custom.css`, or `--custom-css FILE` |
| 3 | document | the `css:` key in the document's own YAML front matter |

Layer 2's path is resolved by the same logic as `monocle_lib::custom_css_path`,
and layer 3 by `shared/frontmatter.rs`, which is `include!`d by both this crate
and `src-tauri` — so the viewer and the PDF can never disagree about which
stylesheet a document gets. A missing file at any layer is not an error; the
block is simply omitted.

Layer 3 lets a stylesheet travel *with* the docs instead of living in the config
folder:

```markdown
---
title: Quarterly report
css: ./house-style.css
---

# Quarterly report
```

The path resolves against the directory holding the `.md` file, never the working
directory, so the folder stays portable — move it, or hand it to someone else,
and it renders the same. Only values ending in `.css` are accepted, so a document
cannot name an unrelated file to be inlined into the page.

Enabling front matter means pulldown-cmark's `ENABLE_YAML_STYLE_METADATA_BLOCKS`
is on in both renderers. A `---` fenced block at the very top of a document is
now metadata and no longer renders; a lone `---` thematic break (no closing
fence) is unaffected.

Because the overlay comes last it lands *after* the `@media print` block, so an
equal-specificity user rule wins over it. That is deliberate: it matches the
viewer (and therefore the viewer's own ⌘P), and the print rules that actually
keep the page laid out correctly are `!important`, so only the discretionary
hints (`orphans`/`widows`, `break-inside`, `.page-break`) are overridable.

| Flag | Effect |
|---|---|
| *(none)* | layer 2 from `~/.config`, layer 3 from the document |
| `--custom-css FILE` | use `FILE` as layer 2; unreadable is an error |
| `--no-custom-css` | drop layer 2 (the machine-local sheet) |
| `--no-doc-css` | drop layer 3 (the document's own sheet) |
| `--css FILE` | **replace** the base sheet; also suppresses layer 2, since `--css` means "I control the stylesheet". Combine with `--custom-css` to opt back in. |

`--no-custom-css` deliberately does *not* drop layer 3: it exists so a render
can't depend on machine-local state, and a document's own stylesheet ships with
the document. Pass both flags for stock styling.

`monocle-render` forwards all three flags and passes nothing by default. Either
way the resolved choices are echoed to stderr:

```
[monocle-render] custom CSS: /Users/you/.config/monocle/custom.css
[monocle-render] document CSS: /docs/guide/house-style.css
```

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
- The arrow/box-drawing alignment fix in `styles.css` uses the system **SF Mono**
  font (macOS) for code blocks, with the generic `monospace` as the *only* fallback
  — deliberately **not** Menlo. SF Mono draws the box-drawing lines into clean,
  connected boxes; the `►`/`◄` arrowheads (U+25BA / U+25C4) fall through to the
  generic monospace so they render vertically centered on the line. (Menlo would
  supply those arrowheads as a low-sitting triangle that misaligns with the box
  line — the bug this stack avoids.) See the comment at `src/styles.css` (code
  font). Off macOS it has no effect and the normal font stack applies.

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
