use pulldown_cmark::{html, Options, Parser};
use std::env;
use std::fs;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: render_html <markdown-file> <styles.css> <output.html>");
        return ExitCode::from(2);
    }
    let md_path = &args[1];
    let css_path = &args[2];
    let out_path = &args[3];

    let md = match fs::read_to_string(md_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read markdown {}: {}", md_path, e);
            return ExitCode::from(1);
        }
    };
    let css = match fs::read_to_string(css_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to read css {}: {}", css_path, e);
            return ExitCode::from(1);
        }
    };

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&md, options);
    let mut body_html = String::new();
    html::push_html(&mut body_html, parser);

    let doc = format!(
        "<!doctype html>\n\
         <html lang=\"en\"><head>\n\
         <meta charset=\"UTF-8\">\n\
         <title>print test</title>\n\
         <style>{css}</style>\n\
         </head><body>\n\
         <article id=\"content\" class=\"md-rendered\" style=\"display:block\">{body}</article>\n\
         </body></html>\n",
        css = css,
        body = body_html
    );

    if let Err(e) = fs::write(out_path, doc) {
        eprintln!("failed to write {}: {}", out_path, e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
