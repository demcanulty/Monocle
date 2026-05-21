#!/usr/bin/env osascript -l JavaScript
// Drive the real Monocle.app: open a markdown file, hit ⌘P, read the page
// count from the print dialog's "All N Pages" radio button, ESC to close, quit.
//
// Catches the stale-pagination bug that headless test harnesses miss because
// it's specific to Tauri's `w.print()` invocation path.
//
// usage: osascript -l JavaScript check_pages.js <md-path> <expected-pages>
// exits 0 if actual page count == expected, non-zero otherwise.

ObjC.import('stdlib');

function run(argv) {
  if (argv.length < 2) {
    console.log('usage: check_pages.js <md-path> <expected-pages>');
    $.exit(2);
  }
  const mdPath = argv[0];
  const expected = parseInt(argv[1], 10);

  const app = Application.currentApplication();
  app.includeStandardAdditions = true;

  // Quit any existing Monocle so we start fresh.
  try { app.doShellScript('pkill -x monocle; sleep 1.0'); } catch (_) {}
  app.doShellScript(`open -a /Applications/Monocle.app ${JSON.stringify(mdPath)}`);

  const se = Application('System Events');
  // Wait for Monocle's window to exist before sending ⌘P.
  let proc = null;
  for (let i = 0; i < 80; i++) {
    try {
      const p = se.processes['Monocle'];
      if (p.exists() && p.windows.length > 0) {
        proc = p;
        break;
      }
    } catch (_) {}
    delay(0.15);
  }
  if (!proc) {
    console.log('Monocle window never appeared');
    $.exit(3);
  }
  // Extra settle time so the file is fully rendered before print.
  delay(0.6);
  proc.frontmost = true;
  delay(0.2);
  se.keystroke('p', { using: 'command down' });

  // Wait for the print sheet to appear.
  let sheet = null;
  for (let i = 0; i < 100; i++) {
    const sheets = proc.windows[0].sheets;
    if (sheets.length > 0) {
      sheet = sheets[0];
      break;
    }
    delay(0.1);
  }
  if (!sheet) {
    console.log('print sheet never appeared');
    $.exit(3);
  }
  // Let the dialog finish settling.
  delay(0.6);

  // Walk the tree and find the "All N Pages" radio button.
  let allRadioTitle = null;
  function walk(el, depth) {
    if (depth > 8 || allRadioTitle) return;
    let role = null, title = null;
    try { role = el.role(); } catch (_) {}
    try { title = el.title(); } catch (_) {}
    if (role === 'AXRadioButton' && title && /^All\s+\d+\s+Pages?$/.test(title)) {
      allRadioTitle = title;
      return;
    }
    let kids;
    try { kids = el.uiElements(); } catch (_) { kids = []; }
    for (const k of kids) walk(k, depth + 1);
  }
  walk(sheet, 0);

  // Close the dialog.
  se.keystroke(String.fromCharCode(27));  // ESC
  delay(0.2);

  // Quit Monocle for a clean next run.
  try { Application('Monocle').quit(); } catch (_) {}

  if (!allRadioTitle) {
    console.log('could not find "All N Pages" radio button');
    $.exit(4);
  }

  const m = allRadioTitle.match(/^All\s+(\d+)\s+Pages?$/);
  const actual = parseInt(m[1], 10);
  console.log(`page count: ${actual} (expected ${expected})  [label: "${allRadioTitle}"]`);
  if (actual !== expected) $.exit(1);
  return 'ok';
}
