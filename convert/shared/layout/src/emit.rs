use std::collections::HashMap;

use convert_pdf_canvas::{ImageId, Page, Rgb, Shaped, ShapedCluster, TextStyle};

use crate::fonts::FontBook;
use crate::model::Document;
use crate::page::{GlyphRun, Item, LaidPage};

fn colour(c: [u8; 3]) -> Rgb {
    Rgb::from_u8(c[0], c[1], c[2])
}

pub(crate) fn show(book: &FontBook, page: &mut Page, height: f32, run: &GlyphRun) {
    let face = book.face(run.face);
    let mut text = String::new();
    let mut clusters: Vec<ShapedCluster> = Vec::with_capacity(run.clusters.len());
    for (piece, shaped) in &run.clusters {
        let start = text.len();
        text.push_str(piece);
        let mut cluster = (**shaped).clone();
        cluster.range = start..text.len();
        clusters.push(cluster);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let shaped = Shaped {
        font: face.id,
        text,
        units_per_em: face.metrics.units_per_em as u16,
        clusters,
    };
    let count = shaped.clusters.len();
    let style = TextStyle {
        size: run.size,
        color: colour(run.colour),
        fake_bold: run.fake_bold,
        fake_italic: run.fake_italic,
    };
    if run.spoken {
        page.show_as(
            &shaped.text,
            &shaped,
            0..count,
            &style,
            run.x,
            height - run.y,
            run.word_spacing,
        );
    } else {
        page.show(
            &shaped,
            0..count,
            &style,
            run.x,
            height - run.y,
            run.word_spacing,
        );
    }
}

pub fn draw(
    book: &FontBook,
    page: &mut Page,
    height: f32,
    items: &[Item],
    images: &HashMap<usize, ImageId>,
) {
    draw_items(book, page, height, items, images, &|_, _, _| false);
}

type Vector<'a> = dyn Fn(&mut Page, usize, [f32; 4]) -> bool + 'a;

fn draw_items(
    book: &FontBook,
    page: &mut Page,
    height: f32,
    items: &[Item],
    images: &HashMap<usize, ImageId>,
    vector: &Vector<'_>,
) {
    for item in items {
        match item {
            Item::Glyphs(run) => show(book, page, height, run),
            Item::Rect {
                x,
                y,
                width,
                height: h,
                fill,
                stroke,
            } => {
                if let Some(fill) = fill {
                    page.fill_rect(*x, height - y - h, *width, *h, colour(*fill));
                }
                if let Some((w, c)) = stroke {
                    page.stroke_rect(*x, height - y - h, *width, *h, *w, colour(*c));
                }
            }
            Item::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                colour: c,
            } => {
                page.stroke_line(*x1, height - y1, *x2, height - y2, *width, colour(*c));
            }
            Item::Image {
                image,
                x,
                y,
                width,
                height: h,
            } => {
                if let Some(id) = images.get(image) {
                    page.image(*id, *x, height - y - h, *width, *h);
                } else {
                    vector(page, *image, [*x, height - y - h, *width, *h]);
                }
            }
            Item::Link {
                x,
                y,
                width,
                height: h,
                uri,
            } => {
                if !uri.starts_with('#') {
                    page.link(*x, height - y - h, *width, *h, uri);
                }
            }
        }
    }
}

pub fn write(doc: &Document, pages: &[LaidPage], book: FontBook) -> Result<Vec<u8>, String> {
    #[cfg(feature = "svg")]
    let mut drawings = crate::svg::prepare(doc, &book);
    let mut canvas = book.canvas.replace(convert_pdf_canvas::Canvas::new());
    let mut images: HashMap<usize, ImageId> = HashMap::new();
    for (index, image) in doc.images.iter().enumerate() {
        #[cfg(feature = "svg")]
        if drawings.contains_key(&index) {
            continue;
        }
        if let Ok(info) = canvas.add_image(&image.bytes) {
            images.insert(index, info.id);
        }
    }
    #[cfg(feature = "svg")]
    for drawing in drawings.values_mut() {
        drawing.add_images(&mut canvas);
    }
    #[cfg(feature = "svg")]
    let vector = |page: &mut Page, image: usize, rect: [f32; 4]| {
        crate::svg::draw(&drawings, &book, page, image, rect)
    };
    #[cfg(not(feature = "svg"))]
    let vector = |_: &mut Page, _: usize, _: [f32; 4]| false;
    if let Some(title) = &doc.title {
        canvas.set_title(title);
    }
    if let Some(lang) = &doc.lang {
        canvas.set_language(lang);
    }
    for laid in pages {
        let id = canvas.add_page(laid.width, laid.height);
        draw_items(
            &book,
            canvas.page(id),
            laid.height,
            &laid.items,
            &images,
            &vector,
        );
    }
    if pages.is_empty() {
        let setup = doc
            .sections
            .first()
            .map_or_else(crate::model::PageSetup::a4, |s| s.setup);
        canvas.add_page(setup.width, setup.height);
    }
    canvas.finish().map_err(|e| format!("{e:?}"))
}
