use std::collections::HashMap;

use convert_pdf_canvas::{Canvas, ImageInfo, Page, Rgb};
use convert_svg::{Drawing, Segment, TextPainter};

use crate::fonts::FontBook;
use crate::model::{Direction, Document, Inline, Paragraph, TextStyle};
use crate::page::Item;

pub(crate) struct Prepared {
    drawing: Drawing,
    texts: Vec<(f32, Vec<Item>)>,
    images: Vec<Option<ImageInfo>>,
}

impl Prepared {
    pub(crate) fn add_images(&mut self, canvas: &mut Canvas) {
        self.images = self
            .drawing
            .images()
            .iter()
            .map(|bytes| canvas.add_image(bytes).ok())
            .collect();
    }
}

pub(crate) fn prepare(doc: &Document, book: &FontBook) -> HashMap<usize, Prepared> {
    let mut out = HashMap::new();
    for (index, image) in doc.images.iter().enumerate() {
        if !convert_svg::is_svg(&image.bytes) {
            continue;
        }
        let Ok(drawing) = Drawing::parse(&image.bytes) else {
            continue;
        };
        let texts = drawing
            .segments()
            .iter()
            .map(|segment| lay_out(segment, book))
            .collect();
        out.insert(
            index,
            Prepared {
                drawing,
                texts,
                images: Vec::new(),
            },
        );
    }
    out
}

fn lay_out(segment: &Segment, book: &FontBook) -> (f32, Vec<Item>) {
    let inlines: Vec<Inline> = segment
        .pieces
        .iter()
        .filter(|p| p.size > 0.0 && p.size.is_finite())
        .map(|p| Inline::Text {
            text: p.text.clone(),
            style: TextStyle {
                family: p.family.clone(),
                size: p.size,
                bold: p.bold,
                italic: p.italic,
                underline: p.underline,
                strike: p.strike,
                colour: p.colour,
                letter_spacing: p.letter_spacing,
                ..TextStyle::default()
            },
        })
        .collect();
    if inlines.is_empty() {
        return (0.0, Vec::new());
    }
    let paragraph = Paragraph {
        inlines,
        direction: if segment.rtl {
            Direction::Rtl
        } else {
            Direction::Ltr
        },
        ..Paragraph::default()
    };
    let lines = crate::paragraph::lay_out(&paragraph, 1.0e7, book);
    let Some(line) = lines.first() else {
        return (0.0, Vec::new());
    };
    let baseline = line.baseline;
    let mut left = f32::INFINITY;
    let mut right = f32::NEG_INFINITY;
    for item in &line.items {
        if let Item::Glyphs(run) = item {
            left = left.min(run.x);
            right = right.max(run.x + run.width);
        }
    }
    if !left.is_finite() {
        return (0.0, Vec::new());
    }
    let items = line
        .items
        .iter()
        .filter(|i| matches!(i, Item::Glyphs(_) | Item::Line { .. }))
        .map(|i| i.clone().moved(-left, -baseline))
        .collect();
    (right - left, items)
}

struct Painter<'a> {
    book: &'a FontBook,
    texts: &'a [(f32, Vec<Item>)],
}

impl TextPainter for Painter<'_> {
    fn width(&self, index: usize) -> f32 {
        self.texts.get(index).map_or(0.0, |t| t.0)
    }

    fn draw(&self, page: &mut Page, index: usize) {
        let Some((_, items)) = self.texts.get(index) else {
            return;
        };
        for item in items {
            match item {
                Item::Glyphs(run) => crate::emit::show(self.book, page, 0.0, run),
                Item::Line {
                    x1,
                    y1,
                    x2,
                    y2,
                    width,
                    colour,
                } => {
                    let c = Rgb::from_u8(colour[0], colour[1], colour[2]);
                    page.stroke_line(*x1, -y1, *x2, -y2, *width, c);
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn draw(
    drawings: &HashMap<usize, Prepared>,
    book: &FontBook,
    page: &mut Page,
    image: usize,
    rect: [f32; 4],
) -> bool {
    let Some(prepared) = drawings.get(&image) else {
        return false;
    };
    let painter = Painter {
        book,
        texts: &prepared.texts,
    };
    prepared
        .drawing
        .draw(page, rect, &painter, &prepared.images);
    true
}
