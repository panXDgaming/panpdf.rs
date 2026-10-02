use std::collections::HashMap;

pub const MOST_DEPTH: usize = 200;
pub const MOST_ELEMENTS: usize = 400_000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Child>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Child {
    Element(Element),
    Text(String),
}

impl Element {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(slot) = self.attrs.iter_mut().find(|(k, _)| k == name) {
            slot.1 = value.to_owned();
        } else {
            self.attrs.push((name.to_owned(), value.to_owned()));
        }
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            Child::Element(e) => Some(e),
            Child::Text(_) => None,
        })
    }

    pub fn walk<'a>(&'a self, visit: &mut dyn FnMut(&'a Element)) {
        let mut stack = vec![self];
        while let Some(e) = stack.pop() {
            visit(e);
            stack.extend(e.elements().collect::<Vec<_>>().into_iter().rev());
        }
    }

    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for c in &self.children {
            if let Child::Text(t) = c {
                out.push_str(t);
            }
        }
        out
    }

    #[must_use]
    pub fn write(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out);
        out
    }

    fn write_into(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_into(out, v, true);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for c in &self.children {
            match c {
                Child::Element(e) => e.write_into(out),
                Child::Text(t) => escape_into(out, t, false),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }
}

pub fn escape_into(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn element_name(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    lower
        .strip_prefix("svg:")
        .map_or(lower.clone(), str::to_owned)
}

fn attribute_name(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    match lower.split_once(':') {
        Some(("xlink" | "svg", local)) => local.to_owned(),
        _ => lower,
    }
}

#[must_use]
pub fn decode(text: &str, entities: &HashMap<String, String>) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest[1..].find(';').map(|e| e + 1).filter(|e| *e <= 40) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let value = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
            u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
        } else if let Some(dec) = name.strip_prefix('#') {
            dec.parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .map(String::from)
        } else {
            match name {
                "amp" => Some("&".into()),
                "lt" => Some("<".into()),
                "gt" => Some(">".into()),
                "quot" => Some("\"".into()),
                "apos" => Some("'".into()),
                "nbsp" => Some("\u{a0}".into()),
                _ => entities.get(name).cloned(),
            }
        };
        if let Some(value) = value {
            out.push_str(&value);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn entities_of(doctype: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = doctype;
    let mut total = 0;
    while let Some(at) = rest.find("<!ENTITY") {
        rest = &rest[at + 8..];
        let body = rest.trim_start();
        if body.starts_with('%') {
            continue;
        }
        let name_end = body.find(|c: char| c.is_whitespace()).unwrap_or(body.len());
        let name = &body[..name_end];
        let after = body[name_end..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(end) = after[1..].find(quote) else {
            break;
        };
        let value = &after[1..=end];
        total += value.len();
        if total > 1 << 20 {
            break;
        }
        out.insert(name.to_owned(), value.to_owned());
    }
    out
}

#[must_use]
pub fn parse(source: &str) -> Option<Element> {
    let source = source.trim_start_matches('\u{feff}');
    let bytes = source.as_bytes();
    let mut entities = HashMap::new();
    let mut stack: Vec<Element> = vec![Element::new("#document")];
    let mut at = 0;
    let mut count = 0;
    let mut deep = 0_usize;
    while at < bytes.len() {
        let Some(lt) = source[at..].find('<').map(|p| at + p) else {
            push_text(&mut stack, &decode(&source[at..], &entities));
            break;
        };
        if lt > at {
            push_text(&mut stack, &decode(&source[at..lt], &entities));
        }
        let rest = &source[lt..];
        if rest.starts_with("<!--") {
            at = rest.find("-->").map_or(bytes.len(), |e| lt + e + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map_or(bytes.len(), |e| lt + e);
            push_text(&mut stack, &source[(lt + 9).min(end)..end]);
            at = (end + 3).min(bytes.len());
            continue;
        }
        if rest.starts_with("<!DOCTYPE") || rest.starts_with("<!doctype") {
            let end = match (rest.find('['), rest.find('>')) {
                (Some(open), Some(gt)) if open < gt => rest[open..]
                    .find("]")
                    .and_then(|close| rest[open + close..].find('>').map(|g| open + close + g)),
                (_, gt) => gt,
            }
            .map_or(bytes.len(), |e| lt + e + 1);
            entities = entities_of(&source[lt..end]);
            at = end;
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            at = rest.find('>').map_or(bytes.len(), |e| lt + e + 1);
            continue;
        }
        let closing = rest.starts_with("</");
        let name_from = lt + if closing { 2 } else { 1 };
        let name_len = source[name_from..]
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(source.len() - name_from);
        let raw_name = &source[name_from..name_from + name_len];
        if raw_name.is_empty() {
            push_text(&mut stack, "<");
            at = lt + 1;
            continue;
        }
        let name = element_name(raw_name);
        let (attrs, self_closing, end) = attributes(source, name_from + name_len, &entities);
        at = end;
        if closing {
            if deep > 0 {
                deep -= 1;
                continue;
            }
            if let Some(position) = stack.iter().rposition(|e| e.name == name)
                && position > 0
            {
                while stack.len() > position {
                    close_top(&mut stack);
                }
            }
            continue;
        }
        count += 1;
        if count > MOST_ELEMENTS {
            break;
        }
        let element = Element {
            name,
            attrs,
            children: Vec::new(),
        };
        if self_closing {
            if let Some(top) = stack.last_mut() {
                top.children.push(Child::Element(element));
            }
        } else if stack.len() > MOST_DEPTH {
            deep += 1;
        } else {
            stack.push(element);
        }
    }
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    let document = stack.pop()?;
    document.children.into_iter().find_map(|c| match c {
        Child::Element(e) => Some(e),
        Child::Text(_) => None,
    })
}

fn close_top(stack: &mut Vec<Element>) {
    if let Some(done) = stack.pop()
        && let Some(parent) = stack.last_mut()
    {
        parent.children.push(Child::Element(done));
    }
}

fn push_text(stack: &mut [Element], text: &str) {
    if text.is_empty() {
        return;
    }
    let Some(top) = stack.last_mut() else {
        return;
    };
    if let Some(Child::Text(before)) = top.children.last_mut() {
        before.push_str(text);
    } else {
        top.children.push(Child::Text(text.to_owned()));
    }
}

fn attributes(
    source: &str,
    from: usize,
    entities: &HashMap<String, String>,
) -> (Vec<(String, String)>, bool, usize) {
    let bytes = source.as_bytes();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut at = from;
    let mut self_closing = false;
    loop {
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at >= bytes.len() {
            return (attrs, self_closing, at);
        }
        match bytes[at] {
            b'>' => return (attrs, self_closing, at + 1),
            b'/' => {
                self_closing = true;
                at += 1;
                continue;
            }
            _ => {}
        }
        self_closing = false;
        let start = at;
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'=' | b'>')
            && !(bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'>'))
        {
            at += 1;
        }
        let name = attribute_name(&source[start..at]);
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = String::new();
        if at < bytes.len() && bytes[at] == b'=' {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if at < bytes.len() && matches!(bytes[at], b'"' | b'\'') {
                let quote = bytes[at] as char;
                let begin = at + 1;
                let end = source[begin..]
                    .find(quote)
                    .map_or(bytes.len(), |e| begin + e);
                value = decode(&source[begin..end], entities);
                at = (end + 1).min(bytes.len());
            } else {
                let begin = at;
                while at < bytes.len()
                    && !bytes[at].is_ascii_whitespace()
                    && bytes[at] != b'>'
                    && !(bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'>'))
                {
                    at += 1;
                }
                value = decode(&source[begin..at], entities);
            }
        }
        if !name.is_empty() && !attrs.iter().any(|(k, _)| *k == name) {
            attrs.push((name, value));
        }
        if at == start {
            at += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_lower_case_and_forgives() {
        let root = parse(
            "<?xml version='1.0'?><!DOCTYPE svg [<!ENTITY a \"#f00\">]>\
             <svg:svg viewBox='0 0 1 1' xlink:href='x' inkscape:label='L'>\
             <linearGradient id=g/><rect fill='&a;'>t&amp;<b>&#x0E81;</svg:svg>",
        )
        .unwrap();
        assert_eq!(root.name, "svg");
        assert_eq!(root.attr("viewbox"), Some("0 0 1 1"));
        assert_eq!(root.attr("href"), Some("x"));
        assert_eq!(root.attr("inkscape:label"), Some("L"));
        let kids: Vec<&Element> = root.elements().collect();
        assert_eq!(kids[0].name, "lineargradient");
        assert_eq!(kids[1].attr("fill"), Some("#f00"));
        assert_eq!(kids[1].text(), "t&");
        let back = parse(&root.write()).unwrap();
        assert_eq!(back, root);
    }

    #[test]
    fn garbage_never_panics() {
        for text in [
            "",
            "<",
            "</",
            "<a",
            "<a b='",
            "<!--",
            "<![CDATA[",
            "&#xFFFFFFFF;",
            "<!DOCTYPE [",
            "<a></b></a></a>",
        ] {
            let _ = parse(text);
        }
        let deep = "<g>".repeat(5000);
        assert!(parse(&deep).is_some());
    }
}
