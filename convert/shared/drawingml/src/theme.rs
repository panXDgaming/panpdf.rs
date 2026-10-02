use std::collections::HashMap;

use convert_office_read::Element;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    #[must_use]
    pub fn rgb(v: u32) -> Self {
        Self {
            r: ((v >> 16) & 255) as f32 / 255.0,
            g: ((v >> 8) & 255) as f32 / 255.0,
            b: (v & 255) as f32 / 255.0,
            a: 1.0,
        }
    }

    #[must_use]
    pub fn pdf(self) -> convert_pdf_canvas::Rgb {
        convert_pdf_canvas::Rgb(self.r, self.g, self.b)
    }
}

#[derive(Clone, Debug, Default)]
pub struct FontSlots {
    pub latin: String,
    pub cs: String,
    pub ea: String,
}

#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub colors: HashMap<String, u32>,
    pub major: FontSlots,
    pub minor: FontSlots,
    pub fills: Vec<Element>,
    pub lines: Vec<Element>,
    pub bg_fills: Vec<Element>,
}

impl Theme {
    #[must_use]
    pub fn read(root: Option<&Element>) -> Self {
        let mut theme = Self::default();
        for (name, value) in [
            ("dk1", 0x000000),
            ("lt1", 0xFFFFFF),
            ("dk2", 0x44546A),
            ("lt2", 0xE7E6E6),
            ("accent1", 0x4472C4),
            ("accent2", 0xED7D31),
            ("accent3", 0xA5A5A5),
            ("accent4", 0xFFC000),
            ("accent5", 0x5B9BD5),
            ("accent6", 0x70AD47),
            ("hlink", 0x0563C1),
            ("folHlink", 0x954F72),
        ] {
            theme.colors.insert(name.to_owned(), value);
        }
        theme.minor.latin = "Calibri".to_owned();
        theme.major.latin = "Calibri Light".to_owned();
        let Some(root) = root else { return theme };
        if let Some(scheme) = root.descendants("clrScheme").first() {
            for entry in scheme.elements() {
                if let Some(color) = entry.elements().next() {
                    let value = match color.local() {
                        "srgbClr" => color.attr("val"),
                        "sysClr" => color.attr("lastClr"),
                        _ => None,
                    }
                    .and_then(|v| u32::from_str_radix(v, 16).ok());
                    if let Some(value) = value {
                        theme.colors.insert(entry.local().to_owned(), value);
                    }
                }
            }
        }
        if let Some(fonts) = root.descendants("fontScheme").first() {
            let slots = |name: &str| {
                let e = fonts.child(name);
                let face = |slot: &str| {
                    e.and_then(|e| e.child(slot))
                        .and_then(|s| s.attr("typeface"))
                        .unwrap_or("")
                        .to_owned()
                };
                FontSlots {
                    latin: face("latin"),
                    cs: face("cs"),
                    ea: face("ea"),
                }
            };
            theme.major = slots("majorFont");
            theme.minor = slots("minorFont");
        }
        if let Some(format) = root.descendants("fmtScheme").first() {
            let list = |name: &str| {
                format
                    .child(name)
                    .map(|l| l.elements().cloned().collect())
                    .unwrap_or_default()
            };
            theme.fills = list("fillStyleLst");
            theme.lines = list("lnStyleLst");
            theme.bg_fills = list("bgFillStyleLst");
        }
        theme
    }

    #[must_use]
    pub fn font(&self, name: &str) -> String {
        let slots = match name.get(..4) {
            Some("+mj-") => &self.major,
            Some("+mn-") => &self.minor,
            _ => return name.to_owned(),
        };
        match &name[4..] {
            "cs" => slots.cs.clone(),
            "ea" => slots.ea.clone(),
            _ => slots.latin.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Palette<'a> {
    pub theme: &'a Theme,
    pub map: &'a HashMap<String, String>,
    pub placeholder: Option<Color>,
}

fn to_hsl(c: Color) -> (f32, f32, f32) {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - c.r).abs() < 1e-6 {
        (c.g - c.b) / d + if c.g < c.b { 6.0 } else { 0.0 }
    } else if (max - c.g).abs() < 1e-6 {
        (c.b - c.r) / d + 2.0
    } else {
        (c.r - c.g) / d + 4.0
    };
    (h / 6.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32, a: f32) -> Color {
    if s == 0.0 {
        return Color {
            r: l,
            g: l,
            b: l,
            a,
        };
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let hue = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Color {
        r: hue(h + 1.0 / 3.0),
        g: hue(h),
        b: hue(h - 1.0 / 3.0),
        a,
    }
}

fn preset(name: &str) -> Option<u32> {
    Some(match name {
        "black" => 0x000000,
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "green" => 0x008000,
        "blue" => 0x0000FF,
        "yellow" => 0xFFFF00,
        "gray" | "grey" => 0x808080,
        "darkGray" => 0xA9A9A9,
        "lightGray" => 0xD3D3D3,
        "orange" => 0xFFA500,
        "purple" => 0x800080,
        "navy" => 0x000080,
        _ => return None,
    })
}

impl Palette<'_> {
    #[must_use]
    pub fn color(&self, element: &Element) -> Option<Color> {
        let base = match element.local() {
            "srgbClr" => Color::rgb(u32::from_str_radix(element.attr("val")?, 16).ok()?),
            "sysClr" => Color::rgb(
                element
                    .attr("lastClr")
                    .and_then(|v| u32::from_str_radix(v, 16).ok())
                    .unwrap_or(0),
            ),
            "prstClr" => Color::rgb(preset(element.attr("val")?)?),
            "scrgbClr" => {
                let p = |k: &str| {
                    element
                        .attr(k)
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.0)
                        / 100_000.0
                };
                Color {
                    r: p("r"),
                    g: p("g"),
                    b: p("b"),
                    a: 1.0,
                }
            }
            "schemeClr" => {
                let name = element.attr("val")?;
                if name == "phClr" {
                    self.placeholder.unwrap_or(Color::rgb(0))
                } else {
                    let mapped = self.map.get(name).map_or(name, String::as_str);
                    let key = match mapped {
                        "bg1" => "lt1",
                        "tx1" => "dk1",
                        "bg2" => "lt2",
                        "tx2" => "dk2",
                        other => other,
                    };
                    Color::rgb(*self.theme.colors.get(key)?)
                }
            }
            _ => return None,
        };
        Some(modify(base, element))
    }

    #[must_use]
    pub fn first_color(&self, parent: &Element) -> Option<Color> {
        parent.elements().find_map(|e| self.color(e))
    }
}

fn modify(mut c: Color, element: &Element) -> Color {
    for m in element.elements() {
        let v = m
            .attr("val")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(100_000.0)
            / 100_000.0;
        match m.local() {
            "alpha" => c.a = v,
            "lumMod" => {
                let (h, s, l) = to_hsl(c);
                c = from_hsl(h, s, (l * v).clamp(0.0, 1.0), c.a);
            }
            "lumOff" => {
                let (h, s, l) = to_hsl(c);
                c = from_hsl(h, s, (l + v).clamp(0.0, 1.0), c.a);
            }
            "tint" => {
                c.r += (1.0 - c.r) * (1.0 - v);
                c.g += (1.0 - c.g) * (1.0 - v);
                c.b += (1.0 - c.b) * (1.0 - v);
            }
            "shade" => {
                c.r *= v;
                c.g *= v;
                c.b *= v;
            }
            "satMod" => {
                let (h, s, l) = to_hsl(c);
                c = from_hsl(h, (s * v).clamp(0.0, 1.0), l, c.a);
            }
            _ => {}
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers() {
        let theme = Theme::read(None);
        let map = HashMap::from([("tx1".to_owned(), "dk1".to_owned())]);
        let palette = Palette {
            theme: &theme,
            map: &map,
            placeholder: None,
        };
        let e = convert_office_read::xml::parse(r#"<a:schemeClr val="tx1"><a:lumMod val="50000"/><a:lumOff val="50000"/></a:schemeClr>"#).unwrap();
        let c = palette.color(&e).unwrap();
        assert!((c.r - 0.5).abs() < 0.01);
        let e = convert_office_read::xml::parse(
            r#"<a:srgbClr val="FF0000"><a:alpha val="40000"/></a:srgbClr>"#,
        )
        .unwrap();
        assert!((palette.color(&e).unwrap().a - 0.4).abs() < 1e-4);
        assert_eq!(theme.font("+mn-lt"), "Calibri");
    }
}
