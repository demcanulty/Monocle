// Renders a markdown-derived HTML body into a paginated PDF using WKWebView +
// NSPrintOperation — the same WebKit print pipeline Monocle hits.
//
// Two loading modes:
//   --mode=load    : loadHTMLString with the full document (baseline)
//   --mode=inject  : load an empty shell, then assign #content.innerHTML via
//                    evaluateJavaScript — mirrors how Monocle injects content.
//
// The inject mode is the one expected to reproduce WebKit's
// stale-print-layout bug, because the screen-mode layout completes but the
// print-media layout has never been computed when print is triggered.

import Cocoa
import WebKit

func die(_ msg: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

// ---- CLI parsing ----
var mode = "inject"
var htmlPath: String? = nil
var pdfPath: String? = nil
var delayMS = 100
for arg in CommandLine.arguments.dropFirst() {
    if arg.hasPrefix("--mode=") {
        mode = String(arg.dropFirst("--mode=".count))
    } else if arg.hasPrefix("--delay=") {
        delayMS = Int(String(arg.dropFirst("--delay=".count))) ?? 100
    } else if htmlPath == nil {
        htmlPath = arg
    } else if pdfPath == nil {
        pdfPath = arg
    }
}
guard let htmlPath = htmlPath, let pdfPath = pdfPath else {
    die("usage: html_to_pdf [--mode=load|inject] [--delay=ms] <input.html> <output.pdf>", code: 2)
}
guard mode == "load" || mode == "inject" else {
    die("invalid --mode (use load or inject)", code: 2)
}

let pdfURL = URL(fileURLWithPath: (pdfPath as NSString).expandingTildeInPath)
try? FileManager.default.createDirectory(at: pdfURL.deletingLastPathComponent(),
                                         withIntermediateDirectories: true)

// Read the full HTML file and split into <style> contents + body inner content.
let inputURL = URL(fileURLWithPath: htmlPath)
let fullHTML: String
do { fullHTML = try String(contentsOf: inputURL, encoding: .utf8) }
catch { die("failed to read html: \(error)") }

func substringBetween(_ s: String, start: String, end: String) -> String? {
    guard let lo = s.range(of: start)?.upperBound else { return nil }
    guard let hi = s[lo...].range(of: end)?.lowerBound else { return nil }
    return String(s[lo..<hi])
}
/// Every <style> block, in document order, joined into one. render_html emits
/// two of them — the base styles.css, then the user's custom.css overlay — and
/// taking only the first would silently render without the overlay. Order is
/// preserved, so concatenating is cascade-equivalent to separate elements.
func allStyleBlocks(_ s: String) -> String {
    var blocks: [String] = []
    var rest = Substring(s)
    while let lo = rest.range(of: "<style>")?.upperBound {
        let tail = rest[lo...]
        guard let hi = tail.range(of: "</style>")?.lowerBound else { break }
        blocks.append(String(tail[..<hi]))
        rest = tail[hi...]
    }
    return blocks.joined(separator: "\n")
}
let styleContent = allStyleBlocks(fullHTML)
let articleInner = substringBetween(fullHTML, start: "<article id=\"content\" class=\"md-rendered\" style=\"display:block\">", end: "</article>")
    ?? substringBetween(fullHTML, start: "<article", end: "</article>")
    ?? ""

// Window dimensions that match Monocle's default (900×700, minus chrome).
final class Renderer: NSObject, WKNavigationDelegate {
    let webView: WKWebView
    let window: NSWindow
    let pdfURL: URL
    let mode: String
    let delayMS: Int
    let articleInner: String

    init(pdfURL: URL, mode: String, delayMS: Int, styleContent: String, articleInner: String) {
        let cfg = WKWebViewConfiguration()
        let frame = NSRect(x: 0, y: 0, width: 900, height: 700)
        self.webView = WKWebView(frame: frame, configuration: cfg)
        self.window = NSWindow(contentRect: frame,
                               styleMask: [.titled],
                               backing: .buffered,
                               defer: false)
        self.window.contentView = self.webView
        self.pdfURL = pdfURL
        self.mode = mode
        self.delayMS = delayMS
        self.articleInner = articleInner
        super.init()
        self.webView.navigationDelegate = self

        if mode == "load" {
            let html = """
            <!doctype html><html><head><meta charset="UTF-8">
            <style>\(styleContent)</style></head><body>
            <article id="content" class="md-rendered" style="display:block">\(articleInner)</article>
            </body></html>
            """
            self.webView.loadHTMLString(html, baseURL: nil)
        } else {
            // Empty shell — matches Monocle's index.html structure.
            let shell = """
            <!doctype html><html><head><meta charset="UTF-8">
            <style>\(styleContent)</style></head><body>
            <article id="content" class="md-rendered" style="display:none"></article>
            </body></html>
            """
            self.webView.loadHTMLString(shell, baseURL: nil)
        }
    }

    func webView(_ wv: WKWebView, didFinish nav: WKNavigation!) {
        if mode == "inject" {
            // Mirror Monocle: assign innerHTML on #content then make it visible.
            let js = """
            (function() {
              var el = document.getElementById('content');
              el.innerHTML = \(stringToJSLiteral(articleInner));
              el.style.display = 'block';
            })();
            """
            wv.evaluateJavaScript(js) { _, err in
                if let err = err { die("js inject failed: \(err)") }
                self.scheduleFinalPrint()
            }
        } else {
            scheduleFinalPrint()
        }
    }

    func scheduleFinalPrint() {
        let secs = Double(delayMS) / 1000.0
        DispatchQueue.main.asyncAfter(deadline: .now() + secs) { self.printToPDF() }
    }

    func webView(_ wv: WKWebView, didFail nav: WKNavigation!, withError err: Error) {
        die("navigation failed: \(err)")
    }

    func printToPDF() {
        let info = NSPrintInfo()
        info.paperSize = NSSize(width: 612, height: 792)  // Letter
        info.topMargin = 54; info.bottomMargin = 54
        info.leftMargin = 54; info.rightMargin = 54
        info.horizontalPagination = .automatic
        info.verticalPagination = .automatic
        info.jobDisposition = .save
        info.dictionary()[NSPrintInfo.AttributeKey.jobSavingURL] = pdfURL

        guard let op = webView.printOperation(with: info) as NSPrintOperation? else {
            die("printOperation returned nil")
        }
        op.showsPrintPanel = false
        op.showsProgressPanel = false
        op.runModal(for: window,
                    delegate: self,
                    didRun: #selector(printDidRun(_:success:contextInfo:)),
                    contextInfo: nil)
    }

    @objc func printDidRun(_ op: NSPrintOperation, success: Bool, contextInfo: UnsafeMutableRawPointer?) {
        if !success { die("print failed") }
        if !FileManager.default.fileExists(atPath: pdfURL.path) {
            die("no PDF at \(pdfURL.path)")
        }
        exit(0)
    }
}

func stringToJSLiteral(_ s: String) -> String {
    let data = try! JSONSerialization.data(withJSONObject: [s], options: [])
    let str = String(data: data, encoding: .utf8)!
    // strip outer brackets
    return String(str.dropFirst().dropLast())
}

let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let renderer = Renderer(pdfURL: pdfURL,
                        mode: mode,
                        delayMS: delayMS,
                        styleContent: styleContent,
                        articleInner: articleInner)
_ = renderer
app.run()
