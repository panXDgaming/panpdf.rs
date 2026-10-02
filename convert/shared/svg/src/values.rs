pub type Matrix = [f32; 6];

pub const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

#[must_use]
pub fn then(first: Matrix, second: Matrix) -> Matrix {
    let [a, b, c, d, e, f] = first;
    let [a2, b2, c2, d2, e2, f2] = second;
    [
        a * a2 + b * c2,
        a * b2 + b * d2,
        c * a2 + d * c2,
        c * b2 + d * d2,
        e * a2 + f * c2 + e2,
        e * b2 + f * d2 + f2,
    ]
}

#[must_use]
pub fn apply(m: Matrix, x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

#[must_use]
pub fn usable(m: Matrix) -> bool {
    m.iter().all(|v| v.is_finite()) && (m[0] * m[3] - m[1] * m[2]).abs() > 1e-12
}

#[must_use]
pub fn number_prefix(text: &str) -> Option<(f32, usize)> {
    let b = text.as_bytes();
    let mut i = 0;
    if i < b.len() && matches!(b[i], b'+' | b'-') {
        i += 1;
    }
    let digits_from = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - digits_from;
    if i < b.len() && b[i] == b'.' {
        let dot = i;
        i += 1;
        let from = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - from;
        if i - from == 0 && digits == 0 {
            i = dot;
        }
    }
    if digits == 0 {
        return None;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'+' | b'-') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    let v: f32 = text[..i].parse().ok()?;
    v.is_finite().then_some((v, i))
}

#[must_use]
pub fn numbers(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        match number_prefix(rest) {
            Some((v, used)) => {
                out.push(v);
                rest = &rest[used..];
            }
            None => break,
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    User(f32),
    Em(f32),
    Ex(f32),
    Percent(f32),
}

impl Length {
    #[must_use]
    pub fn resolve(self, em: f32, of: f32) -> f32 {
        match self {
            Self::User(v) => v,
            Self::Em(v) => v * em,
            Self::Ex(v) => v * em * 0.5,
            Self::Percent(v) => v / 100.0 * of,
        }
    }
}

#[must_use]
pub fn length(text: &str) -> Option<Length> {
    let text = text.trim();
    let (v, used) = number_prefix(text)?;
    let unit = text[used..].trim().to_ascii_lowercase();
    Some(match unit.as_str() {
        "" | "px" => Length::User(v),
        "pt" => Length::User(v * 4.0 / 3.0),
        "pc" => Length::User(v * 16.0),
        "mm" => Length::User(v * 96.0 / 25.4),
        "cm" => Length::User(v * 96.0 / 2.54),
        "in" => Length::User(v * 96.0),
        "q" => Length::User(v * 96.0 / 101.6),
        "em" | "rem" => Length::Em(v),
        "ex" => Length::Ex(v),
        "%" => Length::Percent(v),
        _ => return None,
    })
}

#[must_use]
pub fn lengths(text: &str) -> Vec<Length> {
    text.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|p| !p.is_empty())
        .map_while(length)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Colour {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    fn from_hex(v: u32) -> Self {
        Self {
            r: ((v >> 16) & 255) as f32 / 255.0,
            g: ((v >> 8) & 255) as f32 / 255.0,
            b: (v & 255) as f32 / 255.0,
            a: 1.0,
        }
    }

    #[must_use]
    pub fn bytes(self) -> [u8; 3] {
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [q(self.r), q(self.g), q(self.b)]
    }
}

const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

fn component(text: &str, scale: f32) -> Option<f32> {
    let text = text.trim();
    if let Some(p) = text.strip_suffix('%') {
        return Some(p.trim().parse::<f32>().ok()? / 100.0);
    }
    Some(text.parse::<f32>().ok()? / scale)
}

fn hue(text: &str) -> Option<f32> {
    let text = text.trim().to_ascii_lowercase();
    let (v, unit) = if let Some(v) = text.strip_suffix("deg") {
        (v, 1.0)
    } else if let Some(v) = text.strip_suffix("grad") {
        (v, 0.9)
    } else if let Some(v) = text.strip_suffix("rad") {
        (v, 180.0 / std::f32::consts::PI)
    } else if let Some(v) = text.strip_suffix("turn") {
        (v, 360.0)
    } else {
        (text.as_str(), 1.0)
    };
    Some(v.trim().parse::<f32>().ok()? * unit)
}

fn hsl(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(360.0) / 360.0;
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |t: f32| {
        let t = t.rem_euclid(1.0);
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
    (f(h + 1.0 / 3.0), f(h), f(h - 1.0 / 3.0))
}

#[must_use]
pub fn colour(text: &str) -> Option<Colour> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        let digits: Vec<u32> = hex.chars().map(|c| c.to_digit(16)).collect::<Option<_>>()?;
        let (rgb, a) = match digits.len() {
            3 | 4 => (
                digits[..3].iter().fold(0, |acc, d| (acc << 8) | (d * 17)),
                digits.get(3).map_or(1.0, |d| (*d * 17) as f32 / 255.0),
            ),
            6 | 8 => (
                digits[..6].iter().fold(0, |acc, d| (acc << 4) | d),
                if digits.len() == 8 {
                    ((digits[6] << 4) | digits[7]) as f32 / 255.0
                } else {
                    1.0
                },
            ),
            _ => return None,
        };
        let mut c = Colour::from_hex(rgb);
        c.a = a;
        return Some(c);
    }
    if lower == "transparent" {
        return Some(Colour {
            a: 0.0,
            ..Colour::BLACK
        });
    }
    if let Some(open) = lower.find('(') {
        let name = lower[..open].trim();
        let inner = lower[open + 1..].trim_end().trim_end_matches(')');
        let inner = inner.replace('/', " ");
        let parts: Vec<&str> = if inner.contains(',') {
            inner
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .collect()
        } else {
            inner.split_whitespace().collect()
        };
        if parts.len() < 3 {
            return None;
        }
        let alpha = parts
            .get(3)
            .and_then(|a| component(a, 1.0))
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let (r, g, b) = match name {
            "rgb" | "rgba" => (
                component(parts[0], 255.0)?,
                component(parts[1], 255.0)?,
                component(parts[2], 255.0)?,
            ),
            "hsl" | "hsla" => hsl(
                hue(parts[0])?,
                component(parts[1], 100.0)?,
                component(parts[2], 100.0)?,
            ),
            _ => return None,
        };
        return Some(Colour {
            r: r.clamp(0.0, 1.0),
            g: g.clamp(0.0, 1.0),
            b: b.clamp(0.0, 1.0),
            a: alpha,
        });
    }
    NAMED
        .binary_search_by(|(name, _)| name.cmp(&lower.as_str()))
        .ok()
        .map(|i| Colour::from_hex(NAMED[i].1))
}

#[must_use]
pub fn transform(text: &str) -> Option<Matrix> {
    let mut m = IDENTITY;
    let mut rest = text.trim();
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        if rest.is_empty() {
            break;
        }
        let open = rest.find('(')?;
        let close = rest[open..].find(')')? + open;
        let name = rest[..open].trim().to_ascii_lowercase();
        let args_text = &rest[open + 1..close];
        let mut args = Vec::new();
        for part in args_text.split(|c: char| c.is_whitespace() || c == ',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let lower = part.to_ascii_lowercase();
            let v = if let Some(v) = lower.strip_suffix("deg") {
                v.parse::<f32>().ok()?
            } else if let Some(v) = lower.strip_suffix("grad") {
                v.parse::<f32>().ok()? * 0.9
            } else if let Some(v) = lower.strip_suffix("rad") {
                v.parse::<f32>().ok()?.to_degrees()
            } else if let Some(v) = lower.strip_suffix("turn") {
                v.parse::<f32>().ok()? * 360.0
            } else if let Some(v) = lower.strip_suffix("px") {
                v.parse::<f32>().ok()?
            } else {
                let nums = numbers(part);
                if nums.is_empty() {
                    return None;
                }
                args.extend(nums);
                continue;
            };
            args.push(v);
        }
        let a = |i: usize| args.get(i).copied();
        let item: Matrix = match name.as_str() {
            "matrix" if args.len() >= 6 => [args[0], args[1], args[2], args[3], args[4], args[5]],
            "translate" => [1.0, 0.0, 0.0, 1.0, a(0)?, a(1).unwrap_or(0.0)],
            "translatex" => [1.0, 0.0, 0.0, 1.0, a(0)?, 0.0],
            "translatey" => [1.0, 0.0, 0.0, 1.0, 0.0, a(0)?],
            "scale" => {
                let sx = a(0)?;
                [sx, 0.0, 0.0, a(1).unwrap_or(sx), 0.0, 0.0]
            }
            "scalex" => [a(0)?, 0.0, 0.0, 1.0, 0.0, 0.0],
            "scaley" => [1.0, 0.0, 0.0, a(0)?, 0.0, 0.0],
            "rotate" => {
                let (s, c) = a(0)?.to_radians().sin_cos();
                let r = [c, s, -s, c, 0.0, 0.0];
                match (a(1), a(2)) {
                    (Some(cx), Some(cy)) => then(
                        then([1.0, 0.0, 0.0, 1.0, -cx, -cy], r),
                        [1.0, 0.0, 0.0, 1.0, cx, cy],
                    ),
                    _ => r,
                }
            }
            "skewx" => [1.0, 0.0, a(0)?.to_radians().tan(), 1.0, 0.0, 0.0],
            "skewy" => [1.0, a(0)?.to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
            "skew" => [
                1.0,
                a(1).unwrap_or(0.0).to_radians().tan(),
                a(0)?.to_radians().tan(),
                1.0,
                0.0,
                0.0,
            ],
            _ => return None,
        };
        m = then(item, m);
        rest = &rest[close + 1..];
    }
    m.iter().all(|v| v.is_finite()).then_some(m)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aspect {
    pub align: Option<(f32, f32)>,
    pub slice: bool,
}

impl Default for Aspect {
    fn default() -> Self {
        Self {
            align: Some((0.5, 0.5)),
            slice: false,
        }
    }
}

#[must_use]
pub fn aspect(text: Option<&str>) -> Aspect {
    let Some(text) = text else {
        return Aspect::default();
    };
    let mut words = text.split_whitespace().filter(|w| *w != "defer");
    let align = words.next().unwrap_or("xMidYMid").to_ascii_lowercase();
    let slice = words
        .next()
        .is_some_and(|w| w.eq_ignore_ascii_case("slice"));
    if align == "none" {
        return Aspect { align: None, slice };
    }
    let pick = |s: &str| {
        if s.starts_with("min") {
            0.0
        } else if s.starts_with("max") {
            1.0
        } else {
            0.5
        }
    };
    let (x, y) = match (align.find('x'), align.find('y')) {
        (Some(xi), Some(yi)) if xi < yi => (pick(&align[xi + 1..yi]), pick(&align[yi + 1..])),
        _ => (0.5, 0.5),
    };
    Aspect {
        align: Some((x, y)),
        slice,
    }
}

#[must_use]
pub fn view_box_matrix(vb: [f32; 4], viewport: [f32; 4], aspect: Aspect) -> Matrix {
    let [vx, vy, vw, vh] = vb;
    let [x, y, w, h] = viewport;
    let (mut sx, mut sy) = (w / vw, h / vh);
    let (mut tx, mut ty) = (x - vx * sx, y - vy * sy);
    if let Some((ax, ay)) = aspect.align {
        let s = if aspect.slice { sx.max(sy) } else { sx.min(sy) };
        sx = s;
        sy = s;
        tx = x - vx * s + ax * (w - vw * s);
        ty = y - vy * s + ay * (h - vh * s);
    }
    [sx, 0.0, 0.0, sy, tx, ty]
}

#[must_use]
pub fn view_box(text: Option<&str>) -> Option<[f32; 4]> {
    let v = numbers(text?);
    (v.len() == 4 && v[2] > 0.0 && v[3] > 0.0).then(|| [v[0], v[1], v[2], v[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_colours_and_transforms() {
        assert_eq!(numbers("1-2.5.5e1,3 "), vec![1.0, -2.5, 5.0, 3.0]);
        assert_eq!(colour("#f00").unwrap().bytes(), [255, 0, 0]);
        assert_eq!(colour("SteelBlue").unwrap().bytes(), [70, 130, 180]);
        assert_eq!(colour("rgb(10%, 20, 30)").unwrap().bytes(), [26, 20, 30]);
        assert!((colour("rgba(0 0 0 / 50%)").unwrap().a - 0.5).abs() < 1e-6);
        assert_eq!(colour("hsl(120, 100%, 25%)").unwrap().bytes(), [0, 128, 0]);
        assert!(colour("#12").is_none());
        let m = transform("translate(10,20) scale(2)").unwrap();
        assert_eq!(apply(m, 1.0, 1.0), (12.0, 22.0));
        let r = transform("rotate(90 10 10)").unwrap();
        let (x, y) = apply(r, 20.0, 10.0);
        assert!((x - 10.0).abs() < 1e-4 && (y - 20.0).abs() < 1e-4);
        assert!(transform("bogus(1)").is_none());
        for w in NAMED.windows(2) {
            assert!(w[0].0 < w[1].0, "{} before {}", w[0].0, w[1].0);
        }
        let vb = view_box_matrix(
            [0.0, 0.0, 100.0, 50.0],
            [0.0, 0.0, 200.0, 200.0],
            aspect(None),
        );
        assert_eq!(vb, [2.0, 0.0, 0.0, 2.0, 0.0, 50.0]);
    }
}
