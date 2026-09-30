use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use convert_pdf_canvas::families::{FontBook as Families, normalize};
use convert_pdf_canvas::{Canvas, FontId, FontMetrics, ShapedCluster};
use pdf_font::substitute::FontProvider;

use crate::model::TextStyle;

struct Listed {
    present: Vec<(String, convert_pdf_canvas::families::Pick)>,
    primary: String,
}

pub type FaceId = usize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceMetrics {
    pub units_per_em: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
}

impl FaceMetrics {
    #[must_use]
    pub fn line(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

#[derive(Clone, Debug)]
pub struct Face {
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    pub id: FontId,
    pub metrics: FaceMetrics,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pick {
    pub face: FaceId,
    pub scale: f32,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

type PickKey = (String, bool, bool, String);

pub type Part = (String, Rc<ShapedCluster>);

pub type RtlWord = Rc<Vec<Part>>;

pub struct FontBook {
    pub canvas: RefCell<Canvas>,
    families: RefCell<Families>,
    faces: RefCell<Vec<Face>>,
    by_font: RefCell<HashMap<FontId, FaceId>>,
    shaped: RefCell<HashMap<(FaceId, String), Rc<ShapedCluster>>>,
    shaped_rtl: RefCell<HashMap<(FaceId, String), RtlWord>>,
    picks: RefCell<HashMap<PickKey, Option<Pick>>>,
    lists: RefCell<HashMap<(String, bool, bool), Rc<Listed>>>,
    has_provider: bool,
}

impl std::fmt::Debug for FontBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.faces.borrow().iter()).finish()
    }
}

fn metrics_of(m: &FontMetrics) -> FaceMetrics {
    let units = f32::from(m.units_per_em.max(1));
    #[allow(clippy::cast_precision_loss)]
    let f = |v: i32| v as f32 / units;
    let (mut ascent, mut descent) = if m.ascent > m.descent {
        (f(m.ascent), -f(m.descent))
    } else {
        (0.9, 0.25)
    };
    let mut line_gap = f(m.line_gap).max(0.0);
    let (win_ascent, win_descent) = win_of(m);
    if win_ascent > 0.0 {
        let (wa, wd) = (win_ascent / units, win_descent / units);
        if wa > ascent + line_gap / 2.0 {
            ascent = wa;
        }
        if wd > descent + line_gap / 2.0 {
            descent = wd;
        }
        if wa + wd >= ascent + descent + line_gap - 0.001 {
            line_gap = 0.0;
        }
    } else if ascent + descent + line_gap < 1.05
        && (m.family.contains("Saysettha") || m.family.contains("Phetsarath"))
    {
        ascent += 0.4;
        descent += 0.35;
    }
    FaceMetrics {
        units_per_em: units,
        ascent,
        descent,
        line_gap,
        underline_position: if m.underline_position == 0 {
            -0.1
        } else {
            f(m.underline_position)
        },
        underline_thickness: if m.underline_thickness <= 0 {
            0.05
        } else {
            f(m.underline_thickness)
        },
    }
}

#[allow(clippy::cast_precision_loss)]
const fn win_of(m: &FontMetrics) -> (f32, f32) {
    (m.win_ascent as f32, m.win_descent as f32)
}

fn normal(name: &str) -> String {
    name.trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
        .replace(['-', '_'], " ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Class {
    Lao,
    Thai,
    Cjk,
    Other,
}

fn class_of(c: char) -> Class {
    match u32::from(c) {
        0x0E80..=0x0EFF => Class::Lao,
        0x0E00..=0x0E7F => Class::Thai,
        0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xFF00..=0xFFEF => {
            Class::Cjk
        }
        _ => Class::Other,
    }
}

#[must_use]
pub fn is_complex(c: char) -> bool {
    matches!(class_of(c), Class::Lao | Class::Thai) || convert_pdf_canvas::bidi::is_rtl(c)
}

fn thai_scale(family: &str) -> f32 {
    const SMALL: &[&str] = &[
        "th sarabun new",
        "th sarabunpsk",
        "th sarabun psk",
        "angsana new",
        "angsanaupc",
        "cordia new",
        "cordiaupc",
        "browallia new",
        "browalliaupc",
        "th niramit as",
        "th krub",
        "th k2d july8",
        "th chakra petch",
        "th baijam",
        "th charmonman",
        "th fah kwang",
        "th kodchasal",
        "th koho",
        "th mali grade6",
        "th srisakdi",
        "dilleniaupc",
        "eucrosiaupc",
        "freesiaupc",
        "irisupc",
        "jasmineupc",
        "kodchiangupc",
        "lilyupc",
    ];
    if SMALL.contains(&family) { 0.72 } else { 1.0 }
}

impl FontBook {
    #[must_use]
    pub fn new(provider: Option<Arc<dyn FontProvider>>) -> Self {
        Self {
            has_provider: provider.is_some(),
            canvas: RefCell::new(Canvas::new()),
            families: RefCell::new(Families::new(provider)),
            faces: RefCell::default(),
            by_font: RefCell::default(),
            shaped: RefCell::default(),
            shaped_rtl: RefCell::default(),
            picks: RefCell::default(),
            lists: RefCell::default(),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        !self.has_provider
    }

    #[must_use]
    pub fn faces(&self) -> Vec<Face> {
        self.faces.borrow().clone()
    }

    #[must_use]
    pub fn face(&self, id: FaceId) -> Face {
        self.faces.borrow()[id].clone()
    }

    #[must_use]
    pub fn missing(&self) -> usize {
        self.families.borrow().missing()
    }

    fn face_of(&self, font: FontId) -> FaceId {
        if let Some(found) = self.by_font.borrow().get(&font) {
            return *found;
        }
        let m = self.canvas.borrow().font_metrics(font);
        let sub = m.subfamily.to_ascii_lowercase();
        let face = Face {
            family: m.family.clone(),
            bold: m.bold || sub.contains("bold"),
            italic: m.italic || sub.contains("italic") || sub.contains("oblique"),
            id: font,
            metrics: metrics_of(&m),
        };
        let mut faces = self.faces.borrow_mut();
        faces.push(face);
        let id = faces.len() - 1;
        self.by_font.borrow_mut().insert(font, id);
        id
    }

    fn listed(&self, list: &str, bold: bool, italic: bool) -> Rc<Listed> {
        let key = (list.to_owned(), bold, italic);
        if let Some(found) = self.lists.borrow().get(&key) {
            return Rc::clone(found);
        }
        let names: Vec<&str> = list
            .split(',')
            .map(|f| f.trim().trim_matches(['"', '\'']).trim())
            .filter(|f| !f.is_empty())
            .map(|f| match normal(f).as_str() {
                "serif" | "ui serif" => "Liberation Serif",
                "sans serif" | "system ui" | "ui sans serif" | "apple system"
                | "blinkmacsystemfont" => "Liberation Sans",
                "monospace" | "ui monospace" => "Liberation Mono",
                _ => f,
            })
            .collect();
        let mut present = Vec::new();
        if names.len() > 1 {
            let mut canvas = self.canvas.borrow_mut();
            let mut families = self.families.borrow_mut();
            for name in &names {
                let wanted = normalize(name);
                if let Some(pick) = families.face(&mut canvas, name, bold, italic)
                    && let m = canvas.font_metrics(pick.font)
                    && normalize(&m.family) == wanted
                    && (wanted.contains("condensed")
                        || !m.subfamily.to_ascii_lowercase().contains("condensed"))
                {
                    present.push(((*name).to_owned(), pick));
                }
            }
        }
        let primary = present
            .first()
            .map(|(name, _)| name.clone())
            .or_else(|| names.first().map(|name| (*name).to_owned()))
            .unwrap_or_else(|| "Liberation Serif".to_owned());
        let listed = Rc::new(Listed { present, primary });
        self.lists.borrow_mut().insert(key, Rc::clone(&listed));
        listed
    }

    #[must_use]
    pub fn pick(&self, style: &TextStyle, cluster: &str) -> Option<Pick> {
        let first = cluster.chars().next()?;
        let class = class_of(first);
        let family = if is_complex(first) {
            style.family_complex.as_deref().unwrap_or(&style.family)
        } else {
            &style.family
        };
        let key = (
            family.to_owned(),
            style.bold,
            style.italic,
            cluster.to_owned(),
        );
        if let Some(found) = self.picks.borrow().get(&key) {
            return *found;
        }
        let listed = self.listed(family, style.bold, style.italic);
        let chosen = {
            let mut canvas = self.canvas.borrow_mut();
            let neutral = cluster.chars().all(|c| is_space(c) || is_invisible(c));
            let in_list = listed.present.iter().find(|(_, pick)| {
                neutral
                    || cluster
                        .chars()
                        .filter(|c| !is_space(*c) && !is_invisible(*c))
                        .all(|c| canvas.has_char(pick.font, c))
            });
            match in_list {
                Some((_, pick)) => Some(*pick),
                None => self
                    .families
                    .borrow_mut()
                    .runs(
                        &mut canvas,
                        cluster,
                        &listed.primary,
                        style.bold,
                        style.italic,
                    )
                    .first()
                    .map(|(_, pick)| *pick),
            }
        };
        let pick = chosen.map(|p| {
            let face = self.face_of(p.font);
            let face_family = normal(&self.faces.borrow()[face].family);
            let asked = normal(&listed.primary);
            let scale = if class == Class::Thai && face_family != asked {
                thai_scale(&asked)
            } else {
                1.0
            };
            Pick {
                face,
                scale,
                fake_bold: p.fake_bold,
                fake_italic: p.fake_italic,
            }
        });
        self.picks.borrow_mut().insert(key, pick);
        pick
    }

    #[must_use]
    pub fn shape(&self, face: FaceId, cluster: &str) -> Rc<ShapedCluster> {
        let key = (face, cluster.to_owned());
        if let Some(found) = self.shaped.borrow().get(&key) {
            return Rc::clone(found);
        }
        let font = self.faces.borrow()[face].id;
        let shaped = self.canvas.borrow().shape(font, cluster);
        let one = if shaped.clusters.len() == 1 {
            shaped
                .clusters
                .into_iter()
                .next()
                .unwrap_or_else(|| empty(cluster))
        } else {
            let mut all = empty(cluster);
            let mut pen = 0;
            for c in shaped.clusters {
                for mut g in c.glyphs {
                    g.x += pen;
                    all.glyphs.push(g);
                }
                pen += c.advance;
                all.missing |= c.missing;
                all.space &= c.space;
            }
            all.advance = pen;
            all
        };
        let one = Rc::new(one);
        self.shaped.borrow_mut().insert(key, Rc::clone(&one));
        one
    }
}

impl FontBook {
    #[must_use]
    pub fn shape_rtl(&self, face: FaceId, word: &str) -> Rc<Vec<Part>> {
        let key = (face, word.to_owned());
        if let Some(found) = self.shaped_rtl.borrow().get(&key) {
            return Rc::clone(found);
        }
        let font = self.faces.borrow()[face].id;
        let shaped = self.canvas.borrow().shape_rtl(font, word);
        let parts: Vec<Part> = shaped
            .clusters
            .into_iter()
            .map(|c| {
                let text = c
                    .glyphs
                    .iter()
                    .map(|g| g.meaning.as_str())
                    .collect::<String>();
                (text, Rc::new(c))
            })
            .collect();
        let parts = Rc::new(parts);
        self.shaped_rtl.borrow_mut().insert(key, Rc::clone(&parts));
        parts
    }
}

fn empty(text: &str) -> ShapedCluster {
    ShapedCluster {
        range: 0..text.len(),
        glyphs: Vec::new(),
        advance: 0,
        missing: false,
        space: !text.is_empty() && text.chars().all(char::is_whitespace),
    }
}

pub use convert_pdf_canvas::font::{is_invisible, is_space};
pub use convert_pdf_canvas::graphemes;

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> FontBook {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../fonts/packaged");
        let provider = pdf_font::system_fonts::SystemFontProvider::discover_in(&[dir.into()]);
        FontBook::new(Some(Arc::new(provider)))
    }

    fn style(family: &str, bold: bool, italic: bool) -> TextStyle {
        TextStyle {
            family: family.into(),
            bold,
            italic,
            ..TextStyle::default()
        }
    }

    #[test]
    fn styles_pick_their_own_faces() {
        let book = package();
        for (family, bold, italic) in [
            ("Times New Roman", false, true),
            ("Arial", true, true),
            ("Calibri", true, false),
        ] {
            let pick = book
                .pick(&style(family, bold, italic), "a")
                .expect("a face");
            let face = book.face(pick.face);
            assert_eq!(
                (face.bold, face.italic),
                (bold, italic),
                "{family}: {face:?}"
            );
            assert!(!pick.fake_bold && !pick.fake_italic);
        }
    }

    #[test]
    fn lao_gets_a_lao_face_with_room_for_its_marks() {
        let book = package();
        let pick = book
            .pick(&style("Phetsarath OT", false, false), "ກ")
            .expect("a Lao face");
        let face = book.face(pick.face);
        assert!(
            face.family.contains("Lao")
                || face.family.contains("Saysettha")
                || face.family.contains("Phetsarath"),
            "{face:?}"
        );
        if face.family.contains("Saysettha") {
            assert!(face.metrics.line() > 1.7, "{:?}", face.metrics);
        }
    }

    #[test]
    fn a_css_list_takes_the_first_family_there_is() {
        let book = package();
        let pick = book
            .pick(
                &style("\"No Such Face\", 'Liberation Mono', serif", false, false),
                "a",
            )
            .expect("a face");
        assert_eq!(book.face(pick.face).family, "Liberation Mono");
    }
}
