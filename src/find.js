// find.js — in-page find bar for Monocle's rendered view.
//
// The editor pane has CodeMirror's own search panel; this covers the rendered
// markdown: #content in viewer mode, #preview-content in editor mode.
//
// Matches are painted with the CSS Custom Highlight API rather than wrapped in
// <mark> elements. Live reload replaces the rendered HTML wholesale, so any DOM
// we injected would be thrown away (or worse, saved back into a re-render) —
// highlight ranges are external to the document and simply get rebuilt.

// Enough for any document worth reading; guards against pathological queries
// like a single space in a large file.
const MAX_MATCHES = 5000;

// Tags that don't break a run of text. Anything else gets a synthetic newline
// between it and the previous run, so a query can't match across a block
// boundary (pulldown-cmark emits <td>a</td><td>b</td> with no whitespace).
const INLINE_TAGS = new Set([
  "A", "ABBR", "B", "BDI", "BDO", "CITE", "CODE", "DATA", "DEL", "DFN", "EM",
  "I", "INS", "KBD", "MARK", "Q", "RUBY", "S", "SAMP", "SMALL", "SPAN",
  "STRONG", "SUB", "SUP", "TIME", "U", "VAR",
]);

const SUPPORTS_HIGHLIGHT =
  typeof CSS !== "undefined" &&
  !!CSS.highlights &&
  typeof Highlight === "function";

let barEl, inputEl, countEl, caseBtn, prevBtn, nextBtn, closeBtn;
let rootEl = null;
let isOpen = false;
let caseSensitive = false;
let matches = [];
let current = -1;
let truncated = false;
let searchTimer = null;
let allHighlight = null;
let currentHighlight = null;
let scrollerCache = null;
let scrollerCacheRoot = null;

// ── Match collection ──

function blockAncestor(el, root) {
  let cur = el;
  while (cur && cur !== root && INLINE_TAGS.has(cur.tagName)) {
    cur = cur.parentElement;
  }
  return cur || root;
}

/// Flatten the rendered text into one string, keeping a sorted index of where
/// each text node starts so match offsets can be mapped back to DOM positions.
function collectText(root) {
  const nodes = [];
  let text = "";
  let lastBlock = null;

  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const value = node.nodeValue;
    const parent = node.parentElement;
    if (!value || !parent) continue;
    const tag = parent.tagName;
    if (tag === "SCRIPT" || tag === "STYLE" || tag === "NOSCRIPT") continue;

    const block = blockAncestor(parent, root);
    if (lastBlock && block !== lastBlock) text += "\n";
    lastBlock = block;

    nodes.push({ node, start: text.length });
    text += value;
  }

  return { text, nodes };
}

/// Last node whose start offset is <= `offset`. Synthetic newlines belong to no
/// node, but a match never covers one (the query comes from a single-line input),
/// so every offset we look up lands inside real text.
function nodeAt(nodes, offset) {
  let lo = 0;
  let hi = nodes.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (nodes[mid].start <= offset) lo = mid;
    else hi = mid - 1;
  }
  return nodes[lo];
}

function findMatches(root, query) {
  truncated = false;
  if (!root || !query) return [];

  const { text, nodes } = collectText(root);
  if (!nodes.length) return [];

  let haystack = text;
  let needle = query;
  if (!caseSensitive) {
    const lowered = text.toLowerCase();
    // A handful of characters change length when lowercased, which would
    // desync every offset after them. Rather than mis-highlight, fall back to
    // a case-sensitive search for those documents.
    if (lowered.length === text.length) {
      haystack = lowered;
      needle = query.toLowerCase();
    }
  }

  const found = [];
  let from = 0;
  for (;;) {
    const at = haystack.indexOf(needle, from);
    if (at === -1) break;
    if (found.length >= MAX_MATCHES) {
      truncated = true;
      break;
    }

    const start = nodeAt(nodes, at);
    const end = nodeAt(nodes, at + needle.length - 1);
    const range = document.createRange();
    range.setStart(start.node, at - start.start);
    range.setEnd(end.node, at + needle.length - end.start);
    found.push(range);

    from = at + needle.length;
  }

  return found;
}

// ── Painting ──

function paint() {
  if (!SUPPORTS_HIGHLIGHT) {
    // No Custom Highlight API: fall back to the native selection so the current
    // match is at least visible.
    if (current >= 0) {
      const sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(matches[current]);
    }
    return;
  }

  allHighlight.clear();
  currentHighlight.clear();
  for (let i = 0; i < matches.length; i++) {
    (i === current ? currentHighlight : allHighlight).add(matches[i]);
  }
}

function clearPaint() {
  if (SUPPORTS_HIGHLIGHT) {
    allHighlight.clear();
    currentHighlight.clear();
  } else {
    window.getSelection().removeAllRanges();
  }
}

// ── Scrolling ──

/// Viewer mode scrolls the window; editor mode scrolls #preview-pane.
function scroller() {
  if (scrollerCacheRoot === rootEl) return scrollerCache;
  let el = rootEl ? rootEl.parentElement : null;
  let found = null;
  while (el && el !== document.body) {
    const overflow = getComputedStyle(el).overflowY;
    if (overflow === "auto" || overflow === "scroll") {
      found = el;
      break;
    }
    el = el.parentElement;
  }
  scrollerCacheRoot = rootEl;
  scrollerCache = found;
  return found;
}

function scrollBox() {
  const pane = scroller();
  return pane
    ? pane.getBoundingClientRect()
    : { top: 0, bottom: window.innerHeight, height: window.innerHeight };
}

/// Viewport y a match has to clear to be readable — the fixed toolbar and the
/// find bar itself sit above it, in both viewer and editor layouts.
function visibleTop(box) {
  return Math.max(box.top, 76);
}

function reveal(range) {
  const rect = range.getBoundingClientRect();
  if (!rect.width && !rect.height) return;

  const box = scrollBox();
  const top = visibleTop(box);
  if (rect.top >= top && rect.bottom <= box.bottom - 40) return;

  // Out of view — bring it to a comfortable reading position rather than
  // flush against the top edge.
  const margin = Math.min(Math.max(box.height * 0.25, 40), box.height / 2);
  const by = rect.top - (top + margin);
  const pane = scroller();
  if (pane) pane.scrollTop += by;
  else window.scrollBy(0, by);
}

/// Index of the first match at or below the current viewport, so a fresh query
/// lands where the reader already is instead of jumping to the top.
function nearestToViewport() {
  if (!matches.length) return -1;
  const top = visibleTop(scrollBox());

  for (let i = 0; i < matches.length; i++) {
    if (matches[i].getBoundingClientRect().bottom >= top) return i;
  }
  return 0;
}

// ── Search driving ──

function updateCount() {
  if (!inputEl.value) {
    countEl.textContent = "";
    barEl.classList.remove("no-match");
    return;
  }
  if (!matches.length) {
    countEl.textContent = "0/0";
    barEl.classList.add("no-match");
    return;
  }
  barEl.classList.remove("no-match");
  countEl.textContent = `${current + 1}/${matches.length}${truncated ? "+" : ""}`;
}

function runSearch(opts) {
  const { keepIndex = false, scroll = true } = opts || {};
  const previous = current;

  matches = findMatches(rootEl, inputEl.value);

  if (!matches.length) current = -1;
  else if (keepIndex && previous >= 0) current = Math.min(previous, matches.length - 1);
  else current = nearestToViewport();

  paint();
  if (scroll && current >= 0) reveal(matches[current]);
  updateCount();
}

/// Flush a pending debounced search. Returns true if one ran.
function flush() {
  if (!searchTimer) return false;
  clearTimeout(searchTimer);
  searchTimer = null;
  runSearch();
  return true;
}

function step(delta) {
  if (!matches.length) return;
  current = (current + delta + matches.length) % matches.length;
  paint();
  reveal(matches[current]);
  updateCount();
}

// ── Public API ──

window.MonocleFind = {
  init() {
    barEl = document.getElementById("find-bar");
    inputEl = document.getElementById("find-input");
    countEl = document.getElementById("find-count");
    caseBtn = document.getElementById("find-case");
    prevBtn = document.getElementById("find-prev");
    nextBtn = document.getElementById("find-next");
    closeBtn = document.getElementById("find-close");

    if (SUPPORTS_HIGHLIGHT) {
      allHighlight = new Highlight();
      currentHighlight = new Highlight();
      CSS.highlights.set("monocle-find", allHighlight);
      CSS.highlights.set("monocle-find-current", currentHighlight);
    }

    inputEl.addEventListener("input", () => {
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => {
        searchTimer = null;
        runSearch();
      }, 90);
    });

    inputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        // Typing then hitting Enter should land on the first match, not skip it.
        if (!flush()) step(e.shiftKey ? -1 : 1);
      } else if (e.key === "Escape") {
        e.preventDefault();
        this.close();
      }
    });

    caseBtn.addEventListener("click", () => {
      caseSensitive = !caseSensitive;
      caseBtn.classList.toggle("active", caseSensitive);
      caseBtn.setAttribute("aria-pressed", String(caseSensitive));
      runSearch({ keepIndex: true });
      inputEl.focus();
    });

    prevBtn.addEventListener("click", () => {
      flush();
      step(-1);
      inputEl.focus();
    });
    nextBtn.addEventListener("click", () => {
      if (!flush()) step(1);
      inputEl.focus();
    });
    closeBtn.addEventListener("click", () => this.close());
  },

  isOpen() {
    return isOpen;
  },

  open(root) {
    if (!root) return;
    // ⌘F on an already-open bar re-selects the query without moving the reader
    // off the match they're looking at.
    const reopening = isOpen && root === rootEl;
    if (root !== rootEl) {
      rootEl = root;
      matches = [];
      current = -1;
    }
    isOpen = true;
    barEl.classList.add("visible");
    inputEl.focus();
    inputEl.select();
    if (inputEl.value) runSearch({ keepIndex: reopening, scroll: !reopening });
    else updateCount();
  },

  close() {
    if (!isOpen) return;
    isOpen = false;
    barEl.classList.remove("visible");
    clearTimeout(searchTimer);
    searchTimer = null;
    clearPaint();
    matches = [];
    current = -1;
    // Hand focus back so the arrow keys scroll the document again.
    if (document.activeElement && barEl.contains(document.activeElement)) {
      document.activeElement.blur();
    }
  },

  next() {
    if (!isOpen) return;
    if (!flush()) step(1);
  },

  prev() {
    if (!isOpen) return;
    flush();
    step(-1);
  },

  /// Point the search at a different rendered root (viewer ⇄ editor preview).
  setRoot(root) {
    if (!root || root === rootEl) return;
    rootEl = root;
    if (isOpen) runSearch({ scroll: false });
    else {
      matches = [];
      current = -1;
    }
  },

  /// Re-resolve matches after the rendered HTML was replaced — the old ranges
  /// point at detached nodes.
  refresh() {
    if (!isOpen) return;
    runSearch({ keepIndex: true, scroll: false });
  },
};
