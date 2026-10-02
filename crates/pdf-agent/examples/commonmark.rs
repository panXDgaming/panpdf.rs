use std::collections::BTreeMap;
use std::fmt::Write as _;

use pdf_agent::json::Json;
use pdf_agent::markup::tree::{Align, Block, Inline, List};
use pdf_agent::markup::{Options, blocks_with, plain};

struct Example {
    section: String,
    markdown: String,
    html: String,
    extension: Option<String>,
}

fn gfm_examples(text: &str) -> Vec<Example> {
    let fence = "`".repeat(32);
    let mut out = Vec::new();
    let mut section = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if let Some(title) = line.strip_prefix("## ").or_else(|| line.strip_prefix("# ")) {
            title.trim().clone_into(&mut section);
        }
        if !(line.starts_with(&fence) && line.contains("example")) {
            continue;
        }
        let disabled = line.contains("disabled");
        let extension = line
            .split_whitespace()
            .nth(2)
            .filter(|word| *word != "disabled")
            .map(str::to_owned);
        let mut markdown = String::new();
        let mut html = String::new();
        let mut in_html = false;
        for inner in lines.by_ref() {
            if inner.starts_with(&fence) {
                break;
            }
            if inner == "." && !in_html {
                in_html = true;
                continue;
            }
            let inner = inner.replace('\u{2192}', "\t");
            if in_html {
                html.push_str(&inner);
                html.push('\n');
            } else {
                markdown.push_str(&inner);
                markdown.push('\n');
            }
        }
        if !disabled {
            out.push(Example {
                section: section.clone(),
                markdown,
                html,
                extension,
            });
        }
    }
    out
}

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: commonmark <spec.json | spec.txt --gfm> [--show <section>]");
        return;
    };
    let mut gfm = false;
    let mut show = String::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--gfm" => gfm = true,
            "--show" => show = args.next().unwrap_or_default(),
            _ => {}
        }
    }
    let text = std::fs::read_to_string(&path).expect("the suite");
    let examples = if gfm {
        gfm_examples(&text)
    } else {
        let Json::List(examples) = Json::parse(&text).expect("the suite is JSON") else {
            eprintln!("the suite is a list of examples");
            return;
        };
        examples
            .iter()
            .filter_map(|example| {
                Some(Example {
                    markdown: example.get("markdown").and_then(Json::as_str)?.to_owned(),
                    html: example.get("html").and_then(Json::as_str)?.to_owned(),
                    section: example.get("section").and_then(Json::as_str)?.to_owned(),
                    extension: None,
                })
            })
            .collect()
    };
    let mut tally: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for example in &examples {
        let extended = example.extension.is_some();
        let options = if extended {
            Options::GFM
        } else {
            Options::COMMONMARK
        };
        let filter = example.extension.as_deref() == Some("tagfilter");
        let ours = html(&blocks_with(&example.markdown, options), filter);
        let row = tally.entry(example.section.clone()).or_default();
        row.1 += 1;
        if ours == example.html {
            row.0 += 1;
        } else if !show.is_empty() && (example.section == show || show == "all") {
            println!(
                "--- {:?}\nwant: {:?}\nours: {:?}\n",
                example.markdown, example.html, ours
            );
        }
    }
    let (mut passed, mut all) = (0, 0);
    println!("| section | read | of |");
    println!("|---|---:|---:|");
    for (section, (right, count)) in &tally {
        passed += right;
        all += count;
        println!("| {section} | {right} | {count} |");
    }
    let share = f64::from(u32::try_from(passed).unwrap_or(0)) * 100.0
        / f64::from(u32::try_from(all).unwrap_or(1));
    println!("| **all** | **{passed}** | **{all}** |");
    let which = if gfm {
        "GitHub Flavored Markdown 0.29-gfm"
    } else {
        "CommonMark 0.31.2"
    };
    println!("\n{share:.1}% of {which}");
}

fn html(blocks: &[Block], gfm: bool) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Heading { level, inlines } => {
                let _ = writeln!(out, "<h{level}>{}</h{level}>", line(inlines, gfm));
            }
            Block::Paragraph { inlines } => {
                let _ = writeln!(out, "<p>{}</p>", line(inlines, gfm));
            }
            Block::Code { info, text } => {
                let tongue = info.split_whitespace().next().unwrap_or_default();
                let opening = if tongue.is_empty() {
                    "<code>".to_owned()
                } else {
                    format!("<code class=\"language-{}\">", escaped(tongue))
                };
                let _ = writeln!(out, "<pre>{opening}{}</code></pre>", escaped(text));
            }
            Block::Break => out.push_str("<hr />\n"),
            Block::Quote { blocks } => {
                let _ = writeln!(out, "<blockquote>\n{}</blockquote>", html(blocks, gfm));
            }
            Block::List(list) => out.push_str(&listed(list, gfm)),
            Block::Table { head, rows, align } => {
                out.push_str("<table>\n<thead>\n<tr>\n");
                for (at, cell) in head.iter().enumerate() {
                    let _ = writeln!(
                        out,
                        "<th{}>{}</th>",
                        aligned(align.get(at)),
                        line(cell, gfm)
                    );
                }
                out.push_str("</tr>\n</thead>\n");
                if !rows.is_empty() {
                    out.push_str("<tbody>\n");
                    for row in rows {
                        out.push_str("<tr>\n");
                        for (at, cell) in row.iter().enumerate() {
                            let _ = writeln!(
                                out,
                                "<td{}>{}</td>",
                                aligned(align.get(at)),
                                line(cell, gfm)
                            );
                        }
                        out.push_str("</tr>\n");
                    }
                    out.push_str("</tbody>\n");
                }
                out.push_str("</table>\n");
            }
            Block::Html(raw) => {
                out.push_str(&filtered(raw, gfm));
                out.push('\n');
            }
            Block::Footnotes(_) => {}
        }
    }
    out
}

fn aligned(align: Option<&Align>) -> &'static str {
    match align {
        Some(Align::Left) => " align=\"left\"",
        Some(Align::Middle) => " align=\"center\"",
        Some(Align::End) => " align=\"right\"",
        _ => "",
    }
}

fn filtered(raw: &str, gfm: bool) -> String {
    if !gfm {
        return raw.to_owned();
    }
    let mut out = raw.to_owned();
    for tag in [
        "title",
        "textarea",
        "style",
        "xmp",
        "iframe",
        "noembed",
        "noframes",
        "script",
        "plaintext",
    ] {
        for opening in [format!("<{tag}"), format!("</{tag}")] {
            let mut at = 0;
            while let Some(found) = out[at..].to_ascii_lowercase().find(&opening) {
                let start = at + found;
                let after = out[start + opening.len()..].chars().next();
                if matches!(after, Some(' ' | '\t' | '\n' | '>' | '/')) || after.is_none() {
                    out.replace_range(start..=start, "&lt;");
                    at = start + 4;
                } else {
                    at = start + 1;
                }
            }
        }
    }
    out
}

fn listed(list: &List, gfm: bool) -> String {
    let mut out = String::new();
    match list.first {
        None => out.push_str("<ul>\n"),
        Some(1) => out.push_str("<ol>\n"),
        Some(first) => {
            let _ = writeln!(out, "<ol start=\"{first}\">");
        }
    }
    for item in &list.items {
        if item.is_empty() {
            out.push_str("<li></li>\n");
            continue;
        }
        let inside = if list.loose {
            format!("\n{}", html(item, gfm))
        } else {
            tight(item, gfm)
        };
        let _ = writeln!(out, "<li>{inside}</li>");
    }
    out.push_str(if list.first.is_some() {
        "</ol>\n"
    } else {
        "</ul>\n"
    });
    out
}

fn tight(item: &[Block], gfm: bool) -> String {
    let mut out = String::new();
    for (at, block) in item.iter().enumerate() {
        match block {
            Block::Paragraph { inlines } => {
                if at > 0 && !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&line(inlines, gfm));
            }
            other => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str(&html(std::slice::from_ref(other), gfm));
            }
        }
    }
    out
}

fn line(inlines: &[Inline], gfm: bool) -> String {
    let mut out = String::new();
    for inline in inlines {
        match inline {
            Inline::Text(text) => out.push_str(&escaped(text)),
            Inline::Soft => out.push('\n'),
            Inline::Hard => out.push_str("<br />\n"),
            Inline::Code(text) => {
                let _ = write!(out, "<code>{}</code>", escaped(text));
            }
            Inline::Html(raw) => out.push_str(&filtered(raw, gfm)),
            Inline::Emphasis(inside) => {
                let _ = write!(out, "<em>{}</em>", line(inside, gfm));
            }
            Inline::Strong(inside) => {
                let _ = write!(out, "<strong>{}</strong>", line(inside, gfm));
            }
            Inline::Strike(inside) => {
                let _ = write!(out, "<del>{}</del>", line(inside, gfm));
            }
            Inline::Highlight(inside) => {
                let _ = write!(out, "<mark>{}</mark>", line(inside, gfm));
            }
            Inline::Superscript(inside) => {
                let _ = write!(out, "<sup>{}</sup>", line(inside, gfm));
            }
            Inline::Subscript(inside) => {
                let _ = write!(out, "<sub>{}</sub>", line(inside, gfm));
            }
            Inline::Check(ticked) => {
                out.push_str(if *ticked {
                    "<input checked=\"\" disabled=\"\" type=\"checkbox\" /> "
                } else {
                    "<input disabled=\"\" type=\"checkbox\" /> "
                });
            }
            Inline::Note(number) => {
                let _ = write!(out, "<sup>{number}</sup>");
            }
            Inline::Link { to, title, text } => {
                let titled = if title.is_empty() {
                    String::new()
                } else {
                    format!(" title=\"{}\"", escaped(title))
                };
                let _ = write!(
                    out,
                    "<a href=\"{}\"{titled}>{}</a>",
                    escaped(to),
                    line(text, gfm)
                );
            }
            Inline::Image { at, title, text } => {
                let titled = if title.is_empty() {
                    String::new()
                } else {
                    format!(" title=\"{}\"", escaped(title))
                };
                let _ = write!(
                    out,
                    "<img src=\"{}\" alt=\"{}\"{titled} />",
                    escaped(at),
                    escaped(&plain(text))
                );
            }
        }
    }
    out
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
