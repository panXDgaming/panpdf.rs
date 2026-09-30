use crate::xml::Element;

#[must_use]
pub fn declarations(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in split_top(text, ';') {
        let Some((name, value)) = part.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let mut value = value.trim();
        if let Some(v) = value.strip_suffix("!important") {
            value = v.trim_end();
        }
        if !name.is_empty() && !value.is_empty() {
            out.push((name, value.to_owned()));
        }
    }
    out
}

fn split_top(text: &str, separator: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0_i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == separator && depth <= 0 => {
                out.push(&text[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Compound {
    tag: Option<String>,
    id: Option<String>,
    classes: Vec<String>,
    attrs: Vec<(String, Option<String>)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Combinator {
    Descendant,
    Child,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Selector {
    parts: Vec<(Combinator, Compound)>,
    specificity: (u32, u32, u32),
}

#[derive(Clone, Debug)]
struct Rule {
    selector: Selector,
    declarations: Vec<(String, String)>,
    order: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Sheet {
    rules: Vec<Rule>,
}

fn compound(text: &str) -> Option<Compound> {
    let mut c = Compound::default();
    let mut rest = text;
    let ident_end = |s: &str| s.find(['.', '#', '[', ':']).unwrap_or(s.len());
    let head = ident_end(rest);
    if head > 0 {
        let tag = &rest[..head];
        if tag != "*" {
            c.tag = Some(tag.to_ascii_lowercase());
        }
        rest = &rest[head..];
    }
    while let Some(first) = rest.chars().next() {
        match first {
            '.' | '#' => {
                let body = &rest[1..];
                let end = ident_end(body);
                if end == 0 {
                    return None;
                }
                if first == '.' {
                    c.classes.push(body[..end].to_owned());
                } else {
                    c.id = Some(body[..end].to_owned());
                }
                rest = &body[end..];
            }
            '[' => {
                let end = rest.find(']')?;
                let inside = &rest[1..end];
                let (name, value) = match inside.split_once('=') {
                    Some((n, v)) => (
                        n.trim_end_matches(['~', '|', '^', '$', '*']).trim(),
                        Some(v.trim().trim_matches(['"', '\'']).to_owned()),
                    ),
                    None => (inside.trim(), None),
                };
                c.attrs.push((name.to_ascii_lowercase(), value));
                rest = &rest[end + 1..];
            }
            _ => return None,
        }
    }
    Some(c)
}

fn selector(text: &str) -> Option<Selector> {
    let mut parts = Vec::new();
    let mut next = Combinator::Descendant;
    let spaced = text.replace('>', " > ");
    for token in spaced.split_whitespace() {
        if token == ">" {
            next = Combinator::Child;
            continue;
        }
        if matches!(token, "+" | "~") {
            return None;
        }
        parts.push((next, compound(token)?));
        next = Combinator::Descendant;
    }
    if parts.is_empty() {
        return None;
    }
    let mut specificity = (0, 0, 0);
    for (_, c) in &parts {
        specificity.0 += u32::from(c.id.is_some());
        specificity.1 += (c.classes.len() + c.attrs.len()) as u32;
        specificity.2 += u32::from(c.tag.is_some());
    }
    Some(Selector { parts, specificity })
}

impl Sheet {
    pub fn add(&mut self, text: &str) {
        let text = strip_comments(text);
        let mut rest = text.as_str();
        while let Some(open) = rest.find('{') {
            let head = rest[..open].trim();
            if head.starts_with('@') {
                let Some(close) = matching_brace(rest, open) else {
                    return;
                };
                let lower = head.to_ascii_lowercase();
                if lower.starts_with("@media")
                    && (lower.contains("print")
                        || lower.contains("all")
                        || lower.contains("screen"))
                    && !lower.contains("not ")
                {
                    let inner = rest[open + 1..close].to_owned();
                    self.add(&inner);
                }
                rest = &rest[close + 1..];
                continue;
            }
            let Some(close) = rest[open..].find('}').map(|c| open + c) else {
                return;
            };
            let body = declarations(&rest[open + 1..close]);
            let head = head.rsplit(';').next().unwrap_or(head);
            for part in head.split(',') {
                if let Some(selector) = selector(part.trim()) {
                    let order = self.rules.len();
                    self.rules.push(Rule {
                        selector,
                        declarations: body.clone(),
                        order,
                    });
                }
            }
            rest = &rest[close + 1..];
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    #[must_use]
    pub fn matching(&self, element: &Element, ancestors: &[&Element]) -> Vec<(String, String)> {
        let mut hits: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|r| matches(&r.selector, element, ancestors))
            .collect();
        hits.sort_by_key(|r| (r.selector.specificity, r.order));
        hits.into_iter()
            .flat_map(|r| r.declarations.iter().cloned())
            .collect()
    }
}

fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = rest[at + 2..]
            .find("*/")
            .map_or("", |e| &rest[at + 2 + e + 2..]);
    }
    out.push_str(rest);
    out.replace("<!--", "").replace("-->", "")
}

fn matching_brace(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn matches_compound(c: &Compound, e: &Element) -> bool {
    if let Some(tag) = &c.tag
        && *tag != e.name
    {
        return false;
    }
    if let Some(id) = &c.id
        && e.attr("id") != Some(id.as_str())
    {
        return false;
    }
    if !c.classes.is_empty() {
        let classes = e.attr("class").unwrap_or("");
        if !c
            .classes
            .iter()
            .all(|want| classes.split_whitespace().any(|have| have == want))
        {
            return false;
        }
    }
    c.attrs
        .iter()
        .all(|(name, value)| match (e.attr(name), value) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(have), Some(want)) => have == want,
        })
}

fn matches(selector: &Selector, element: &Element, ancestors: &[&Element]) -> bool {
    let parts = &selector.parts;
    let Some((last_comb, last)) = parts.last() else {
        return false;
    };
    if !matches_compound(last, element) {
        return false;
    }
    let mut combinator = *last_comb;
    let mut up = ancestors.len();
    for (comb, part) in parts[..parts.len() - 1].iter().rev() {
        match combinator {
            Combinator::Child => {
                if up == 0 || !matches_compound(part, ancestors[up - 1]) {
                    return false;
                }
                up -= 1;
            }
            Combinator::Descendant => {
                let mut found = false;
                while up > 0 {
                    up -= 1;
                    if matches_compound(part, ancestors[up]) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
        }
        combinator = *comb;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_match_by_class_type_and_ancestry() {
        let mut sheet = Sheet::default();
        sheet.add(
            "/* c */ .st0{fill:#f00} g > rect.a { stroke: blue !important } text:hover{fill:red} \
             @media print { #x { opacity: .5 } } @font-face { src: x }",
        );
        let mut rect = Element::new("rect");
        rect.set("class", "st0 a");
        rect.set("id", "x");
        let g = Element::new("g");
        let got = sheet.matching(&rect, &[&g]);
        assert_eq!(
            got,
            vec![
                ("fill".to_owned(), "#f00".to_owned()),
                ("stroke".to_owned(), "blue".to_owned()),
                ("opacity".to_owned(), ".5".to_owned()),
            ]
        );
        assert_eq!(sheet.matching(&rect, &[]).len(), 2);
    }
}
