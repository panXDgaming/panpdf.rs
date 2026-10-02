#![forbid(unsafe_code)]

pub mod emit;
pub mod flow;
pub mod fonts;
pub mod model;
pub mod numbering;
pub mod page;
pub mod paragraph;
#[cfg(feature = "svg")]
mod svg;

pub use convert_pdf_canvas::bidi;
pub use fonts::FontBook;
pub use model::Document;
pub use page::LaidPage;

pub fn render(doc: &Document, book: FontBook) -> Result<Vec<u8>, String> {
    if book.is_empty() {
        return Err("no fonts were given".into());
    }
    let pages = flow::paginate(doc, &book);
    emit::write(doc, &pages, book)
}

#[must_use]
pub fn font_book(
    provider: Option<std::sync::Arc<dyn pdf_font::substitute::FontProvider>>,
) -> FontBook {
    FontBook::new(provider)
}

#[must_use]
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let w = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
        let h = u16::from_le_bytes(bytes.get(8..10)?.try_into().ok()?);
        return (w > 0 && h > 0).then_some((u32::from(w), u32::from(h)));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        return Some((w, h));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut at = 2;
        while at + 9 < bytes.len() {
            if bytes[at] != 0xFF {
                at += 1;
                continue;
            }
            let marker = bytes[at + 1];
            if marker == 0xFF {
                at += 1;
                continue;
            }
            let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
            if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                let h = u16::from_be_bytes([bytes[at + 5], bytes[at + 6]]);
                let w = u16::from_be_bytes([bytes[at + 7], bytes[at + 8]]);
                return Some((u32::from(w), u32::from(h)));
            }
            at += 2 + length;
        }
    }
    None
}
