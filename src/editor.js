// editor.js — CodeMirror 6 wrapper for Monocle
// Depends on window.CM (loaded from vendor/codemirror-bundle.js)

const {
  state: { EditorState },
  view: {
    EditorView,
    keymap,
    lineNumbers,
    highlightActiveLine,
    highlightSpecialChars,
    drawSelection,
    rectangularSelection,
  },
  commands: {
    defaultKeymap,
    history,
    historyKeymap,
    indentWithTab,
  },
  langMarkdown: { markdown },
  language: {
    indentOnInput,
    bracketMatching,
    syntaxHighlighting,
    defaultHighlightStyle,
  },
  search: { searchKeymap, highlightSelectionMatches },
} = CM;

let editorView = null;
let lastSavedContent = "";
let changeCallback = null;
let debounceTimer = null;

const monocleTheme = EditorView.theme({
  "&": {
    height: "100%",
    fontSize: "14px",
  },
  ".cm-scroller": {
    fontFamily:
      '"SF Mono", "Fira Code", "Fira Mono", Menlo, Consolas, monospace',
    overflow: "auto",
  },
  ".cm-line": {
    padding: "0 8px",
  },
});

const monocleHighlight = syntaxHighlighting(defaultHighlightStyle, {
  fallback: true,
});

window.MonocleEditor = {
  init(containerEl) {
    if (editorView) this.destroy();

    const startState = EditorState.create({
      doc: "",
      extensions: [
        lineNumbers(),
        highlightActiveLine(),
        highlightSpecialChars(),
        drawSelection(),
        rectangularSelection(),
        indentOnInput(),
        bracketMatching(),
        highlightSelectionMatches(),
        history(),
        markdown(),
        monocleTheme,
        monocleHighlight,
        keymap.of([
          ...defaultKeymap,
          ...historyKeymap,
          ...searchKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged && changeCallback) {
            clearTimeout(debounceTimer);
            debounceTimer = setTimeout(() => {
              changeCallback(editorView.state.doc.toString());
            }, 300);
          }
        }),
        EditorView.lineWrapping,
      ],
    });

    editorView = new EditorView({
      state: startState,
      parent: containerEl,
    });
  },

  setContent(text) {
    if (!editorView) return;
    editorView.dispatch({
      changes: {
        from: 0,
        to: editorView.state.doc.length,
        insert: text,
      },
    });
    lastSavedContent = text;
  },

  insertAtCursor(text) {
    if (!editorView) return;
    const { from, to } = editorView.state.selection.main;
    editorView.dispatch({
      changes: { from, to, insert: text },
      selection: { anchor: from + text.length },
    });
    editorView.focus();
  },

  getContent() {
    if (!editorView) return "";
    return editorView.state.doc.toString();
  },

  isDirty() {
    if (!editorView) return false;
    return editorView.state.doc.toString() !== lastSavedContent;
  },

  markClean() {
    if (!editorView) return;
    lastSavedContent = editorView.state.doc.toString();
  },

  onContentChange(callback) {
    changeCallback = callback;
  },

  focus() {
    if (editorView) editorView.focus();
  },

  destroy() {
    if (editorView) {
      editorView.destroy();
      editorView = null;
    }
    lastSavedContent = "";
    changeCallback = null;
    clearTimeout(debounceTimer);
  },
};
