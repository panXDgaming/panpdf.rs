#[must_use]
pub fn builtin(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "#,##0_);(#,##0)",
        6 => "#,##0_);[Red](#,##0)",
        7 => "#,##0.00_);(#,##0.00)",
        8 => "#,##0.00_);[Red](#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "yyyy-mm-dd",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "yyyy-mm-dd h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatted {
    pub text: String,
    pub color: Option<u32>,
}

fn color_name(name: &str) -> Option<u32> {
    Some(match name.to_ascii_lowercase().as_str() {
        "black" => 0x000000,
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "green" => 0x00FF00,
        "blue" => 0x0000FF,
        "yellow" => 0xFFFF00,
        "magenta" => 0xFF00FF,
        "cyan" => 0x00FFFF,
        _ => return None,
    })
}

fn sections(code: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut bracket = false;
    let mut escape = false;
    for c in code.chars() {
        if escape {
            out.last_mut().unwrap_or(&mut String::new()).push(c);
            escape = false;
            continue;
        }
        match c {
            '\\' if !quoted => {
                escape = true;
                if let Some(last) = out.last_mut() {
                    last.push(c);
                }
                continue;
            }
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            ';' if !quoted && !bracket => {
                out.push(String::new());
                continue;
            }
            _ => {}
        }
        if let Some(last) = out.last_mut() {
            last.push(c);
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
enum Condition {
    Lt(f64),
    Le(f64),
    Gt(f64),
    Ge(f64),
    Eq(f64),
    Ne(f64),
}

impl Condition {
    fn holds(&self, v: f64) -> bool {
        match *self {
            Self::Lt(x) => v < x,
            Self::Le(x) => v <= x,
            Self::Gt(x) => v > x,
            Self::Ge(x) => v >= x,
            Self::Eq(x) => (v - x).abs() < 1e-12,
            Self::Ne(x) => (v - x).abs() >= 1e-12,
        }
    }
}

struct Section {
    body: String,
    color: Option<u32>,
    condition: Option<Condition>,
}

fn parse_section(text: &str) -> Section {
    let mut body = String::new();
    let mut color = None;
    let mut condition = None;
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        let before = &rest[..start];
        if before.matches('"').count() % 2 == 1 {
            body.push_str(&rest[..=start]);
            rest = &rest[start + 1..];
            continue;
        }
        let Some(end) = rest[start..].find(']') else {
            break;
        };
        let inner = &rest[start + 1..start + end];
        body.push_str(before);
        let lower = inner.to_ascii_lowercase();
        if let Some(c) = color_name(inner) {
            color = Some(c);
        } else if let Some(n) = lower.strip_prefix("color") {
            color = n.parse::<u32>().ok().map(|_| 0x000000);
        } else if let Some(locale) = inner.strip_prefix('$') {
            let currency = locale.split('-').next().unwrap_or("");
            if !currency.is_empty() {
                body.push('"');
                body.push_str(currency);
                body.push('"');
            }
        } else if inner.starts_with(['<', '>', '=']) {
            let (op, number) = if let Some(n) = inner.strip_prefix("<=") {
                ("<=", n)
            } else if let Some(n) = inner.strip_prefix(">=") {
                (">=", n)
            } else if let Some(n) = inner.strip_prefix("<>") {
                ("<>", n)
            } else {
                (&inner[..1], &inner[1..])
            };
            if let Ok(x) = number.trim().parse::<f64>() {
                condition = Some(match op {
                    "<" => Condition::Lt(x),
                    "<=" => Condition::Le(x),
                    ">" => Condition::Gt(x),
                    ">=" => Condition::Ge(x),
                    "<>" => Condition::Ne(x),
                    _ => Condition::Eq(x),
                });
            }
        } else if matches!(lower.as_str(), "h" | "hh" | "m" | "mm" | "s" | "ss") {
            body.push('[');
            body.push_str(inner);
            body.push(']');
        }
        rest = &rest[start + end + 1..];
    }
    body.push_str(rest);
    Section {
        body,
        color,
        condition,
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Literal(String),
    Digit(char),
    Point,
    Comma,
    Percent,
    Exponent(bool),
    Slash,
    At,
    Date(String),
    AmPm(String),
    Elapsed(char),
}

fn tokenize(body: &str) -> Vec<Token> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '"' => {
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    text.push(chars[i]);
                    i += 1;
                }
                out.push(Token::Literal(text));
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1) {
                    out.push(Token::Literal(n.to_string()));
                    i += 1;
                }
            }
            '_' => {
                i += 1;
                out.push(Token::Literal(" ".to_owned()));
            }
            '*' => {
                i += 1;
            }
            '0' | '#' | '?' => out.push(Token::Digit(c)),
            '.' => out.push(Token::Point),
            ',' => out.push(Token::Comma),
            '%' => out.push(Token::Percent),
            '/' => out.push(Token::Slash),
            '@' => out.push(Token::At),
            'E' | 'e' if matches!(chars.get(i + 1), Some('+' | '-')) => {
                out.push(Token::Exponent(chars[i + 1] == '+'));
                i += 1;
            }
            '[' => {
                let end = chars[i..]
                    .iter()
                    .position(|&x| x == ']')
                    .map_or(chars.len(), |p| i + p);
                let inner: String = chars[i + 1..end].iter().collect();
                if let Some(first) = inner.chars().next() {
                    out.push(Token::Elapsed(first.to_ascii_lowercase()));
                }
                i = end;
            }
            'A' | 'a'
                if body[byte_index(&chars, i)..]
                    .to_ascii_uppercase()
                    .starts_with("AM/PM") =>
            {
                out.push(Token::AmPm("AM/PM".to_owned()));
                i += 4;
            }
            'A' | 'a'
                if body[byte_index(&chars, i)..]
                    .to_ascii_uppercase()
                    .starts_with("A/P") =>
            {
                out.push(Token::AmPm("A/P".to_owned()));
                i += 2;
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => {
                let lower = c.to_ascii_lowercase();
                let mut run = String::new();
                while i < chars.len() && chars[i].to_ascii_lowercase() == lower {
                    run.push(lower);
                    i += 1;
                }
                out.push(Token::Date(run));
                continue;
            }
            'G' | 'g' if body[byte_index(&chars, i)..].eq_ignore_ascii_case("general") => {
                out.push(Token::Literal("\u{0}GENERAL".to_owned()));
                i += 6;
            }
            _ => out.push(Token::Literal(c.to_string())),
        }
        i += 1;
    }
    out
}

fn byte_index(chars: &[char], i: usize) -> usize {
    chars[..i].iter().map(|c| c.len_utf8()).sum()
}

#[must_use]
pub fn general(v: f64) -> String {
    if v == 0.0 {
        return "0".to_owned();
    }
    if !v.is_finite() {
        return "#NUM!".to_owned();
    }
    let abs = v.abs();
    if (1e-9..1e11).contains(&abs) {
        let digits_before = if abs >= 1.0 {
            abs.log10().floor() as i32 + 1
        } else {
            1
        };
        let decimals = (10 - digits_before).clamp(0, 9) as usize;
        let mut text = format!("{v:.decimals$}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        if text.len() <= 12 {
            return text;
        }
    }
    let text = format!("{v:.5E}");
    let (mantissa, exponent) = text.split_once('E').unwrap_or((&text, "0"));
    let mut mantissa = mantissa.to_owned();
    if mantissa.contains('.') {
        while mantissa.ends_with('0') {
            mantissa.pop();
        }
        if mantissa.ends_with('.') {
            mantissa.pop();
        }
    }
    let e: i32 = exponent.parse().unwrap_or(0);
    format!("{mantissa}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

#[must_use]
pub fn civil(serial: f64, date1904: bool) -> Option<(i64, u32, u32, u32, u32, u32, f64)> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let mut days = serial.floor() as i64;
    let seconds_total = ((serial - serial.floor()) * 86_400.0 * 1000.0).round() / 1000.0;
    let (mut secs, fraction) = (
        seconds_total.floor() as i64,
        seconds_total - seconds_total.floor(),
    );
    if secs >= 86_400 {
        secs -= 86_400;
        days += 1;
    }
    let (y, m, d) = if date1904 {
        from_days(days + 1462)
    } else if days == 60 {
        (1900, 2, 29)
    } else if days == 0 {
        (1900, 1, 0)
    } else {
        from_days(if days < 60 { days } else { days - 1 })
    };
    Some((
        y,
        m,
        d,
        (secs / 3600) as u32,
        ((secs / 60) % 60) as u32,
        (secs % 60) as u32,
        fraction,
    ))
}

fn from_days(days: i64) -> (i64, u32, u32) {
    let z = days - 25_568 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn weekday(y: i64, m: u32, d: u32) -> usize {
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    ((y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + i64::from(d)) % 7) as usize
}

fn is_date(tokens: &[Token]) -> bool {
    tokens
        .iter()
        .any(|t| matches!(t, Token::Date(_) | Token::AmPm(_) | Token::Elapsed(_)))
}

fn format_date(tokens: &[Token], v: f64, date1904: bool) -> String {
    let Some((y, mo, d, h, mi, s, fraction)) = civil(v, date1904) else {
        return "#".repeat(8);
    };
    let ampm = tokens.iter().any(|t| matches!(t, Token::AmPm(_)));
    let mut out = String::new();
    for (k, token) in tokens.iter().enumerate() {
        match token {
            Token::Date(run) => {
                let first = run.chars().next().unwrap_or('y');
                let minute = first == 'm'
                    && run.len() <= 2
                    && (tokens[..k]
                        .iter()
                        .rev()
                        .find(|t| matches!(t, Token::Date(_) | Token::Elapsed(_)))
                        .is_some_and(|t| {
                            matches!(t, Token::Date(r) if r.starts_with('h'))
                                || matches!(t, Token::Elapsed('h'))
                        })
                        || tokens[k + 1..]
                            .iter()
                            .find(|t| matches!(t, Token::Date(_)))
                            .is_some_and(|t| matches!(t, Token::Date(r) if r.starts_with('s'))));
                let text = match (first, run.len()) {
                    ('y', 1 | 2) => format!("{:02}", y.rem_euclid(100)),
                    ('y', _) => format!("{y:04}"),
                    ('m', 1 | 2) if minute => {
                        if run.len() == 2 {
                            format!("{mi:02}")
                        } else {
                            mi.to_string()
                        }
                    }
                    ('m', 1) => mo.to_string(),
                    ('m', 2) => format!("{mo:02}"),
                    ('m', 3) => MONTHS[(mo as usize).clamp(1, 12) - 1][..3].to_owned(),
                    ('m', 5) => MONTHS[(mo as usize).clamp(1, 12) - 1][..1].to_owned(),
                    ('m', _) => MONTHS[(mo as usize).clamp(1, 12) - 1].to_owned(),
                    ('d', 1) => d.to_string(),
                    ('d', 2) => format!("{d:02}"),
                    ('d', 3) => DAYS[weekday(y, mo, d)][..3].to_owned(),
                    ('d', _) => DAYS[weekday(y, mo, d)].to_owned(),
                    ('h', n) => {
                        let hour = if ampm {
                            if h % 12 == 0 { 12 } else { h % 12 }
                        } else {
                            h
                        };
                        if n >= 2 {
                            format!("{hour:02}")
                        } else {
                            hour.to_string()
                        }
                    }
                    ('s', n) => {
                        let mut text = if n >= 2 {
                            format!("{s:02}")
                        } else {
                            s.to_string()
                        };
                        if matches!(tokens.get(k + 1), Some(Token::Point)) {
                            let places = tokens[k + 2..]
                                .iter()
                                .take_while(|t| matches!(t, Token::Digit('0')))
                                .count();
                            if places > 0 {
                                let f = format!("{fraction:.places$}");
                                text.push_str(&f[1..]);
                            }
                        }
                        text
                    }
                    _ => run.clone(),
                };
                out.push_str(&text);
            }
            Token::Elapsed(unit) => {
                let total = v * 24.0;
                let text = match unit {
                    'h' => (total.floor() as i64).to_string(),
                    'm' => ((v * 1440.0).floor() as i64).to_string(),
                    _ => ((v * 86_400.0).floor() as i64).to_string(),
                };
                out.push_str(&text);
            }
            Token::AmPm(kind) => {
                let pm = h >= 12;
                out.push_str(match (kind.as_str(), pm) {
                    ("AM/PM", false) => "AM",
                    ("AM/PM", true) => "PM",
                    (_, false) => "A",
                    (_, true) => "P",
                });
            }
            Token::Point => {
                if matches!(tokens.get(k.wrapping_sub(1)), Some(Token::Date(r)) if r.starts_with('s'))
                {
                    continue;
                }
                out.push('.');
            }
            Token::Digit(_) => {}
            Token::Literal(text) => out.push_str(text),
            Token::Comma => out.push(','),
            Token::Slash => out.push('/'),
            Token::Percent => out.push('%'),
            Token::At | Token::Exponent(_) => {}
        }
    }
    out
}

fn fill_integer(digits: &str, pattern: &[char], group: bool) -> String {
    let zeros = pattern.iter().filter(|c| **c == '0').count();
    let qs = pattern.iter().filter(|c| **c == '?').count();
    let mut body = digits.trim_start_matches('0').to_owned();
    let min = zeros;
    while body.len() < min {
        body.insert(0, '0');
    }
    if group && body.len() > 3 {
        let mut grouped = String::new();
        for (k, c) in body.chars().enumerate() {
            if k > 0 && (body.len() - k).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(c);
        }
        body = grouped;
    }
    let pad = (zeros + qs).saturating_sub(body.len().max(zeros));
    format!("{}{body}", " ".repeat(if qs > 0 { pad } else { 0 }))
}

fn format_number(tokens: &[Token], v: f64, signed: bool) -> String {
    let first_digit = tokens.iter().position(|t| matches!(t, Token::Digit(_)));
    let Some(first_digit) = first_digit else {
        let mut out = String::new();
        if signed && v < 0.0 {
            out.push('-');
        }
        for t in tokens {
            if let Token::Literal(text) = t {
                out.push_str(text);
            }
        }
        return out;
    };
    let percent = tokens
        .iter()
        .filter(|t| matches!(t, Token::Percent))
        .count();
    let exponent = tokens.iter().position(|t| matches!(t, Token::Exponent(_)));
    let slash = tokens.iter().position(|t| matches!(t, Token::Slash));
    let last_digit = tokens
        .iter()
        .rposition(|t| matches!(t, Token::Digit(_)))
        .unwrap_or(first_digit);
    let scale_commas = tokens[last_digit + 1..]
        .iter()
        .take_while(|t| matches!(t, Token::Comma))
        .count();
    let mut value = v.abs() * 100_f64.powi(percent as i32) / 1000_f64.powi(scale_commas as i32);
    let number_end = exponent.unwrap_or(last_digit + 1 + scale_commas);
    let point = tokens[..number_end]
        .iter()
        .position(|t| matches!(t, Token::Point));
    let int_pattern: Vec<char> = tokens[first_digit..point.unwrap_or(number_end)]
        .iter()
        .filter_map(|t| {
            if let Token::Digit(c) = t {
                Some(*c)
            } else {
                None
            }
        })
        .collect();
    let group = tokens[first_digit..point.unwrap_or(number_end)]
        .iter()
        .any(|t| matches!(t, Token::Comma));
    let frac_pattern: Vec<char> = point.map_or_else(Vec::new, |p| {
        tokens[p + 1..number_end]
            .iter()
            .filter_map(|t| {
                if let Token::Digit(c) = t {
                    Some(*c)
                } else {
                    None
                }
            })
            .collect()
    });
    let mut number = String::new();
    if let Some(slash) = slash.filter(|s| *s > first_digit) {
        let whole_part = tokens[first_digit..slash]
            .iter()
            .any(|t| matches!(t, Token::Literal(l) if l == " "));
        let denominator_digits = tokens[slash + 1..]
            .iter()
            .take_while(|t| matches!(t, Token::Digit(_)))
            .count()
            .max(1);
        let limit = 10_i64.pow(denominator_digits as u32) - 1;
        let whole = if whole_part { value.trunc() } else { 0.0 };
        let rest = value - whole;
        let (mut best_n, mut best_d, mut best_err) = (0_i64, 1_i64, f64::MAX);
        for d in 1..=limit {
            let n = (rest * d as f64).round() as i64;
            let err = (rest - n as f64 / d as f64).abs();
            if err < best_err - 1e-12 {
                best_n = n;
                best_d = d;
                best_err = err;
            }
        }
        if whole_part && whole > 0.0 {
            number.push_str(&format!("{}", whole as i64));
            if best_n > 0 {
                number.push(' ');
            }
        }
        if best_n > 0 || whole == 0.0 {
            number.push_str(&format!("{best_n}/{best_d}"));
        }
        let mut out = String::new();
        if signed && v < 0.0 {
            out.push('-');
        }
        for t in &tokens[..first_digit] {
            if let Token::Literal(text) = t {
                out.push_str(text);
            }
        }
        out.push_str(&number);
        return out;
    }
    let mut exponent_text = String::new();
    if let Some(e) = exponent {
        let plus = matches!(tokens[e], Token::Exponent(true));
        let exp_digits = tokens[e + 1..]
            .iter()
            .take_while(|t| matches!(t, Token::Digit(_)))
            .count()
            .max(1);
        let int_digits = int_pattern.len().max(1) as i32;
        let mut power = if value == 0.0 {
            0
        } else {
            value.log10().floor() as i32
        };
        if int_pattern.len() > 1 && int_pattern.contains(&'#') {
            power = power.div_euclid(int_digits) * int_digits;
        } else {
            power -= int_digits - 1;
        }
        value /= 10_f64.powi(power);
        let sign = if power < 0 {
            "-"
        } else if plus {
            "+"
        } else {
            ""
        };
        exponent_text = format!("E{sign}{:0width$}", power.abs(), width = exp_digits);
    }
    let decimals = frac_pattern.len();
    let rounded = format!("{value:.decimals$}");
    let (int_digits, frac_digits) = rounded.split_once('.').unwrap_or((&rounded, ""));
    number.push_str(&fill_integer(int_digits, &int_pattern, group));
    if point.is_some() {
        let mut frac: Vec<char> = frac_digits.chars().collect();
        for (k, pattern) in frac_pattern.iter().enumerate().rev() {
            if *pattern == '0' {
                break;
            }
            if frac.get(k) == Some(&'0') && k + 1 == frac.len() {
                frac.pop();
                if *pattern == '?' {
                    frac.push(' ');
                }
            }
        }
        number.push('.');
        number.extend(frac);
    }
    number.push_str(&exponent_text);
    let mut out = String::new();
    if signed && v < 0.0 && number.chars().any(|c| c.is_ascii_digit() && c != '0') {
        out.push('-');
    }
    let mut placed = false;
    for (k, t) in tokens.iter().enumerate() {
        let inside = k >= first_digit
            && k < number_end
                + exponent.map_or(0, |e| {
                    1 + tokens[e + 1..]
                        .iter()
                        .take_while(|t| matches!(t, Token::Digit(_)))
                        .count()
                });
        match t {
            _ if inside && !placed => {
                out.push_str(&number);
                placed = true;
            }
            _ if inside => {}
            Token::Literal(text) => out.push_str(text),
            Token::Percent => out.push('%'),
            Token::Comma if k > last_digit => {}
            Token::Comma => out.push(','),
            Token::Point => out.push('.'),
            Token::Slash => out.push('/'),
            _ => {}
        }
    }
    out
}

#[must_use]
pub fn format(code: &str, v: f64, date1904: bool) -> Formatted {
    let code = if code.eq_ignore_ascii_case("general") || code.is_empty() {
        return Formatted {
            text: general(v),
            color: None,
        };
    } else {
        code
    };
    let parts: Vec<Section> = sections(code).iter().map(|s| parse_section(s)).collect();
    let has_conditions = parts.iter().any(|p| p.condition.is_some());
    let (section, signed) = if has_conditions {
        let found = parts
            .iter()
            .position(|p| p.condition.as_ref().is_some_and(|c| c.holds(v)))
            .or_else(|| parts.iter().position(|p| p.condition.is_none()))
            .unwrap_or(0);
        (found, parts[found].condition.is_none() && v < 0.0)
    } else {
        match (parts.len(), v) {
            (1, _) => (0, true),
            (_, v) if v > 0.0 => (0, false),
            (_, v) if v < 0.0 => (1, false),
            (2, _) => (0, false),
            (_, _) => (2, false),
        }
    };
    let part = &parts[section.min(parts.len() - 1)];
    let tokens = tokenize(&part.body);
    if tokens
        .iter()
        .any(|t| matches!(t, Token::Literal(l) if l.starts_with('\u{0}')))
    {
        let mut text = String::new();
        for t in &tokens {
            match t {
                Token::Literal(l) if l.starts_with('\u{0}') => {
                    text.push_str(&general(if signed { v } else { v.abs() }));
                }
                Token::Literal(l) => text.push_str(l),
                _ => {}
            }
        }
        return Formatted {
            text,
            color: part.color,
        };
    }
    let text = if is_date(&tokens) {
        format_date(&tokens, v, date1904)
    } else {
        format_number(&tokens, v, signed)
    };
    Formatted {
        text,
        color: part.color,
    }
}

#[must_use]
pub fn format_text(code: &str, text: &str) -> Formatted {
    let parts: Vec<String> = sections(code);
    let Some(part) = parts
        .get(3)
        .or_else(|| parts.first().filter(|p| p.contains('@')))
    else {
        return Formatted {
            text: text.to_owned(),
            color: None,
        };
    };
    let section = parse_section(part);
    let mut out = String::new();
    for t in tokenize(&section.body) {
        match t {
            Token::At => out.push_str(text),
            Token::Literal(l) => out.push_str(&l),
            _ => {}
        }
    }
    Formatted {
        text: out,
        color: section.color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(code: &str, v: f64) -> String {
        format(code, v, false).text
    }

    #[test]
    fn numbers() {
        assert_eq!(f("General", 1200.0), "1200");
        assert_eq!(f("General", 0.1 + 0.2), "0.3");
        assert_eq!(f("General", 1.0 / 3.0), "0.333333333");
        assert_eq!(f("#,##0", 1200.0), "1,200");
        assert_eq!(f("#,##0.00", 1234567.891), "1,234,567.89");
        assert_eq!(f("0.00%", 0.125), "12.50%");
        assert_eq!(f("0%", 0.5), "50%");
        assert_eq!(f("#,##0.00_);[Red](#,##0.00)", -500.0), "(500.00)");
        assert_eq!(
            format("#,##0.00_);[Red](#,##0.00)", -500.0, false).color,
            Some(0xFF0000)
        );
        assert_eq!(f("#,##0.00_);[Red](#,##0.00)", 500.0), "500.00 ");
        assert_eq!(f("\"₭\" #,##0", 1500.0), "₭ 1,500");
        assert_eq!(f("[$₭-454] #,##0", 1500.0), "₭ 1,500");
        assert_eq!(f("0.00E+00", 12345.0), "1.23E+04");
        assert_eq!(f("0", -3.0), "-3");
        assert_eq!(f("#,##0;(#,##0);\"-\"", 0.0), "-");
        assert_eq!(f("# ?/?", 1.5), "1 1/2");
        assert_eq!(f("#,##0,\"K\"", 12_345.0), "12K");
        assert_eq!(f("000", 7.0), "007");
    }

    #[test]
    fn dates() {
        assert_eq!(f("yyyy-mm-dd", 46289.0), "2026-09-24");
        assert_eq!(f("d-mmm-yy", 46289.0), "24-Sep-26");
        assert_eq!(
            f("dddd, mmmm d, yyyy", 46289.0),
            "Thursday, September 24, 2026"
        );
        assert_eq!(f("h:mm AM/PM", 0.75), "6:00 PM");
        assert_eq!(f("hh:mm:ss", 0.5 + 1.0 / 86400.0), "12:00:01");
        assert_eq!(f("[h]:mm", 1.5), "36:00");
        assert_eq!(f("mm/dd/yyyy", 1.0), "01/01/1900");
        assert_eq!(f("yyyy-mm-dd", 61.0), "1900-03-01");
    }

    #[test]
    fn text() {
        assert_eq!(format_text("@", "abc").text, "abc");
        assert_eq!(format_text("0;0;0;\"x\"@", "abc").text, "xabc");
    }
}
