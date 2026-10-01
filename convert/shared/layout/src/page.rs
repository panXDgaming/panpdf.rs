use std::rc::Rc;

use convert_pdf_canvas::ShapedCluster;

use crate::fonts::FaceId;
use crate::model::{ImageRef, Rgb};

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphRun {
    pub face: FaceId,
    pub size: f32,
    pub colour: Rgb,
    pub fake_bold: bool,
    pub fake_italic: bool,
    pub x: f32,
    pub y: f32,
    pub word_spacing: f32,
    pub width: f32,
    pub clusters: Vec<(String, Rc<ShapedCluster>)>,
    pub spoken: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Glyphs(GlyphRun),
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill: Option<Rgb>,
        stroke: Option<(f32, Rgb)>,
    },
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        colour: Rgb,
    },
    Image {
        image: ImageRef,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Link {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        uri: String,
    },
}

impl Item {
    #[must_use]
    pub fn moved(mut self, dx: f32, dy: f32) -> Self {
        match &mut self {
            Self::Glyphs(run) => {
                run.x += dx;
                run.y += dy;
            }
            Self::Rect { x, y, .. } | Self::Image { x, y, .. } | Self::Link { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Self::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaidPage {
    pub width: f32,
    pub height: f32,
    pub items: Vec<Item>,
}

impl LaidPage {
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            if let Item::Glyphs(run) = item {
                for (text, _) in &run.clusters {
                    out.push_str(text);
                }
            }
        }
        out
    }
}
