use std::collections::HashMap;

use convert_drawingml::chart::simple_format;
use convert_drawingml::geometry::num;
use convert_drawingml::{IDENTITY, Inherit, Scene};
use convert_office_read::{Element, Package};
use convert_pdf_canvas::{Canvas, FontBook, ImageInfo, PageId};

use crate::theme::Theme;

pub struct Parts {
    pub slide: Element,
    pub slide_part: String,
    pub layout: Option<Element>,
    pub layout_part: String,
    pub master: Option<Element>,
    pub master_part: String,
    pub theme: Theme,
    pub map: HashMap<String, String>,
}

impl Inherit for Parts {
    fn placeholders(&self, ph: &Element) -> [Option<&Element>; 2] {
        let kind = ph.attr("type").unwrap_or("obj");
        let idx = ph.attr("idx");
        [
            find_ph(self.master.as_ref(), kind, idx),
            find_ph(self.layout.as_ref(), kind, idx),
        ]
    }

    fn master_style(&self, kind: Option<&str>) -> Option<&Element> {
        let styles = self.master.as_ref()?.child("txStyles")?;
        match kind {
            Some("title" | "ctrTitle") => styles.child("titleStyle"),
            Some("body" | "subTitle" | "obj") => styles.child("bodyStyle"),
            Some(_) => styles.child("otherStyle"),
            None => None,
        }
    }
}

pub struct Slide<'a, 'b> {
    pub canvas: &'a mut Canvas,
    pub fonts: &'a mut FontBook,
    pub package: &'a Package<'b>,
    pub images: &'a mut HashMap<String, Option<ImageInfo>>,
    pub default_text: Option<&'a Element>,
    pub parts: &'a Parts,
    pub page: PageId,
    pub height: f32,
    pub notes: &'a mut Vec<String>,
}

impl Slide<'_, '_> {
    pub fn draw(&mut self) {
        let parts = self.parts;
        let (w, h) = (self.canvas.page(self.page).width, self.height);
        let mut scene = Scene {
            canvas: &mut *self.canvas,
            fonts: &mut *self.fonts,
            package: self.package,
            images: &mut *self.images,
            theme: &parts.theme,
            map: &parts.map,
            page: self.page,
            base: [1.0, 0.0, 0.0, -1.0, 0.0, h],
            notes: &mut *self.notes,
            default_size: 18.0,
            default_text: self.default_text,
            format: &simple_format,
            hidden: &|_| None,
        };
        let bg = [
            Some(&parts.slide),
            parts.layout.as_ref(),
            parts.master.as_ref(),
        ]
        .into_iter()
        .zip([&parts.slide_part, &parts.layout_part, &parts.master_part])
        .find_map(|(root, part)| root?.path(&["cSld", "bg"]).map(|bg| (bg, part.clone())));
        if let Some((bg, part)) = bg {
            background(&mut scene, bg, &part, w, h);
        }
        let show =
            |root: Option<&Element>| root.is_none_or(|r| r.attr("showMasterSp") != Some("0"));
        if show(Some(&parts.slide)) {
            if show(parts.layout.as_ref())
                && let Some(tree) = parts
                    .master
                    .as_ref()
                    .and_then(|m| m.path(&["cSld", "spTree"]))
            {
                scene.tree(tree, &parts.master_part, IDENTITY, false, parts);
            }
            if let Some(tree) = parts
                .layout
                .as_ref()
                .and_then(|m| m.path(&["cSld", "spTree"]))
            {
                scene.tree(tree, &parts.layout_part, IDENTITY, false, parts);
            }
        }
        if let Some(tree) = parts.slide.path(&["cSld", "spTree"]) {
            scene.tree(tree, &parts.slide_part, IDENTITY, true, parts);
        }
    }
}

fn background(scene: &mut Scene<'_, '_>, bg: &Element, part: &str, w: f32, h: f32) {
    let (fill, placeholder) = if let Some(pr) = bg.child("bgPr") {
        (pr.elements().next().cloned(), None)
    } else if let Some(r) = bg.child("bgRef") {
        let idx = num(r, "idx").unwrap_or(0.0) as usize;
        let ph = scene.palette(None).first_color(r);
        let list = if idx >= 1001 {
            &scene.theme.bg_fills
        } else {
            &scene.theme.fills
        };
        let slot = if idx >= 1001 {
            idx - 1001
        } else {
            idx.saturating_sub(1)
        };
        (list.get(slot).cloned(), ph)
    } else {
        (None, None)
    };
    let Some(fill) = fill else { return };
    scene.page().save();
    scene.apply(IDENTITY, h);
    scene.page().rect(0.0, 0.0, w, h);
    scene.paint_fill(&fill, part, placeholder, w, h);
    scene.page().restore();
}

fn find_ph<'p>(root: Option<&'p Element>, kind: &str, idx: Option<&str>) -> Option<&'p Element> {
    let tree = root?.path(&["cSld", "spTree"])?;
    let shapes: Vec<&'p Element> = tree.elements().filter(|e| e.local() == "sp").collect();
    let ph_of = |s: &Element| s.path(&["nvSpPr", "nvPr", "ph"]).cloned();
    let norm = |t: &str| {
        match t {
            "ctrTitle" => "title",
            "subTitle" | "obj" => "body",
            other => other,
        }
        .to_owned()
    };
    let same_kind = |t: &str| norm(t) == norm(kind);
    shapes
        .iter()
        .find(|s| {
            ph_of(s).is_some_and(|p| {
                idx.is_some() && p.attr("idx") == idx && p.attr("type").is_none_or(same_kind)
            })
        })
        .or_else(|| {
            shapes
                .iter()
                .find(|s| ph_of(s).is_some_and(|p| p.attr("type") == Some(kind)))
        })
        .or_else(|| {
            shapes
                .iter()
                .find(|s| ph_of(s).is_some_and(|p| same_kind(p.attr("type").unwrap_or("obj"))))
        })
        .copied()
}
