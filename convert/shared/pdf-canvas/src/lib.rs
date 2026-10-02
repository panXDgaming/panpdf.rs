pub mod bidi;
pub mod families;
pub mod font;
pub mod gradient;
pub mod image;
pub mod page;
pub mod rtl;
pub mod text;
mod unicode_data;
mod write;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

pub use families::{FontBook, Pick};
pub use font::{FontError, FontId, FontMetrics, Glyph, Shaped, ShapedCluster, graphemes};
pub use gradient::{Gradient, GradientKind};
pub use image::ImageError;
pub use page::{Cap, Page, Rgb, TextStyle};
pub use text::{Direction, Line, Piece, Span, layout, layout_directed};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(pub(crate) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub id: ImageId,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageId(pub usize);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasError {
    Subset(String),
    TooManyGlyphs,
}

impl std::fmt::Display for CanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Subset(why) => write!(f, "a font could not be embedded: {why}"),
            Self::TooManyGlyphs => f.write_str("more than 65535 distinct glyphs in one font"),
        }
    }
}

impl std::error::Error for CanvasError {}

#[derive(Default)]
pub(crate) struct Cids {
    pub(crate) by_key: HashMap<(u16, String, i32), u16>,
    pub(crate) glyphs: Vec<(u16, String, i32)>,
    pub(crate) overflow: bool,
    pub(crate) fixed: std::collections::BTreeMap<u16, (u16, String, i32)>,
    pub(crate) codes: HashMap<u16, u16>,
}

#[derive(Default)]
pub(crate) struct Fonts {
    faces: Vec<font::Face>,
    cids: Vec<Cids>,
}

impl Fonts {
    pub(crate) fn advance(&self, font: usize, gid: u16) -> i32 {
        self.faces[font].advance(gid)
    }

    pub(crate) fn cid(&mut self, font: usize, gid: u16, text: &str, width: i32) -> (u16, i32) {
        let cids = &mut self.cids[font];
        if self.faces[font].cff().is_some() {
            let face = &self.faces[font];
            let code = *cids
                .codes
                .entry(gid)
                .or_insert_with(|| face.fixed_code(gid).unwrap_or(gid));
            let entry = cids
                .fixed
                .entry(code)
                .or_insert_with(|| (gid, text.to_owned(), width));
            return (code, entry.2);
        }
        let key = (gid, text.to_owned(), width);
        if let Some(&cid) = cids.by_key.get(&key) {
            return (cid, width);
        }
        let Ok(cid) = u16::try_from(cids.glyphs.len() + 1) else {
            cids.overflow = true;
            return (0, width);
        };
        cids.glyphs.push(key.clone());
        cids.by_key.insert(key, cid);
        (cid, width)
    }
}

pub struct Canvas {
    fonts: Rc<RefCell<Fonts>>,
    images: Vec<image::Picture>,
    pages: Vec<Page>,
    title: Option<String>,
    language: Option<String>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: Rc::new(RefCell::new(Fonts::default())),
            images: Vec::new(),
            pages: Vec::new(),
            title: None,
            language: None,
        }
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }

    pub fn set_language(&mut self, tag: &str) {
        self.language = Some(tag.to_owned());
    }

    pub fn add_font(&mut self, bytes: Vec<u8>, face_index: u32) -> Result<FontId, FontError> {
        let face = font::Face::parse(bytes, face_index)?;
        let mut fonts = self.fonts.borrow_mut();
        fonts.faces.push(face);
        fonts.cids.push(Cids::default());
        Ok(FontId(fonts.faces.len() - 1))
    }

    pub fn add_program(
        &mut self,
        program: std::sync::Arc<pdf_font::glyph::GlyphProgram>,
        face_index: u32,
    ) -> Result<FontId, FontError> {
        let face = font::Face::from_program(program, face_index)?;
        let mut fonts = self.fonts.borrow_mut();
        fonts.faces.push(face);
        fonts.cids.push(Cids::default());
        Ok(FontId(fonts.faces.len() - 1))
    }

    #[must_use]
    pub fn font_count(&self) -> usize {
        self.fonts.borrow().faces.len()
    }

    #[must_use]
    pub fn font_metrics(&self, font: FontId) -> FontMetrics {
        self.fonts.borrow().faces[font.0].metrics.clone()
    }

    #[must_use]
    pub fn has_char(&self, font: FontId, c: char) -> bool {
        self.fonts.borrow().faces[font.0].has_char(c)
    }

    #[must_use]
    pub fn shape(&self, font: FontId, text: &str) -> Shaped {
        self.fonts.borrow().faces[font.0].shape(font, text)
    }

    #[must_use]
    pub fn shape_rtl(&self, font: FontId, text: &str) -> Shaped {
        self.fonts.borrow().faces[font.0].shape_rtl(font, text)
    }

    #[must_use]
    pub fn measure(&self, font: FontId, text: &str, size: f32) -> f32 {
        self.shape(font, text).total_width(size)
    }

    pub fn add_image(&mut self, bytes: &[u8]) -> Result<ImageInfo, ImageError> {
        let picture = image::read(bytes)?;
        let info = ImageInfo {
            id: ImageId(self.images.len()),
            width: picture.width,
            height: picture.height,
        };
        self.images.push(picture);
        Ok(info)
    }

    pub fn add_page(&mut self, width: f32, height: f32) -> PageId {
        self.pages
            .push(Page::new(width, height, Rc::clone(&self.fonts)));
        PageId(self.pages.len() - 1)
    }

    pub fn page(&mut self, id: PageId) -> &mut Page {
        &mut self.pages[id.0]
    }

    #[must_use]
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn finish(self) -> Result<Vec<u8>, CanvasError> {
        let fonts = self.fonts.borrow();
        if fonts.cids.iter().any(|c| c.overflow) {
            return Err(CanvasError::TooManyGlyphs);
        }
        let used: BTreeSet<usize> = self
            .pages
            .iter()
            .flat_map(|p| p.fonts_used.iter().copied())
            .collect();
        write::document(
            &self.pages,
            &fonts.faces,
            &fonts.cids,
            &used,
            &self.images,
            self.title.as_deref(),
            self.language.as_deref(),
        )
    }
}
