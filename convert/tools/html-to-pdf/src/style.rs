use convert_layout::model::{Align, Border, Direction, LineHeight, Rgb, Sides, VerticalShift};

use crate::css::Declaration;

pub const PX: f32 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    ListItem,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
    TableCaption,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Pt(f32),
    Percent(f32),
    Auto,
}

impl Length {
    #[must_use]
    pub fn resolve(self, of: f32) -> Option<f32> {
        match self {
            Self::Pt(v) => Some(v),
            Self::Percent(p) => Some(of * p / 100.0),
            Self::Auto => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub display: Display,
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub colour: Rgb,
    pub align: Align,
    pub align_physical: bool,
    pub direction: Direction,
    pub line_height: Option<LineHeight>,
    pub pre: bool,
    pub nowrap: bool,
    pub list_style: String,
    pub indent: f32,
    pub upper: bool,
    pub letter_spacing: f32,
    pub underline: bool,
    pub strike: bool,
    pub lang: Option<String>,
    pub border_collapse: bool,
    pub shift: VerticalShift,
    pub background: Option<Rgb>,
    pub margin: Sides<Length>,
    pub padding: Sides<f32>,
    pub border: Sides<Option<Border>>,
    pub width: Length,
    pub height: Length,
    pub max_width: Length,
    pub break_before: bool,
    pub break_after: bool,
    pub valign: Option<convert_layout::model::VAlign>,
    pub float_or_absolute: bool,
    pub page: Option<String>,
    pub columns: usize,
    pub column_gap: f32,
}

impl Computed {
    #[must_use]
    pub fn root() -> Self {
        Self {
            display: Display::Block,
            family: "serif".into(),
            size: 12.0,
            bold: false,
            italic: false,
            colour: [0, 0, 0],
            align: Align::Left,
            align_physical: false,
            direction: Direction::Ltr,
            line_height: None,
            pre: false,
            nowrap: false,
            list_style: "disc".into(),
            indent: 0.0,
            upper: false,
            letter_spacing: 0.0,
            underline: false,
            strike: false,
            lang: None,
            border_collapse: false,
            shift: VerticalShift::None,
            background: None,
            margin: Sides::all(Length::Pt(0.0)),
            padding: Sides::all(0.0),
            border: Sides::default(),
            width: Length::Auto,
            height: Length::Auto,
            max_width: Length::Auto,
            break_before: false,
            break_after: false,
            valign: None,
            float_or_absolute: false,
            page: None,
            columns: 1,
            column_gap: 12.0,
        }
    }

    #[must_use]
    pub fn paragraph_align(&self, rtl: bool) -> Align {
        match (self.align, rtl && self.align_physical) {
            (Align::Left, true) => Align::Right,
            (Align::Right, true) => Align::Left,
            (align, _) => align,
        }
    }

    #[must_use]
    pub fn inherit(&self) -> Self {
        Self {
            display: Display::Inline,
            shift: VerticalShift::None,
            background: None,
            margin: Sides::all(Length::Pt(0.0)),
            padding: Sides::all(0.0),
            border: Sides::default(),
            width: Length::Auto,
            height: Length::Auto,
            max_width: Length::Auto,
            break_before: false,
            break_after: false,
            valign: None,
            float_or_absolute: false,
            page: None,
            columns: 1,
            column_gap: 12.0,
            ..self.clone()
        }
    }

    pub fn apply(&mut self, declarations: &[Declaration], parent_size: f32) {
        for d in declarations {
            if d.property == "font-size" {
                if let Some(size) = font_size(&d.value, parent_size) {
                    self.size = size;
                }
            } else if d.property == "font"
                && let Some(size) = d
                    .value
                    .split_whitespace()
                    .find_map(|w| font_size(w.split('/').next().unwrap_or(w), parent_size))
            {
                self.size = size;
            }
        }
        for d in declarations {
            self.apply_one(&d.property, d.value.trim());
        }
    }

    #[allow(clippy::too_many_lines)]
    fn apply_one(&mut self, property: &str, value: &str) {
        let lower = value.to_ascii_lowercase();
        let em = self.size;
        let length = |text: &str| parse_length(text, em);
        match property {
            "display" => {
                self.display = match lower.as_str() {
                    "none" => Display::None,
                    "inline" | "inline-block" | "inline-flex" => Display::Inline,
                    "list-item" => Display::ListItem,
                    "table" | "inline-table" => Display::Table,
                    "table-row-group" | "table-header-group" | "table-footer-group" => {
                        Display::TableRowGroup
                    }
                    "table-row" => Display::TableRow,
                    "table-cell" => Display::TableCell,
                    "table-caption" => Display::TableCaption,
                    _ => Display::Block,
                };
            }
            "font-family" => {
                self.family = value.to_owned();
            }
            "font-weight" => {
                self.bold = match lower.as_str() {
                    "bold" | "bolder" => true,
                    "normal" | "lighter" => false,
                    number => number.parse::<u32>().is_ok_and(|w| w >= 600),
                };
            }
            "font-style" => self.italic = lower.contains("italic") || lower.contains("oblique"),
            "font" => {
                for word in lower.split_whitespace() {
                    match word {
                        "bold" | "bolder" | "600" | "700" | "800" | "900" => self.bold = true,
                        "italic" | "oblique" => self.italic = true,
                        _ => {}
                    }
                }
                if let Some(position) = value.find(|c: char| c.is_ascii_digit())
                    && let Some(space) = value[position..].find(' ')
                {
                    let family = value[position + space..].trim();
                    if !family.is_empty() {
                        self.family = family.to_owned();
                    }
                }
            }
            "color" => {
                if let Some(c) = parse_colour(&lower) {
                    self.colour = c;
                }
            }
            "background-color" | "background" => {
                self.background = lower
                    .split_whitespace()
                    .find_map(parse_colour)
                    .or_else(|| parse_colour(&lower));
                if lower.contains("transparent") || lower == "none" {
                    self.background = None;
                }
            }
            "text-align" => {
                self.align = match lower.as_str() {
                    "center" | "-webkit-center" => Align::Center,
                    "right" | "end" | "-webkit-right" => Align::Right,
                    "justify" => Align::Justify,
                    _ => Align::Left,
                };
                self.align_physical = matches!(
                    lower.as_str(),
                    "left" | "right" | "-webkit-left" | "-webkit-right"
                );
            }
            "direction" => match lower.as_str() {
                "rtl" => self.direction = Direction::Rtl,
                "ltr" => self.direction = Direction::Ltr,
                _ => {}
            },
            "line-height" => {
                self.line_height = if lower == "normal" {
                    None
                } else if let Ok(factor) = lower.parse::<f32>() {
                    Some(LineHeight::OfSize(factor))
                } else if let Some(pct) = lower.strip_suffix('%') {
                    pct.trim()
                        .parse::<f32>()
                        .ok()
                        .map(|p| LineHeight::Exact(p / 100.0 * em))
                } else {
                    length(&lower).map(LineHeight::Exact)
                };
            }
            "white-space" => {
                self.pre = lower.starts_with("pre");
                self.nowrap = lower == "nowrap";
            }
            "list-style-type" | "list-style" => {
                if let Some(word) = lower
                    .split_whitespace()
                    .find(|w| !matches!(*w, "inside" | "outside") && !w.starts_with("url"))
                {
                    self.list_style = word.to_owned();
                }
            }
            "text-indent" => {
                if let Some(v) = length(&lower) {
                    self.indent = v;
                }
            }
            "text-transform" => self.upper = lower == "uppercase",
            "letter-spacing" => self.letter_spacing = length(&lower).unwrap_or(0.0),
            "text-decoration" | "text-decoration-line" => {
                if lower.contains("none") {
                    self.underline = false;
                    self.strike = false;
                }
                if lower.contains("underline") {
                    self.underline = true;
                }
                if lower.contains("line-through") {
                    self.strike = true;
                }
            }
            "vertical-align" => {
                match lower.as_str() {
                    "super" | "top" | "text-top" => self.shift = VerticalShift::Super,
                    "sub" | "bottom" | "text-bottom" => self.shift = VerticalShift::Sub,
                    _ => {}
                }
                self.valign = match lower.as_str() {
                    "middle" => Some(convert_layout::model::VAlign::Middle),
                    "bottom" => Some(convert_layout::model::VAlign::Bottom),
                    "top" => Some(convert_layout::model::VAlign::Top),
                    _ => self.valign,
                };
            }
            "margin" => {
                let sides = four(&lower, |t| parse_length_or_auto(t, em));
                if let Some(sides) = sides {
                    self.margin = sides;
                }
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                if let Some(v) = parse_length_or_auto(&lower, em) {
                    *side_mut(&mut self.margin, property) = v;
                }
            }
            "padding" => {
                if let Some(sides) = four(&lower, |t| length(t)) {
                    self.padding = sides;
                }
            }
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                if let Some(v) = length(&lower) {
                    *side_mut(&mut self.padding, property) = v;
                }
            }
            "border" => {
                let b = parse_border(&lower, em);
                self.border = Sides::all(b);
            }
            "border-top" | "border-right" | "border-bottom" | "border-left" => {
                *side_mut(&mut self.border, property) = parse_border(&lower, em);
            }
            "border-width" => {
                if let Some(widths) = four(&lower, |t| length(t)) {
                    for (side, width) in [
                        (&mut self.border.top, widths.top),
                        (&mut self.border.right, widths.right),
                        (&mut self.border.bottom, widths.bottom),
                        (&mut self.border.left, widths.left),
                    ] {
                        if let Some(border) = side {
                            border.width = width;
                        }
                    }
                }
            }
            "border-color" => {
                if let Some(colour) = parse_colour(&lower) {
                    for side in [
                        &mut self.border.top,
                        &mut self.border.right,
                        &mut self.border.bottom,
                        &mut self.border.left,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        side.colour = colour;
                    }
                }
            }
            "border-style" => {
                if lower == "none" || lower == "hidden" {
                    self.border = Sides::default();
                } else if self.border.top.is_none() {
                    self.border = Sides::all(Some(Border {
                        width: 2.25,
                        colour: self.colour,
                    }));
                }
            }
            "border-collapse" => self.border_collapse = lower == "collapse",
            "width" => self.width = parse_length_or_auto(&lower, em).unwrap_or(Length::Auto),
            "height" => self.height = parse_length_or_auto(&lower, em).unwrap_or(Length::Auto),
            "max-width" => {
                self.max_width = parse_length_or_auto(&lower, em).unwrap_or(Length::Auto);
            }
            "page-break-before" | "break-before" => {
                self.break_before = matches!(lower.as_str(), "always" | "page" | "left" | "right");
            }
            "page-break-after" | "break-after" => {
                self.break_after = matches!(lower.as_str(), "always" | "page" | "left" | "right");
            }
            "column-count" | "columns" => {
                if let Some(n) = lower
                    .split_whitespace()
                    .find_map(|w| w.parse::<usize>().ok())
                {
                    self.columns = n.max(1);
                }
            }
            "column-gap" => {
                if let Some(v) = length(&lower) {
                    self.column_gap = v;
                }
            }
            "page" => self.page = (lower != "auto").then(|| lower.clone()),
            "float" => self.float_or_absolute = lower != "none",
            "position" => self.float_or_absolute = lower == "absolute" || lower == "fixed",
            _ => {}
        }
    }
}

fn side_mut<'a, T>(sides: &'a mut Sides<T>, property: &str) -> &'a mut T {
    if property.ends_with("top") {
        &mut sides.top
    } else if property.ends_with("right") {
        &mut sides.right
    } else if property.ends_with("bottom") {
        &mut sides.bottom
    } else {
        &mut sides.left
    }
}

fn four<T: Copy>(value: &str, parse: impl Fn(&str) -> Option<T>) -> Option<Sides<T>> {
    let parts: Vec<T> = value
        .split_whitespace()
        .map(&parse)
        .collect::<Option<_>>()?;
    Some(match parts.as_slice() {
        [a] => Sides::all(*a),
        [a, b] => Sides {
            top: *a,
            right: *b,
            bottom: *a,
            left: *b,
        },
        [a, b, c] => Sides {
            top: *a,
            right: *b,
            bottom: *c,
            left: *b,
        },
        [a, b, c, d, ..] => Sides {
            top: *a,
            right: *b,
            bottom: *c,
            left: *d,
        },
        [] => return None,
    })
}

fn parse_border(value: &str, em: f32) -> Option<Border> {
    if value.contains("none") || value.contains("hidden") || value == "0" {
        return None;
    }
    let mut width = 2.25;
    let mut colour = [0, 0, 0];
    for word in value.split_whitespace() {
        if let Some(w) = parse_length(word, em) {
            width = w;
        } else if let Some(c) = parse_colour(word) {
            colour = c;
        } else if word == "thin" {
            width = 0.75;
        } else if word == "thick" {
            width = 3.75;
        }
    }
    (width > 0.0).then_some(Border { width, colour })
}

#[must_use]
pub fn parse_length(text: &str, em: f32) -> Option<f32> {
    let text = text.trim();
    if text == "0" {
        return Some(0.0);
    }
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(text.len());
    let number: f32 = text[..split].parse().ok()?;
    let unit = &text[split..];
    Some(match unit {
        "px" | "" => number * PX,
        "pt" => number,
        "pc" => number * 12.0,
        "in" => number * 72.0,
        "cm" => number * 72.0 / 2.54,
        "mm" => number * 72.0 / 25.4,
        "em" => number * em,
        "rem" => number * 12.0,
        "ex" | "ch" => number * em * 0.5,
        _ => return None,
    })
}

fn parse_length_or_auto(text: &str, em: f32) -> Option<Length> {
    let text = text.trim();
    if text == "auto" {
        return Some(Length::Auto);
    }
    if let Some(p) = text.strip_suffix('%') {
        return p.trim().parse().ok().map(Length::Percent);
    }
    parse_length(text, em).map(Length::Pt)
}

fn font_size(value: &str, parent: f32) -> Option<f32> {
    let lower = value.trim().to_ascii_lowercase();
    Some(match lower.as_str() {
        "xx-small" => 7.0,
        "x-small" => 7.5,
        "small" => 9.75,
        "medium" => 12.0,
        "large" => 13.5,
        "x-large" => 18.0,
        "xx-large" => 24.0,
        "smaller" => parent / 1.2,
        "larger" => parent * 1.2,
        _ => {
            if let Some(p) = lower.strip_suffix('%') {
                parent * p.trim().parse::<f32>().ok()? / 100.0
            } else {
                parse_length(&lower, parent)?
            }
        }
    })
}

#[must_use]
pub fn parse_colour(text: &str) -> Option<Rgb> {
    let text = text.trim().to_ascii_lowercase();
    if let Some(hex) = text.strip_prefix('#') {
        let digits: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).and_then(|d| u8::try_from(d).ok()))
            .collect::<Option<_>>()?;
        return match digits.len() {
            3 | 4 => Some([digits[0] * 17, digits[1] * 17, digits[2] * 17]),
            6 | 8 => Some([
                digits[0] * 16 + digits[1],
                digits[2] * 16 + digits[3],
                digits[4] * 16 + digits[5],
            ]),
            _ => None,
        };
    }
    if let Some(inner) = text
        .strip_prefix("rgba(")
        .or_else(|| text.strip_prefix("rgb("))
        .and_then(|t| t.strip_suffix(')'))
    {
        let parts: Vec<&str> = inner
            .split([',', ' ', '/'])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() < 3 {
            return None;
        }
        if parts.len() >= 4 && parts[3].trim().parse::<f32>().is_ok_and(|a| a == 0.0) {
            return None;
        }
        let channel = |p: &str| -> Option<u8> {
            let p = p.trim();
            let v = if let Some(pct) = p.strip_suffix('%') {
                pct.parse::<f32>().ok()? * 2.55
            } else {
                p.parse::<f32>().ok()?
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            Some(v.round().clamp(0.0, 255.0) as u8)
        };
        return Some([channel(parts[0])?, channel(parts[1])?, channel(parts[2])?]);
    }
    let named: Rgb = match text.as_str() {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "lime" => [0, 255, 0],
        "blue" => [0, 0, 255],
        "navy" => [0, 0, 128],
        "yellow" => [255, 255, 0],
        "orange" => [255, 165, 0],
        "purple" => [128, 0, 128],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        "lightgray" | "lightgrey" => [211, 211, 211],
        "darkgray" | "darkgrey" => [169, 169, 169],
        "gainsboro" => [220, 220, 220],
        "whitesmoke" => [245, 245, 245],
        "maroon" => [128, 0, 0],
        "olive" => [128, 128, 0],
        "teal" => [0, 128, 128],
        "aqua" | "cyan" => [0, 255, 255],
        "fuchsia" | "magenta" => [255, 0, 255],
        "brown" => [165, 42, 42],
        "pink" => [255, 192, 203],
        "gold" => [255, 215, 0],
        "darkblue" => [0, 0, 139],
        "darkred" => [139, 0, 0],
        "darkgreen" => [0, 100, 0],
        "lightblue" => [173, 216, 230],
        "lightgreen" => [144, 238, 144],
        "lightyellow" => [255, 255, 224],
        "beige" => [245, 245, 220],
        "ivory" => [255, 255, 240],
        "steelblue" => [70, 130, 180],
        "royalblue" => [65, 105, 225],
        "crimson" => [220, 20, 60],
        "tomato" => [255, 99, 71],
        "coral" => [255, 127, 80],
        "salmon" => [250, 128, 114],
        "khaki" => [240, 230, 140],
        "indigo" => [75, 0, 130],
        "violet" => [238, 130, 238],
        "tan" => [210, 180, 140],
        "skyblue" => [135, 206, 235],
        "dimgray" | "dimgrey" => [105, 105, 105],
        "slategray" | "slategrey" => [112, 128, 144],
        "aliceblue" => [240, 248, 255],
        "lavender" => [230, 230, 250],
        "honeydew" => [240, 255, 240],
        "mintcream" => [245, 255, 250],
        "seashell" => [255, 245, 238],
        "linen" => [250, 240, 230],
        "wheat" => [245, 222, 179],
        "chocolate" => [210, 105, 30],
        "firebrick" => [178, 34, 34],
        "forestgreen" => [34, 139, 34],
        "seagreen" => [46, 139, 87],
        "darkorange" => [255, 140, 0],
        "dodgerblue" => [30, 144, 255],
        "midnightblue" => [25, 25, 112],
        "darkslategray" | "darkslategrey" => [47, 79, 79],
        _ => return None,
    };
    Some(named)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_lengths() {
        assert_eq!(parse_colour("#0a0"), Some([0, 170, 0]));
        assert_eq!(parse_colour("rgb(10, 20, 30)"), Some([10, 20, 30]));
        assert_eq!(parse_colour("Navy"), Some([0, 0, 128]));
        assert_eq!(parse_length("16px", 12.0), Some(12.0));
        assert_eq!(parse_length("2em", 10.0), Some(20.0));
        assert_eq!(parse_length("1in", 10.0), Some(72.0));
    }
}
