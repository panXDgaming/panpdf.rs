use super::{Colour, Composer, Style, shapes};
use pdf_edit::PenStep;
use pdf_edit::figures::{self, Figure};

const MOST_COMMANDS: usize = 3000;

const DEFAULT_SIZE: (f64, f64) = (400.0, 300.0);

pub(crate) fn keep_height(composer: &Composer<'_>, spec: &str, indent: f64) -> f64 {
    let (wide, high) = size_of(spec).unwrap_or(DEFAULT_SIZE);
    high * scale(composer, (wide, high), indent)
}

pub(crate) fn set_out(
    composer: &mut Composer<'_>,
    spec: &str,
    space_before: f64,
    indent: f64,
) -> Result<(), String> {
    let commands = read(spec)?;
    let (wide, high) = size_of(spec).unwrap_or(DEFAULT_SIZE);
    let scale = scale(composer, (wide, high), indent);
    let left = composer.left() + indent;
    let column = composer.right() - left;
    let x0 = left + (column - wide * scale).max(0.0) / 2.0;
    let top = composer.place(space_before, high * scale);
    composer.top = top;
    let paper = composer.theme().paper.unwrap_or([1.0, 1.0, 1.0]);
    let ink = composer.theme().ink;
    let family = composer.setting.family.to_owned();
    let to_page = |(x, y): (f64, f64)| (x.mul_add(scale, x0), y.mul_add(scale, top));
    for command in &commands {
        match command {
            Command::Size(..) => {}
            Command::Background(colour) => {
                let steps =
                    shapes::rect(x0, top, wide.mul_add(scale, x0), high.mul_add(scale, top));
                composer.shape(steps, None, Some(*colour));
            }
            Command::Draw { steps, paint } => {
                let placed = steps.iter().map(|step| moved(step, &to_page)).collect();
                let paint = paint.seen_on(paper, ink, scale);
                composer.shape(placed, paint.stroke, paint.fill);
            }
            Command::Text {
                at,
                words,
                size,
                colour,
                align,
                bold,
                italic,
            } => {
                let style = Style {
                    family: family.clone(),
                    size: (size * scale).max(1.0),
                    bold: *bold,
                    italic: *italic,
                    colour: Some(colour.map_or(ink, |colour| colour.over(paper))),
                };
                let fitted = composer.fit(words, &style, (wide * scale).max(20.0))?;
                let room = fitted.room.widest;
                let (x, baseline) = to_page(*at);
                let x = x - room * align;
                composer.text((x, baseline - style.size * 0.8), room, fitted);
            }
        }
    }
    composer.top = high.mul_add(scale, top);
    Ok(())
}

fn scale(composer: &Composer<'_>, (wide, high): (f64, f64), indent: f64) -> f64 {
    let column = (composer.right() - composer.left() - indent).max(1.0);
    let sheet = composer.setting.sheet;
    let page = (sheet.high - 2.0 * sheet.margin).max(1.0);
    (column / wide).min(page / high).min(1.0)
}

fn moved(step: &PenStep, to_page: &impl Fn((f64, f64)) -> (f64, f64)) -> PenStep {
    match *step {
        PenStep::Move(point) => PenStep::Move(to_page(point)),
        PenStep::Line(point) => PenStep::Line(to_page(point)),
        PenStep::Curve(one, other, end) => {
            PenStep::Curve(to_page(one), to_page(other), to_page(end))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Tint {
    colour: Colour,
    opacity: f64,
}

impl Tint {
    fn over(self, paper: Colour) -> Colour {
        let a = self.opacity.clamp(0.0, 1.0);
        [0, 1, 2].map(|at| self.colour[at].mul_add(a, paper[at] * (1.0 - a)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Paint {
    fill: Option<Tint>,
    stroke: Option<Tint>,
    width: f64,
}

struct Seen {
    fill: Option<Colour>,
    stroke: Option<(Colour, f64)>,
}

impl Paint {
    fn seen_on(&self, paper: Colour, ink: Colour, scale: f64) -> Seen {
        let mut stroke = self.stroke;
        if self.fill.is_none() && stroke.is_none() {
            stroke = Some(Tint {
                colour: ink,
                opacity: 1.0,
            });
        }
        Seen {
            fill: self.fill.map(|tint| tint.over(paper)),
            stroke: stroke.map(|tint| (tint.over(paper), (self.width * scale).max(0.25))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Command {
    Size(f64, f64),
    Background(Colour),
    Draw {
        steps: Vec<PenStep>,
        paint: Paint,
    },
    Text {
        at: (f64, f64),
        words: String,
        size: f64,
        colour: Option<Tint>,
        align: f64,
        bold: bool,
        italic: bool,
    },
}

fn size_of(spec: &str) -> Option<(f64, f64)> {
    read(spec)
        .ok()?
        .into_iter()
        .find_map(|command| match command {
            Command::Size(wide, high) => Some((wide, high)),
            _ => None,
        })
}

fn read(spec: &str) -> Result<Vec<Command>, String> {
    let mut out = Vec::new();
    for (number, line) in spec.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with("# ") || line == "#" {
            continue;
        }
        if out.len() >= MOST_COMMANDS {
            return Err(format!("a drawing holds at most {MOST_COMMANDS} commands"));
        }
        let command =
            command(line).map_err(|why| format!("drawing, line {}: {why}", number + 1))?;
        out.push((number + 1, command));
    }
    let (wide, high) = out
        .iter()
        .find_map(|(_, command)| match command {
            Command::Size(wide, high) => Some((*wide, *high)),
            _ => None,
        })
        .unwrap_or(DEFAULT_SIZE);
    for (number, command) in &out {
        if let Command::Draw { steps, paint } = command {
            inside((wide, high), steps, paint.width)
                .map_err(|why| format!("drawing, line {number}: {why}"))?;
        }
    }
    Ok(out.into_iter().map(|(_, command)| command).collect())
}

fn inside((wide, high): (f64, f64), steps: &[PenStep], pen: f64) -> Result<(), String> {
    let spare = 0.05 * wide.max(high) + pen;
    for step in steps {
        let points = match *step {
            PenStep::Move(point) | PenStep::Line(point) => vec![point],
            PenStep::Curve(one, other, end) => vec![one, other, end],
        };
        for (x, y) in points {
            if x < -spare || x > wide + spare {
                return Err(format!(
                    "this reaches x {x:.0}, out of the drawing, which is {wide:.0} wide: \
                     keep every shape inside `size {wide:.0} {high:.0}`, or make the size bigger"
                ));
            }
            if y < -spare || y > high + spare {
                return Err(format!(
                    "this reaches y {y:.0}, out of the drawing, which is {high:.0} high: \
                     keep every shape inside `size {wide:.0} {high:.0}`, or make the size bigger"
                ));
            }
        }
    }
    Ok(())
}

fn words(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(letter) = chars.next() {
        match letter {
            '"' => {
                if quoted {
                    out.push(std::mem::take(&mut word));
                    quoted = false;
                } else {
                    quoted = true;
                }
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            letter if letter.is_whitespace() && !quoted => {
                if !word.is_empty() {
                    out.push(std::mem::take(&mut word));
                }
            }
            letter => word.push(letter),
        }
    }
    if quoted {
        return Err("a quote is opened and not closed".to_owned());
    }
    if !word.is_empty() {
        out.push(word);
    }
    Ok(out)
}

struct Said {
    numbers: Vec<f64>,
    points: Vec<(f64, f64)>,
    text: Option<String>,
    options: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Said {
    fn option(&self, key: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn number_option(&self, key: &str) -> Result<Option<f64>, String> {
        self.option(key)
            .map(|value| number(value).ok_or(format!("{key}={value} is not a number")))
            .transpose()
    }

    fn numbers(&self, want: usize, shape: &str, form: &str) -> Result<&[f64], String> {
        match self.numbers.len() {
            had if had < want => Err(format!("{shape} takes {form}")),
            had if had > want => Err(format!("{shape} takes {form}: {want} numbers, not {had}")),
            _ => Ok(&self.numbers[..want]),
        }
    }

    fn paint(&self, open: bool) -> Result<Paint, String> {
        let opacity = self
            .number_option("opacity")?
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        let tint = |key: &str| -> Result<Option<Tint>, String> {
            match self.option(key) {
                None => Ok(None),
                Some(value) if value.eq_ignore_ascii_case("none") => Ok(None),
                Some(value) => colour(value)
                    .map(|colour| Some(Tint { colour, opacity }))
                    .ok_or(format!("{value} is not a colour (a name, #rgb or #rrggbb)")),
            }
        };
        let width = self
            .number_option("width")?
            .or(self.number_option("thickness")?)
            .unwrap_or(2.0);
        if !(width.is_finite() && width > 0.0) {
            return Err("width is a number above zero".to_owned());
        }
        let colour = tint("color")?.or(tint("colour")?);
        let (mut fill, mut stroke) = (tint("fill")?, tint("stroke")?);
        if open {
            stroke = stroke.or(colour).or(fill.take());
            fill = None;
        } else if fill.is_none() {
            fill = colour;
        }
        Ok(Paint {
            fill,
            stroke,
            width,
        })
    }
}

fn number(word: &str) -> Option<f64> {
    word.trim_end_matches("deg")
        .trim_end_matches("px")
        .trim_end_matches("pt")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && value.abs() < 1e7)
}

fn said(rest: &[String]) -> Said {
    let mut out = Said {
        numbers: Vec::new(),
        points: Vec::new(),
        text: None,
        options: Vec::new(),
        flags: Vec::new(),
    };
    for word in rest {
        if let Some((key, value)) = word.split_once('=') {
            out.options
                .push((key.to_ascii_lowercase(), value.trim_matches('"').to_owned()));
        } else if let Some((x, y)) = word.split_once(',')
            && let (Some(x), Some(y)) = (number(x), number(y))
        {
            out.points.push((x, y));
        } else if let Some(value) = number(word) {
            out.numbers.push(value);
        } else if out.text.is_none() && !word.chars().all(|letter| letter.is_ascii_alphabetic()) {
            out.text = Some(word.clone());
        } else {
            out.flags.push(word.to_ascii_lowercase());
        }
    }
    out
}

#[expect(
    clippy::too_many_lines,
    reason = "one arm per command, read as a table"
)]
fn command(line: &str) -> Result<Command, String> {
    let words = words(line)?;
    let Some((name, rest)) = words.split_first() else {
        return Err("an empty command".to_owned());
    };
    let name = name.to_ascii_lowercase();
    let said = said(rest);
    let quoted_text = || -> Option<String> {
        let start = line.find('"')?;
        let end = line.rfind('"')?;
        (end > start).then(|| line[start + 1..end].replace("\\\"", "\""))
    };
    let open = matches!(
        name.as_str(),
        "line" | "arrow" | "curve" | "polyline" | "arc"
    );
    let draw = |steps: Vec<PenStep>, about: (f64, f64)| -> Result<Command, String> {
        let turn = said.number_option("rotate")?.unwrap_or(0.0);
        Ok(Command::Draw {
            steps: figures::turned(&steps, about, turn),
            paint: said.paint(open)?,
        })
    };
    match name.as_str() {
        "size" | "canvas" => {
            let n = said.numbers(2, "size", "a width and a height")?;
            if n[0] > 0.0 && n[1] > 0.0 {
                Ok(Command::Size(n[0], n[1]))
            } else {
                Err("a drawing's size is above zero".to_owned())
            }
        }
        "background" | "bg" => {
            let word = rest.first().ok_or("background takes a colour")?;
            colour(word)
                .map(Command::Background)
                .ok_or(format!("{word} is not a colour"))
        }
        "circle" => {
            let n = said.numbers(3, "circle", "a centre x y and a radius")?;
            let (cx, cy, r) = (n[0], n[1], n[2].abs());
            draw(
                Figure::Ellipse.steps([cx - r, cy - r, cx + r, cy + r], true),
                (cx, cy),
            )
        }
        "ellipse" => {
            let n = said.numbers(4, "ellipse", "a centre x y and two radii")?;
            let (cx, cy, rx, ry) = (n[0], n[1], n[2].abs(), n[3].abs());
            draw(
                Figure::Ellipse.steps([cx - rx, cy - ry, cx + rx, cy + ry], true),
                (cx, cy),
            )
        }
        "line" | "arrow" => {
            let n = said.numbers(4, &name, "two points: x1 y1 x2 y2")?;
            let (from, to) = ((n[0], n[1]), (n[2], n[3]));
            let width = said.paint(true)?.width;
            let steps = if name == "arrow" {
                shapes::arrow(from, to, width)
            } else {
                shapes::line(from.0, from.1, to.0, to.1)
            };
            draw(steps, midpoint(from, to))
        }
        "curve" => {
            let n = said.numbers(8, "curve", "x1 y1, two handles and x2 y2: eight numbers")?;
            let steps = vec![
                PenStep::Move((n[0], n[1])),
                PenStep::Curve((n[2], n[3]), (n[4], n[5]), (n[6], n[7])),
            ];
            draw(steps, midpoint((n[0], n[1]), (n[6], n[7])))
        }
        "polyline" | "polygon" | "path" => {
            let mut points = said.points.clone();
            if points.is_empty() {
                points = said
                    .numbers
                    .chunks_exact(2)
                    .map(|pair| (pair[0], pair[1]))
                    .collect();
            }
            if points.len() < 2 {
                return Err(format!("{name} takes at least two points, as x,y x,y ..."));
            }
            let about = centre_of(&points);
            if name == "polyline" {
                draw(shapes::polyline(&points), about)
            } else {
                draw(shapes::polygon(&points), about)
            }
        }
        "arc" | "pie" | "wedge" => {
            let n = said.numbers(
                5,
                &name,
                "a centre x y, a radius, and from and to in degrees",
            )?;
            let (cx, cy, r) = (n[0], n[1], n[2].abs());
            let (from, to) = (n[3].to_radians(), n[4].to_radians());
            let steps = if name == "arc" {
                let mut steps = vec![PenStep::Move((
                    r.mul_add(from.cos(), cx),
                    r.mul_add(from.sin(), cy),
                ))];
                steps.extend(shapes::arc_steps(cx, cy, (r, r), from, to));
                steps
            } else {
                shapes::wedge(cx, cy, r, from, to)
            };
            draw(steps, (cx, cy))
        }
        "text" | "label" => {
            let n = said
                .numbers
                .get(..2)
                .ok_or("text takes x y and the words in quotes")?;
            let words = quoted_text()
                .or(said.text.clone())
                .ok_or("text takes its words in quotes")?;
            let align = match said.option("align").or(said.option("anchor")) {
                None | Some("left" | "start") => 0.0,
                Some("center" | "centre" | "middle") => 0.5,
                Some("right" | "end") => 1.0,
                Some(other) => return Err(format!("align={other}: left, center or right")),
            };
            let colour = match said.option("color").or(said.option("fill")) {
                None => None,
                Some(value) => Some(Tint {
                    colour: colour(value).ok_or(format!("{value} is not a colour"))?,
                    opacity: said.number_option("opacity")?.unwrap_or(1.0),
                }),
            };
            Ok(Command::Text {
                at: (n[0], n[1]),
                words,
                size: said
                    .number_option("size")?
                    .unwrap_or(16.0)
                    .clamp(2.0, 400.0),
                colour,
                align,
                bold: said.flags.iter().any(|flag| flag == "bold")
                    || said.option("weight") == Some("bold"),
                italic: said.flags.iter().any(|flag| flag == "italic"),
            })
        }
        "triangle" if said.numbers.len() == 6 || said.points.len() == 3 => {
            let points: Vec<(f64, f64)> = if said.points.len() == 3 {
                said.points.clone()
            } else {
                said.numbers
                    .chunks_exact(2)
                    .map(|pair| (pair[0], pair[1]))
                    .collect()
            };
            draw(shapes::polygon(&points), centre_of(&points))
        }
        other => {
            let regular = matches!(other, "regular" | "polygonn" | "ngon");
            let figure = Figure::named(other);
            if figure.is_none() && !regular {
                return Err(format!(
                    "`{other}` is not something a drawing has: size, background, circle, \
                     ellipse, line, arrow, curve, polyline, polygon, arc, pie, text, regular, \
                     or a figure ({})",
                    Figure::ALL.map(Figure::keyword).join(", ")
                ));
            }
            let n = said.numbers(
                4,
                other,
                if other == "triangle" {
                    "the box it fills: left top width height, or three corners x1 y1 x2 y2 x3 y3"
                } else {
                    "the box it fills: left top width height"
                },
            )?;
            let frame = [n[0], n[1], n[0] + n[2], n[1] + n[3]];
            let steps = match figure {
                Some(Figure::Star) => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a small count"
                    )]
                    let points = said
                        .number_option("points")?
                        .unwrap_or(5.0)
                        .round()
                        .clamp(3.0, 64.0) as usize;
                    let inner = said.number_option("inner")?.unwrap_or(0.4);
                    figures::star_figure(points, inner, frame, true)
                }
                Some(figure) => figure.steps(frame, true),
                None => {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a small count"
                    )]
                    let sides = said
                        .number_option("sides")?
                        .unwrap_or(6.0)
                        .round()
                        .clamp(3.0, 64.0) as usize;
                    figures::regular_polygon(sides, 0.0, frame, true)
                }
            };
            draw(steps, (n[0] + n[2] / 2.0, n[1] + n[3] / 2.0))
        }
    }
}

fn midpoint(one: (f64, f64), other: (f64, f64)) -> (f64, f64) {
    (f64::midpoint(one.0, other.0), f64::midpoint(one.1, other.1))
}

fn centre_of(points: &[(f64, f64)]) -> (f64, f64) {
    let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in points {
        left = left.min(*x);
        right = right.max(*x);
        top = top.min(*y);
        bottom = bottom.max(*y);
    }
    midpoint((left, top), (right, bottom))
}

fn colour(word: &str) -> Option<Colour> {
    let word = word.trim().to_ascii_lowercase();
    let part = |value: u32| f64::from(value) / 255.0;
    if let Some(hex) = word.strip_prefix('#') {
        let value = u32::from_str_radix(hex, 16).ok()?;
        return match hex.len() {
            3 => Some(
                [(value >> 8) & 0xF, (value >> 4) & 0xF, value & 0xF]
                    .map(|nibble| part(nibble * 17)),
            ),
            6 => Some([value >> 16, (value >> 8) & 0xFF, value & 0xFF].map(part)),
            _ => None,
        };
    }
    if let Some(inside) = word
        .strip_prefix("rgb(")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        let parts: Vec<f64> = inside
            .split(',')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect();
        return (parts.len() == 3)
            .then(|| [parts[0], parts[1], parts[2]].map(|value| (value / 255.0).clamp(0.0, 1.0)));
    }
    NAMED
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, value)| [value >> 16, (value >> 8) & 0xFF, value & 0xFF].map(part))
}

const NAMED: [(&str, u32); 62] = [
    ("black", 0x00_0000),
    ("white", 0xFF_FFFF),
    ("red", 0xFF_0000),
    ("green", 0x00_8000),
    ("blue", 0x00_00FF),
    ("yellow", 0xFF_FF00),
    ("orange", 0xFF_A500),
    ("purple", 0x80_0080),
    ("pink", 0xFF_C0CB),
    ("brown", 0xA5_2A2A),
    ("gray", 0x80_8080),
    ("grey", 0x80_8080),
    ("lightgray", 0xD3_D3D3),
    ("lightgrey", 0xD3_D3D3),
    ("darkgray", 0xA9_A9A9),
    ("darkgrey", 0xA9_A9A9),
    ("silver", 0xC0_C0C0),
    ("gold", 0xFF_D700),
    ("navy", 0x00_0080),
    ("teal", 0x00_8080),
    ("lime", 0x00_FF00),
    ("olive", 0x80_8000),
    ("maroon", 0x80_0000),
    ("aqua", 0x00_FFFF),
    ("cyan", 0x00_FFFF),
    ("magenta", 0xFF_00FF),
    ("fuchsia", 0xFF_00FF),
    ("skyblue", 0x87_CEEB),
    ("lightblue", 0xAD_D8E6),
    ("deepskyblue", 0x00_BFFF),
    ("dodgerblue", 0x1E_90FF),
    ("royalblue", 0x41_69E1),
    ("darkblue", 0x00_008B),
    ("lightgreen", 0x90_EE90),
    ("darkgreen", 0x00_6400),
    ("forestgreen", 0x22_8B22),
    ("limegreen", 0x32_CD32),
    ("seagreen", 0x2E_8B57),
    ("lightyellow", 0xFF_FFE0),
    ("khaki", 0xF0_E68C),
    ("beige", 0xF5_F5DC),
    ("tan", 0xD2_B48C),
    ("chocolate", 0xD2_691E),
    ("sienna", 0xA0_522D),
    ("salmon", 0xFA_8072),
    ("coral", 0xFF_7F50),
    ("tomato", 0xFF_6347),
    ("crimson", 0xDC_143C),
    ("darkred", 0x8B_0000),
    ("hotpink", 0xFF_69B4),
    ("lightpink", 0xFF_B6C1),
    ("violet", 0xEE_82EE),
    ("lavender", 0xE6_E6FA),
    ("indigo", 0x4B_0082),
    ("plum", 0xDD_A0DD),
    ("orchid", 0xDA_70D6),
    ("turquoise", 0x40_E0D0),
    ("peachpuff", 0xFF_DAB9),
    ("ivory", 0xFF_FFF0),
    ("snow", 0xFF_FAFA),
    ("wheat", 0xF5_DEB3),
    ("skin", 0xF1_C27D),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn draws(line: &str) -> (Vec<PenStep>, Paint) {
        match command(line).unwrap() {
            Command::Draw { steps, paint } => (steps, paint),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn colours_are_read_by_name_and_by_number() {
        assert_eq!(colour("#ff0000"), Some([1.0, 0.0, 0.0]));
        assert_eq!(colour("#0f0"), Some([0.0, 1.0, 0.0]));
        assert_eq!(colour("Navy"), Some([0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(colour("rgb(255, 255, 0)"), Some([1.0, 1.0, 0.0]));
        assert_eq!(colour("#12"), None);
        assert_eq!(colour("blurple"), None);
    }

    #[test]
    fn a_circle_is_where_its_centre_and_radius_put_it() {
        let (steps, paint) = draws("circle 100 50 20 fill=yellow stroke=#000 width=3");
        let reached: Vec<(f64, f64)> = steps
            .iter()
            .map(|step| match *step {
                PenStep::Move(at) | PenStep::Line(at) | PenStep::Curve(_, _, at) => at,
            })
            .collect();
        for want in [(120.0, 50.0), (100.0, 70.0), (80.0, 50.0), (100.0, 30.0)] {
            assert!(
                reached
                    .iter()
                    .any(|end| (end.0 - want.0).abs() < 1e-9 && (end.1 - want.1).abs() < 1e-9),
                "{want:?} in {reached:?}"
            );
        }
        assert_eq!(
            paint.fill.unwrap().colour.map(f64::to_bits),
            [1.0, 1.0, 0.0].map(f64::to_bits)
        );
        assert_eq!(
            paint.stroke.unwrap().colour.map(f64::to_bits),
            [0.0_f64; 3].map(f64::to_bits)
        );
        assert!((paint.width - 3.0).abs() < 1e-12);
    }

    #[test]
    fn every_figure_is_a_command() {
        for figure in Figure::ALL {
            let (steps, _) = draws(&format!("{} 10 10 80 60 fill=red", figure.keyword()));
            assert!(steps.len() >= 3, "{figure:?}");
        }
        let (star, _) = draws("star 0 0 100 100 points=6 inner=0.5");
        assert_eq!(star.len(), 13);
        let (heptagon, _) = draws("regular 0 0 100 100 sides=7");
        assert_eq!(heptagon.len(), 8);
    }

    #[test]
    fn opacity_mixes_with_the_paper() {
        let (_, paint) = draws("rect 0 0 10 10 fill=red opacity=0.5");
        let seen = paint.seen_on([1.0, 1.0, 1.0], [0.0; 3], 1.0);
        assert_eq!(seen.fill, Some([1.0, 0.5, 0.5]));
        assert!(seen.stroke.is_none());
    }

    #[test]
    fn an_unpainted_shape_is_outlined() {
        let (_, paint) = draws("triangle 0 0 10 10");
        let seen = paint.seen_on([1.0; 3], [0.1, 0.1, 0.1], 2.0);
        assert_eq!(seen.stroke, Some(([0.1, 0.1, 0.1], 4.0)));
    }

    #[test]
    fn text_keeps_its_quoted_words_and_options() {
        let Command::Text {
            at,
            words,
            size,
            align,
            bold,
            ..
        } = command(r#"text 200 40 "Hello, little star" size=20 align=center bold"#).unwrap()
        else {
            panic!("text")
        };
        assert_eq!(at, (200.0, 40.0));
        assert_eq!(words, "Hello, little star");
        assert!((size - 20.0).abs() < 1e-12 && (align - 0.5).abs() < 1e-12 && bold);
    }

    #[test]
    fn a_bad_line_is_named() {
        let why = read("size 100 100\ncircle 1 2\n").unwrap_err();
        assert!(why.contains("line 2"), "{why}");
        let why = read("blob 1 2 3 4").unwrap_err();
        assert!(why.contains("blob") && why.contains("heart"), "{why}");
        assert!(read("size 100 100\n# a comment\n// another\nrect 0 0 10 10\n").is_ok());
    }

    #[test]
    fn a_triangle_is_drawn_through_three_corners() {
        let (steps, _) = draws("triangle 199 51 222 31 221 57");
        let corners: Vec<(f64, f64)> = steps
            .iter()
            .filter_map(|step| match step {
                PenStep::Move(point) | PenStep::Line(point) => Some(*point),
                PenStep::Curve(..) => None,
            })
            .collect();
        for corner in [(199.0, 51.0), (222.0, 31.0), (221.0, 57.0)] {
            assert!(corners.contains(&corner), "{corners:?}");
        }
        assert!(corners.iter().all(|(x, _)| (199.0..=222.0).contains(x)));
        let (pointed, _) = draws("triangle 199,51 222,31 221,57");
        assert_eq!(pointed, steps);
        assert!(command("triangle 0 0 10 10").is_ok(), "a box still fills");
    }

    #[test]
    fn a_number_too_many_is_refused() {
        let why = command("circle 10 10 5 7").unwrap_err();
        assert!(why.contains("3 numbers, not 4"), "{why}");
        let why = command("rect 0 0 10 10 12").unwrap_err();
        assert!(why.contains("4 numbers, not 5"), "{why}");
        assert!(
            command("text 10 20 \"5\"").is_ok(),
            "words that read as a number"
        );
    }

    #[test]
    fn a_shape_out_of_its_drawing_is_refused() {
        let why = read("size 460 105\ncircle 40 40 20\ntriangle 409 50 437 34\n").unwrap_err();
        assert!(why.contains("line 3") && why.contains("460"), "{why}");
        assert!(read("size 100 100\nrect 0 0 100 100 width=4\nline 0 50 104 50\n").is_ok());
        let why = read("circle 400 150 40\n").unwrap_err();
        assert!(why.contains("400 wide"), "the default size: {why}");
    }

    #[test]
    fn rotate_turns_about_the_middle() {
        let (upright, _) = draws("rect 0 0 10 10");
        let (turned, _) = draws("rect 0 0 10 10 rotate=90");
        assert_eq!(upright.len(), turned.len());
        let PenStep::Move(first) = turned[0] else {
            panic!()
        };
        assert!(
            (first.0 - 10.0).abs() < 1e-9 && first.1.abs() < 1e-9,
            "{first:?}"
        );
    }
}
