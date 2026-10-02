use std::fmt::Write as _;
use std::sync::Arc;

use convert_structure::model::{
    Align, Block, Document, ListKind, Paragraph, Role, Run, Shift, Table,
};
use convert_structure::script::{self, Script};
use convert_structure::{FontProvider, Options, Output, Request};
use convert_xml::escape_into;

pub use convert_structure;

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

pub fn write_output(document: &Document, _request: &Request) -> Result<Output, String> {
    Ok(Output {
        main: write(document).into_bytes(),
        attachments: Vec::new(),
        notes: Vec::new(),
        dropped: document.running_heads(),
    })
}

convert_wasm::export_tool!(
    convert_structure::Options::default(),
    crate::write_output,
    "html"
);

const STYLE: &str = "body{font-family:\"Phetsarath OT\",\"Saysettha OT\",\"Noto Sans Lao\",\"Leelawadee UI\",\"Noto Sans Thai\",system-ui,sans-serif;line-height:1.6;max-width:50em;margin:2em auto;padding:0 1em;color:#111;background:#fff}\
table{border-collapse:collapse;margin:1em 0}td{border:1px solid #888;padding:.2em .4em;vertical-align:top}\
img{max-width:100%;height:auto}hr.page{border:0;border-top:1px dashed #bbb;margin:2em 0}\
@media print{hr.page{display:none}section{break-after:page}body{max-width:none;margin:0}}";

#[must_use]
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn language(script: Script) -> Option<&'static str> {
    script
        .language()
        .map(|tag| tag.split('-').next().unwrap_or(tag))
}

fn document_language(document: &Document) -> &'static str {
    let mut text = String::new();
    for page in &document.pages {
        for block in &page.blocks {
            if let Block::Paragraph(paragraph) = block {
                text.push_str(&paragraph.text());
            }
        }
    }
    language(script::dominant(&text)).unwrap_or("en")
}

fn inline(runs: &[Run], out: &mut String) {
    for run in runs {
        let mut open = Vec::new();
        if run.style.bold {
            open.push("b");
        }
        if run.style.italic {
            open.push("i");
        }
        if run.style.underline {
            open.push("u");
        }
        match run.style.baseline {
            Shift::Up => open.push("sup"),
            Shift::Down => open.push("sub"),
            Shift::None => {}
        }
        for tag in &open {
            let _ = write!(out, "<{tag}>");
        }
        let colored = run.style.color != [0, 0, 0];
        if colored {
            let [r, g, b] = run.style.color;
            let _ = write!(out, "<span style=\"color:#{r:02x}{g:02x}{b:02x}\">");
        }
        escape_into(out, &run.text);
        if colored {
            out.push_str("</span>");
        }
        for tag in open.iter().rev() {
            let _ = write!(out, "</{tag}>");
        }
    }
}

fn attributes(paragraph: &Paragraph, page_language: &str) -> String {
    let mut out = String::new();
    if let Some(lang) = language(script::dominant(&paragraph.text()))
        && lang != page_language
    {
        let _ = write!(out, " lang=\"{lang}\"");
    }
    match paragraph.align {
        Align::Center => out.push_str(" style=\"text-align:center\""),
        Align::Right => out.push_str(" style=\"text-align:right\""),
        Align::Justify => out.push_str(" style=\"text-align:justify\""),
        Align::Left => {}
    }
    out
}

struct Lists {
    open: Vec<&'static str>,
}

impl Lists {
    fn close_all(&mut self, out: &mut String) {
        while let Some(tag) = self.open.pop() {
            let _ = write!(out, "</{tag}>");
        }
    }
}

fn paragraph(paragraph: &Paragraph, lists: &mut Lists, language: &str, out: &mut String) {
    let attributes = attributes(paragraph, language);
    match &paragraph.role {
        Role::Heading(level) => {
            lists.close_all(out);
            let _ = write!(out, "<h{level}{attributes}>");
            inline(&paragraph.runs, out);
            let _ = writeln!(out, "</h{level}>");
        }
        Role::ListItem(item) => {
            let (tag, kind) = match item.kind {
                ListKind::Bullet => ("ul", ""),
                ListKind::Decimal(_) => ("ol", ""),
                ListKind::LowerLetter(_) => ("ol", " type=\"a\""),
                ListKind::UpperLetter(_) => ("ol", " type=\"A\""),
                ListKind::Kept => ("ul", " style=\"list-style:none\""),
            };
            let depth = usize::from(item.level) + 1;
            while lists.open.len() > depth {
                let _ = write!(out, "</{}>", lists.open.pop().unwrap_or("ul"));
            }
            if lists.open.len() == depth && lists.open.last() != Some(&tag) {
                let _ = write!(out, "</{}>", lists.open.pop().unwrap_or("ul"));
            }
            while lists.open.len() < depth {
                let start = match item.kind {
                    ListKind::Decimal(n) | ListKind::LowerLetter(n) | ListKind::UpperLetter(n)
                        if n != 1 =>
                    {
                        format!(" start=\"{n}\"")
                    }
                    _ => String::new(),
                };
                let _ = write!(out, "<{tag}{kind}{start}>");
                lists.open.push(tag);
            }
            let _ = write!(out, "<li{attributes}>");
            inline(&paragraph.runs, out);
            out.push_str("</li>\n");
        }
        Role::Body => {
            lists.close_all(out);
            let _ = write!(out, "<p{attributes}>");
            inline(&paragraph.runs, out);
            out.push_str("</p>\n");
        }
    }
}

fn table(table: &Table, language: &str, out: &mut String) {
    out.push_str("<table>\n");
    for row in &table.rows {
        out.push_str("<tr>");
        for cell in row.cells.iter().filter(|cell| !cell.covered) {
            out.push_str("<td");
            if cell.span.0 > 1 {
                let _ = write!(out, " colspan=\"{}\"", cell.span.0);
            }
            if cell.span.1 > 1 {
                let _ = write!(out, " rowspan=\"{}\"", cell.span.1);
            }
            if let Some([r, g, b]) = cell.fill {
                let _ = write!(out, " style=\"background:#{r:02x}{g:02x}{b:02x}\"");
            }
            out.push('>');
            let mut lists = Lists { open: Vec::new() };
            for (at, p) in cell.paragraphs.iter().enumerate() {
                if at > 0 && matches!(p.role, Role::Body) {
                    out.push_str("<br>");
                }
                if matches!(p.role, Role::Body) {
                    inline(&p.runs, out);
                } else {
                    paragraph(p, &mut lists, language, out);
                }
            }
            lists.close_all(out);
            out.push_str("</td>");
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
}

#[must_use]
pub fn write(document: &Document) -> String {
    let language = document_language(document);
    let mut out = String::with_capacity(64 * 1024);
    let _ = write!(
        out,
        "<!DOCTYPE html>\n<html lang=\"{language}\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"generator\" content=\"PanPDF\">\n<title>"
    );
    let title = document
        .pages
        .iter()
        .flat_map(|page| &page.blocks)
        .find_map(|block| match block {
            Block::Paragraph(p) if matches!(p.role, Role::Heading(_)) => Some(p.text()),
            _ => None,
        })
        .unwrap_or_default();
    escape_into(&mut out, title.trim());
    let _ = write!(out, "</title>\n<style>{STYLE}</style>\n</head>\n<body>\n");
    for (number, page) in document.pages.iter().enumerate() {
        if number > 0 {
            out.push_str("<hr class=\"page\">\n");
        }
        let _ = writeln!(out, "<section id=\"page-{}\">", page.index + 1);
        let mut lists = Lists { open: Vec::new() };
        for block in &page.blocks {
            match block {
                Block::Paragraph(p) => paragraph(p, &mut lists, language, &mut out),
                Block::Table(t) => {
                    lists.close_all(&mut out);
                    table(t, language, &mut out);
                }
                Block::Picture(picture) => {
                    if picture.background {
                        continue;
                    }
                    lists.close_all(&mut out);
                    let _ = writeln!(
                        out,
                        "<p><img width=\"{:.0}\" height=\"{:.0}\" alt=\"\" src=\"data:{};base64,{}\"></p>",
                        picture.frame.width(),
                        picture.frame.height(),
                        picture.format.mime(),
                        base64(&picture.data)
                    );
                }
            }
        }
        lists.close_all(&mut out);
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_structure::model::{ListItem, Page, Style};

    #[test]
    fn base64_known_answers() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn lists_open_and_close() {
        let item = |text: &str, n: u32| {
            Block::Paragraph(Paragraph {
                role: Role::ListItem(ListItem {
                    marker: format!("{n}."),
                    kind: ListKind::Decimal(n),
                    level: 0,
                }),
                runs: vec![Run {
                    text: text.into(),
                    style: Style::default(),
                }],
                ..Paragraph::default()
            })
        };
        let document = Document {
            pages: vec![Page {
                blocks: vec![item("ໜຶ່ງ", 1), item("ສອງ & <b>", 2)],
                ..Page::default()
            }],
            body_size: 10.0,
        };
        let html = write(&document);
        assert!(html.contains("<html lang=\"lo\">"));
        assert!(html.contains("<ol><li>ໜຶ່ງ</li>\n<li>ສອງ &amp; &lt;b&gt;</li>\n</ol>"));
    }
}
