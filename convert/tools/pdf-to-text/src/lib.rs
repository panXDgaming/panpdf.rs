use std::sync::Arc;

use convert_structure::model::{Block, Document, Paragraph};
use convert_structure::{FontProvider, Options, Output, Request};

pub use convert_structure;

pub const OPTIONS: Options = Options {
    pictures: false,
    tables: true,
};

pub fn convert(
    pdf: Vec<u8>,
    request: &Request,
    fonts: Option<Arc<dyn FontProvider>>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String> {
    let document = convert_structure::read(pdf, request, fonts, &OPTIONS, progress, &mut |_| {})?;
    write_output(&document, request)
}

pub fn write_output(document: &Document, _request: &Request) -> Result<Output, String> {
    Ok(Output {
        main: write(document).into_bytes(),
        attachments: Vec::new(),
        notes: Vec::new(),
        dropped: String::new(),
    })
}

convert_wasm::export_tool!(crate::OPTIONS, crate::write_output, "txt");

fn line_of(paragraph: &Paragraph) -> String {
    paragraph.text_with_marker().replace('\n', " ")
}

#[must_use]
pub fn write(document: &Document) -> String {
    let mut out = String::new();
    for (number, page) in document.pages.iter().enumerate() {
        if number > 0 {
            out.push('\u{c}');
        }
        let mut paragraphs: Vec<String> = Vec::new();
        paragraphs.extend(page.header.iter().map(line_of));
        for block in &page.blocks {
            match block {
                Block::Paragraph(paragraph) => paragraphs.push(line_of(paragraph)),
                Block::Table(table) => {
                    let rows: Vec<String> = table
                        .rows
                        .iter()
                        .map(|row| {
                            row.cells
                                .iter()
                                .filter(|cell| !cell.covered)
                                .map(|cell| {
                                    cell.paragraphs
                                        .iter()
                                        .map(line_of)
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                })
                                .collect::<Vec<_>>()
                                .join("\t")
                        })
                        .collect();
                    paragraphs.push(rows.join("\n"));
                }
                Block::Picture(_) => {}
            }
        }
        paragraphs.extend(page.footer.iter().map(line_of));
        paragraphs.retain(|text| !text.trim().is_empty());
        out.push_str(&paragraphs.join("\n\n"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_structure::model::{Page, Run};

    #[test]
    fn pages_are_separated_by_form_feeds() {
        let paragraph = |text: &str| {
            Block::Paragraph(Paragraph {
                runs: vec![Run {
                    text: text.into(),
                    ..Run::default()
                }],
                ..Paragraph::default()
            })
        };
        let document = Document {
            pages: vec![
                Page {
                    blocks: vec![paragraph("ໜຶ່ງ"), paragraph("two")],
                    ..Page::default()
                },
                Page {
                    blocks: vec![paragraph("สาม")],
                    ..Page::default()
                },
            ],
            body_size: 10.0,
        };
        assert_eq!(write(&document), "ໜຶ່ງ\n\ntwo\n\u{c}สาม\n");
    }
}
