use crate::values::{Colour, Length, Matrix, length, lengths, transform};

#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
    None,
    Colour(Colour),
    Current,
    Url(String, Option<Box<Paint>>),
}

pub const PROPERTIES: &[&str] = &[
    "alignment-baseline",
    "baseline-shift",
    "clip-path",
    "clip-rule",
    "color",
    "direction",
    "display",
    "dominant-baseline",
    "fill",
    "fill-opacity",
    "fill-rule",
    "filter",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "letter-spacing",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "opacity",
    "stop-color",
    "stop-opacity",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "text-anchor",
    "text-decoration",
    "visibility",
    "word-spacing",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub fill: Paint,
    pub fill_opacity: f32,
    pub fill_even_odd: bool,
    pub stroke: Paint,
    pub stroke_opacity: f32,
    pub stroke_width: f32,
    pub cap: u8,
    pub join: u8,
    pub miter: f32,
    pub dash: Option<Vec<f32>>,
    pub dash_offset: f32,
    pub color: Colour,
    pub font_family: String,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub anchor: u8,
    pub visible: bool,
    pub letter_spacing: f32,
    pub word_spacing: f32,
    pub clip_even_odd: bool,
    pub rtl: bool,
    pub baseline: f32,
    pub underline: bool,
    pub strike: bool,
    pub opacity: f32,
    pub display: bool,
    pub clip_path: Option<String>,
    pub mask: bool,
    pub filter: bool,
    pub markers: bool,
    pub stop_colour: Colour,
    pub stop_opacity: f32,
    pub transform: Option<Matrix>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fill: Paint::Colour(Colour::BLACK),
            fill_opacity: 1.0,
            fill_even_odd: false,
            stroke: Paint::None,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            cap: 0,
            join: 0,
            miter: 4.0,
            dash: None,
            dash_offset: 0.0,
            color: Colour::BLACK,
            font_family: "serif".into(),
            font_size: 16.0,
            bold: false,
            italic: false,
            anchor: 0,
            visible: true,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            clip_even_odd: false,
            rtl: false,
            baseline: 0.0,
            underline: false,
            strike: false,
            opacity: 1.0,
            display: true,
            clip_path: None,
            mask: false,
            filter: false,
            markers: false,
            stop_colour: Colour::BLACK,
            stop_opacity: 1.0,
            transform: None,
        }
    }
}

#[must_use]
pub fn url_id(text: &str) -> Option<String> {
    let text = text.trim();
    let inner = text.strip_prefix("url(")?.split(')').next()?;
    let inner = inner.trim().trim_matches(['"', '\'']);
    inner.strip_prefix('#').map(str::to_owned)
}

fn paint(text: &str) -> Option<Paint> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    match lower.as_str() {
        "none" | "context-fill" | "context-stroke" => return Some(Paint::None),
        "currentcolor" => return Some(Paint::Current),
        _ => {}
    }
    if lower.starts_with("url(") {
        let close = text.find(')')?;
        let id = url_id(&text[..=close]).unwrap_or_default();
        let rest = text[close + 1..].trim();
        let fallback = if rest.is_empty() {
            None
        } else {
            paint(rest).map(Box::new)
        };
        return Some(Paint::Url(id, fallback));
    }
    crate::values::colour(text).map(Paint::Colour)
}

fn opacity(text: &str) -> Option<f32> {
    let text = text.trim();
    let v = if let Some(p) = text.strip_suffix('%') {
        p.trim().parse::<f32>().ok()? / 100.0
    } else {
        text.parse::<f32>().ok()?
    };
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

fn font_size(text: &str, parent: f32) -> Option<f32> {
    let lower = text.trim().to_ascii_lowercase();
    let keyword = match lower.as_str() {
        "xx-small" => Some(9.0),
        "x-small" => Some(10.0),
        "small" => Some(13.0),
        "medium" => Some(16.0),
        "large" => Some(18.0),
        "x-large" => Some(24.0),
        "xx-large" => Some(32.0),
        "larger" => Some(parent * 1.2),
        "smaller" => Some(parent / 1.2),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let v = length(&lower)?.resolve(parent, parent);
    (v.is_finite() && v >= 0.0).then_some(v)
}

fn bold(text: &str, parent: bool) -> bool {
    match text.trim().to_ascii_lowercase().as_str() {
        "bold" | "bolder" => true,
        "normal" | "lighter" => false,
        other => other.parse::<u32>().map_or(parent, |w| w >= 600),
    }
}

fn font_shorthand(text: &str, style: &mut Style, parent_size: f32) {
    let mut words = text.split_whitespace().peekable();
    while let Some(word) = words.peek() {
        let lower = word.to_ascii_lowercase();
        match lower.as_str() {
            "italic" | "oblique" => style.italic = true,
            "normal" | "small-caps" => {}
            "bold" | "bolder" | "lighter" => style.bold = bold(&lower, style.bold),
            w if w.parse::<u32>().is_ok() => style.bold = bold(w, style.bold),
            _ => break,
        }
        words.next();
    }
    let Some(size) = words.next() else {
        return;
    };
    let size = size.split('/').next().unwrap_or(size);
    if let Some(v) = font_size(size, parent_size) {
        style.font_size = v;
    }
    let family: Vec<&str> = words.collect();
    if !family.is_empty() {
        style.font_family = family.join(" ");
    }
}

impl Style {
    #[must_use]
    pub fn inherit(&self) -> Self {
        let initial = Self::default();
        Self {
            opacity: initial.opacity,
            display: initial.display,
            clip_path: None,
            mask: false,
            filter: false,
            markers: false,
            stop_colour: initial.stop_colour,
            stop_opacity: initial.stop_opacity,
            transform: None,
            ..self.clone()
        }
    }

    pub fn apply(&mut self, parent: &Self, props: &[(String, String)], viewport: (f32, f32)) {
        for (name, value) in props {
            if name == "font-size"
                && let Some(v) = font_size(value, parent.font_size)
            {
                self.font_size = v;
            }
            if name == "font" {
                font_shorthand(value, self, parent.font_size);
            }
        }
        let em = self.font_size;
        let diagonal = ((viewport.0 * viewport.0 + viewport.1 * viewport.1) / 2.0).sqrt();
        let len = |v: &str| length(v).map(|l| l.resolve(em, diagonal));
        for (name, value) in props {
            let value = value.trim();
            if value.eq_ignore_ascii_case("inherit") {
                continue;
            }
            let lower = value.to_ascii_lowercase();
            match name.as_str() {
                "fill" => {
                    if let Some(p) = paint(value) {
                        self.fill = p;
                    }
                }
                "stroke" => {
                    if let Some(p) = paint(value) {
                        self.stroke = p;
                    }
                }
                "fill-opacity" => self.fill_opacity = opacity(value).unwrap_or(self.fill_opacity),
                "stroke-opacity" => {
                    self.stroke_opacity = opacity(value).unwrap_or(self.stroke_opacity);
                }
                "opacity" => self.opacity = opacity(value).unwrap_or(self.opacity),
                "stop-opacity" => self.stop_opacity = opacity(value).unwrap_or(self.stop_opacity),
                "fill-rule" => self.fill_even_odd = lower == "evenodd",
                "clip-rule" => self.clip_even_odd = lower == "evenodd",
                "stroke-width" => {
                    if let Some(v) = len(value).filter(|v| *v >= 0.0) {
                        self.stroke_width = v;
                    }
                }
                "stroke-linecap" => {
                    self.cap = match lower.as_str() {
                        "round" => 1,
                        "square" => 2,
                        _ => 0,
                    };
                }
                "stroke-linejoin" => {
                    self.join = match lower.as_str() {
                        "round" => 1,
                        "bevel" => 2,
                        _ => 0,
                    };
                }
                "stroke-miterlimit" => {
                    if let Ok(v) = value.parse::<f32>()
                        && v >= 1.0
                    {
                        self.miter = v;
                    }
                }
                "stroke-dasharray" => {
                    self.dash = if lower == "none" {
                        None
                    } else {
                        let v: Vec<f32> = lengths(value)
                            .into_iter()
                            .map(|l| l.resolve(em, diagonal))
                            .collect();
                        if v.is_empty() || v.iter().any(|d| *d < 0.0) || v.iter().all(|d| *d == 0.0)
                        {
                            None
                        } else if v.len() % 2 == 1 {
                            Some([v.clone(), v].concat())
                        } else {
                            Some(v)
                        }
                    };
                }
                "stroke-dashoffset" => self.dash_offset = len(value).unwrap_or(self.dash_offset),
                "color" => {
                    if let Some(c) = crate::values::colour(value) {
                        self.color = c;
                    }
                }
                "stop-color" => {
                    if lower == "currentcolor" {
                        self.stop_colour = self.color;
                    } else if let Some(c) = crate::values::colour(value) {
                        self.stop_colour = c;
                    }
                }
                "font-family" => self.font_family = value.to_owned(),
                "font-weight" => self.bold = bold(value, parent.bold),
                "font-style" => self.italic = matches!(lower.as_str(), "italic" | "oblique"),
                "text-anchor" => {
                    self.anchor = match lower.as_str() {
                        "middle" => 1,
                        "end" => 2,
                        _ => 0,
                    };
                }
                "visibility" => self.visible = lower == "visible",
                "display" => self.display = lower != "none",
                "letter-spacing" => {
                    self.letter_spacing = if lower == "normal" {
                        0.0
                    } else {
                        len(value).unwrap_or(self.letter_spacing)
                    };
                }
                "word-spacing" => {
                    self.word_spacing = if lower == "normal" {
                        0.0
                    } else {
                        len(value).unwrap_or(self.word_spacing)
                    };
                }
                "direction" => self.rtl = lower == "rtl",
                "dominant-baseline" | "alignment-baseline" => {
                    self.baseline = match lower.as_str() {
                        "middle" => 0.3,
                        "central" | "mathematical" => 0.35,
                        "hanging" => 0.75,
                        "text-before-edge" | "text-top" | "before-edge" => 0.9,
                        "text-after-edge" | "text-bottom" | "after-edge" | "ideographic" => -0.2,
                        _ => 0.0,
                    };
                }
                "text-decoration" | "text-decoration-line" => {
                    if lower.contains("none") {
                        self.underline = false;
                        self.strike = false;
                    }
                    self.underline |= lower.contains("underline");
                    self.strike |= lower.contains("line-through");
                }
                "clip-path" => self.clip_path = url_id(value),
                "mask" => self.mask = lower != "none",
                "filter" => self.filter = lower != "none",
                "marker" | "marker-start" | "marker-mid" | "marker-end" => {
                    self.markers = lower != "none";
                }
                "transform" if lower != "none" => self.transform = transform(value),
                _ => {}
            }
        }
    }
}

#[must_use]
pub fn attr_length(value: Option<&str>) -> Option<Length> {
    length(value?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_apply_in_order() {
        let parent = Style::default();
        let mut s = parent.inherit();
        let props: Vec<(String, String)> = [
            ("fill", "red"),
            ("font", "italic bold 20px Noto Sans Lao, sans-serif"),
            ("stroke-width", "0.5em"),
            ("stroke-dasharray", "2 1 3"),
            ("fill", "url(#g) blue"),
        ]
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect();
        s.apply(&parent, &props, (100.0, 100.0));
        assert!(s.italic && s.bold);
        assert_eq!(s.font_size, 20.0);
        assert_eq!(s.font_family, "Noto Sans Lao, sans-serif");
        assert_eq!(s.stroke_width, 10.0);
        assert_eq!(s.dash.as_deref(), Some(&[2.0, 1.0, 3.0, 2.0, 1.0, 3.0][..]));
        assert!(matches!(&s.fill, Paint::Url(id, Some(_)) if id == "g"));
    }
}
