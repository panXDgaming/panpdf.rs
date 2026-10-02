use std::fmt::Write as _;
use std::sync::Arc;

use convert_structure::model::{Block, Document, ListKind, Paragraph, Role, Run, Shift, Table};
use convert_structure::{FontProvider, Options, Output, Request};

pub use convert_structure;

const INLINE: &[char] = &['\\', '`', '*', '_', '[', ']', '<', '>', '|', '~'];

pub fn convert(
    pdf: Vec<u8>,
    request: &Request,
    fonts: Option<Arc<dyn FontProvider>>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String> {
    let document = convert_structure::read(
        pdf,
        request,
        fonts,
        &Options::default(),
        progress,
        &mut |_| {},
    )?;
    write_output(&document, request)
}

pub fn write_output(document: &Document, request: &Request) -> Result<Output, String> {
    let folder = if request.attachments.is_empty() {
        "files"
    } else {
        &request.attachments
    };
    let (main, attachments) = write(document, folder);
    Ok(Output {
        main: main.into_bytes(),
        attachments,
        notes: Vec::new(),
        dropped: document.running_heads(),
    })
}

convert_wasm::export_tool!(
    convert_structure::Options::default(),
    crate::write_output,
    "md"
);

fn escape(text: &str, out: &mut String) {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if matches!(c, '*' | '_') {
            let end = chars[at..].iter().take_while(|d| **d == c).count() + at;
            let before = at.checked_sub(1).map(|b| chars[b]);
            let after = chars.get(end).copied();
            let literal = (before.is_some_and(char::is_whitespace)
                && after.is_some_and(char::is_whitespace))
                || (c == '_'
                    && before.is_some_and(char::is_alphanumeric)
                    && after.is_some_and(char::is_alphanumeric));
            for _ in at..end {
                if !literal {
                    out.push('\\');
                }
                out.push(c);
            }
            at = end;
            continue;
        }
        if INLINE.contains(&c) {
            out.push('\\');
        }
        out.push(c);
        at += 1;
    }
}

fn guard_line_start(line: &mut String) {
    let trimmed = line.trim_start();
    let first = trimmed.chars().next();
    let needs = match first {
        Some('#' | '>' | '+' | '-' | '=') => true,
        Some(c) if c.is_ascii_digit() => {
            let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
            matches!(trimmed[digits..].chars().next(), Some('.' | ')'))
        }
        _ => false,
    };
    if needs {
        let at = line.len() - trimmed.len();
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            line.insert(at + digits, '\\');
        } else {
            line.insert(at, '\\');
        }
    }
}

fn inline(runs: &[Run]) -> String {
    let mut spans: Vec<(bool, bool, Shift, String)> = Vec::new();
    for run in runs {
        let key = (run.style.bold, run.style.italic, run.style.baseline);
        match spans.last_mut() {
            Some(last) if (last.0, last.1, last.2) == key => last.3.push_str(&run.text),
            _ => spans.push((key.0, key.1, key.2, run.text.clone())),
        }
    }
    let mut out = String::new();
    for (bold, italic, baseline, text) in spans {
        let text = text.replace('\n', " ");
        let core = text.trim();
        if core.is_empty() {
            out.push_str(&text);
            continue;
        }
        let lead = &text[..text.len() - text.trim_start().len()];
        let tail = &text[text.trim_end().len()..];
        let mark = match (bold, italic) {
            (true, true) => "***",
            (true, false) => "**",
            (false, true) => "*",
            (false, false) => "",
        };
        out.push_str(lead);
        match baseline {
            Shift::Up => out.push_str("<sup>"),
            Shift::Down => out.push_str("<sub>"),
            Shift::None => {}
        }
        out.push_str(mark);
        escape(core, &mut out);
        out.push_str(mark);
        match baseline {
            Shift::Up => out.push_str("</sup>"),
            Shift::Down => out.push_str("</sub>"),
            Shift::None => {}
        }
        out.push_str(tail);
    }
    out
}

fn paragraph(paragraph: &Paragraph, out: &mut String) {
    let text = inline(&paragraph.runs);
    if text.trim().is_empty() {
        return;
    }
    match &paragraph.role {
        Role::Heading(level) => {
            let plain: Vec<Run> = paragraph
                .runs
                .iter()
                .map(|run| {
                    let mut run = run.clone();
                    run.style.bold = false;
                    run
                })
                .collect();
            let _ = writeln!(
                out,
                "{} {}\n",
                "#".repeat(usize::from(*level)),
                inline(&plain).trim()
            );
        }
        Role::ListItem(item) => {
            let indent = "   ".repeat(usize::from(item.level));
            match item.kind {
                ListKind::Bullet => {
                    let _ = writeln!(out, "{indent}- {}", text.trim());
                }
                ListKind::Decimal(n) => {
                    let _ = writeln!(out, "{indent}{n}. {}", text.trim());
                }
                ListKind::LowerLetter(_) | ListKind::UpperLetter(_) => {
                    let mut marker = String::new();
                    escape(&item.marker, &mut marker);
                    let _ = writeln!(out, "{indent}- {marker} {}", text.trim());
                }
                ListKind::Kept => {
                    let _ = writeln!(out, "{indent}- {}", text.trim());
                }
            }
        }
        Role::Body => {
            let mut line = text.trim().to_owned();
            guard_line_start(&mut line);
            let _ = writeln!(out, "{line}\n");
        }
    }
}

fn cell_text(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|p| inline(&p.runs).trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("<br>")
}

fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
    out
}

fn table(table: &Table, out: &mut String) {
    let simple = table.rows.iter().all(|row| {
        row.cells
            .iter()
            .all(|cell| !cell.covered && cell.span == (1, 1))
    });
    if simple && !table.rows.is_empty() {
        for (r, row) in table.rows.iter().enumerate() {
            out.push('|');
            for cell in &row.cells {
                let _ = write!(out, " {} |", cell_text(&cell.paragraphs));
            }
            out.push('\n');
            if r == 0 {
                out.push('|');
                for _ in &row.cells {
                    out.push_str(" --- |");
                }
                out.push('\n');
            }
        }
        out.push('\n');
        return;
    }
    out.push_str("<table>\n");
    for row in &table.rows {
        out.push_str("<tr>");
        for cell in row.cells.iter().filter(|cell| !cell.covered) {
            let mut attributes = String::new();
            if cell.span.0 > 1 {
                let _ = write!(attributes, " colspan=\"{}\"", cell.span.0);
            }
            if cell.span.1 > 1 {
                let _ = write!(attributes, " rowspan=\"{}\"", cell.span.1);
            }
            let text: Vec<String> = cell
                .paragraphs
                .iter()
                .map(|p| html_escape(&p.text()))
                .collect();
            let _ = write!(out, "<td{attributes}>{}</td>", text.join("<br>"));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n\n");
}

#[must_use]
pub fn write(document: &Document, folder: &str) -> (String, Vec<(String, Vec<u8>)>) {
    let mut out = String::new();
    let mut attachments = Vec::new();
    let mut previous_list = false;
    for (number, page) in document.pages.iter().enumerate() {
        if number > 0 {
            let _ = writeln!(out, "<!-- page {} -->\n", page.index + 1);
        }
        for block in &page.blocks {
            let is_list =
                matches!(block, Block::Paragraph(p) if matches!(p.role, Role::ListItem(_)));
            if previous_list && !is_list {
                out.push('\n');
            }
            previous_list = is_list;
            match block {
                Block::Paragraph(p) => paragraph(p, &mut out),
                Block::Table(t) => table(t, &mut out),
                Block::Picture(picture) => {
                    if picture.background {
                        continue;
                    }
                    let name = format!(
                        "image{}.{}",
                        attachments.len() + 1,
                        picture.format.extension()
                    );
                    let _ = writeln!(out, "![]({folder}/{name})\n");
                    attachments.push((name, picture.data.clone()));
                }
            }
        }
    }
    (out, attachments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_structure::model::{ListItem, Page, Style};

    fn run(text: &str, bold: bool) -> Run {
        Run {
            text: text.into(),
            style: Style {
                bold,
                ..Style::default()
            },
        }
    }

    #[test]
    fn syntax_in_text_is_escaped() {
        let mut out = String::new();
        paragraph(
            &Paragraph {
                runs: vec![run("# not a heading *or* [link]", false)],
                ..Paragraph::default()
            },
            &mut out,
        );
        assert_eq!(out, "\\# not a heading \\*or\\* \\[link\\]\n\n");
        let mut out = String::new();
        escape("Name: ____ Date: __/__ file_name 2 * 3 _x_", &mut out);
        assert_eq!(
            out,
            "Name: ____ Date: \\_\\_/\\_\\_ file_name 2 * 3 \\_x\\_"
        );
        let mut line = "12. not a list".to_owned();
        guard_line_start(&mut line);
        assert_eq!(line, "12\\. not a list");
    }

    #[test]
    fn roles_become_markdown() {
        let document = Document {
            pages: vec![Page {
                blocks: vec![
                    Block::Paragraph(Paragraph {
                        role: Role::Heading(2),
                        runs: vec![run("ບົດທີ", true)],
                        ..Paragraph::default()
                    }),
                    Block::Paragraph(Paragraph {
                        role: Role::ListItem(ListItem {
                            marker: "3.".into(),
                            kind: ListKind::Decimal(3),
                            level: 0,
                        }),
                        runs: vec![run("ສາມ ", false), run("bold", true)],
                        ..Paragraph::default()
                    }),
                ],
                ..Page::default()
            }],
            body_size: 10.0,
        };
        let (text, _) = write(&document, "files");
        assert_eq!(text, "## ບົດທີ\n\n3. ສາມ **bold**\n");
    }
}
