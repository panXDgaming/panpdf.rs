use std::borrow::Cow;

#[must_use]
pub fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

#[must_use]
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest);
    }
    let utf16 = |big: bool, rest: &[u8]| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                if big {
                    u16::from_be_bytes([pair[0], pair[1]])
                } else {
                    u16::from_le_bytes([pair[0], pair[1]])
                }
            })
            .collect();
        Cow::Owned(String::from_utf16_lossy(&units))
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(false, rest),
        [0xFE, 0xFF, rest @ ..] => utf16(true, rest),
        _ => String::from_utf8_lossy(bytes),
    }
}

#[must_use]
pub fn unescape(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let window = &rest.as_bytes()[..rest.len().min(12)];
        let Some(end) = window.iter().position(|&b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name.strip_prefix('#').and_then(|number| {
                let value = match number.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => number.parse().ok(),
                };
                value.and_then(char::from_u32)
            }),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    Start {
        name: &'a str,
        attrs: Vec<(&'a str, Cow<'a, str>)>,
        empty: bool,
    },
    End {
        name: &'a str,
    },
    Text(Cow<'a, str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XmlError {
    pub offset: usize,
    pub what: &'static str,
}

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XML error at byte {}: {}", self.offset, self.what)
    }
}

impl std::error::Error for XmlError {}

#[derive(Clone, Debug)]
pub struct Reader<'a> {
    text: &'a str,
    pos: usize,
    pending_end: Option<&'a str>,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(text: &'a str) -> Self {
        Self {
            text,
            pos: 0,
            pending_end: None,
        }
    }

    fn fail(&self, what: &'static str) -> XmlError {
        XmlError {
            offset: self.pos,
            what,
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<Result<Event<'a>, XmlError>> {
        if let Some(name) = self.pending_end.take() {
            return Some(Ok(Event::End { name }));
        }
        loop {
            let rest = &self.text[self.pos..];
            if rest.is_empty() {
                return None;
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.pos += end;
                return Some(Ok(Event::Text(unescape(&rest[..end]))));
            }
            if rest.starts_with("<!--") {
                match rest.find("-->") {
                    Some(end) => self.pos += end + 3,
                    None => return Some(Err(self.fail("unterminated comment"))),
                }
                continue;
            }
            if let Some(body) = rest.strip_prefix("<![CDATA[") {
                return Some(match body.find("]]>") {
                    Some(end) => {
                        self.pos += 9 + end + 3;
                        Ok(Event::Text(Cow::Borrowed(&body[..end])))
                    }
                    None => Err(self.fail("unterminated CDATA")),
                });
            }
            if rest.starts_with("<?") {
                match rest.find("?>") {
                    Some(end) => self.pos += end + 2,
                    None => return Some(Err(self.fail("unterminated instruction"))),
                }
                continue;
            }
            if rest.starts_with("<!") {
                let mut depth = 0_i32;
                let mut end = None;
                for (at, c) in rest.char_indices().skip(2) {
                    match c {
                        '[' => depth += 1,
                        ']' => depth -= 1,
                        '>' if depth <= 0 => {
                            end = Some(at);
                            break;
                        }
                        _ => {}
                    }
                }
                match end {
                    Some(end) => self.pos += end + 1,
                    None => return Some(Err(self.fail("unterminated declaration"))),
                }
                continue;
            }
            if let Some(body) = rest.strip_prefix("</") {
                let Some(end) = body.find('>') else {
                    return Some(Err(self.fail("unterminated end tag")));
                };
                self.pos += 2 + end + 1;
                return Some(Ok(Event::End {
                    name: body[..end].trim(),
                }));
            }
            return Some(self.start_tag());
        }
    }

    fn start_tag(&mut self) -> Result<Event<'a>, XmlError> {
        let text = self.text;
        let body_start = self.pos + 1;
        let bytes = text.as_bytes();
        let mut at = body_start;
        let mut quote = 0_u8;
        while at < bytes.len() {
            let b = bytes[at];
            if quote != 0 {
                if b == quote {
                    quote = 0;
                }
            } else if b == b'"' || b == b'\'' {
                quote = b;
            } else if b == b'>' {
                break;
            }
            at += 1;
        }
        if at >= bytes.len() {
            return Err(self.fail("unterminated start tag"));
        }
        let mut body = &text[body_start..at];
        self.pos = at + 1;
        let empty = body.ends_with('/');
        if empty {
            body = &body[..body.len() - 1];
        }
        let name_end = body
            .find(|c: char| c.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = &body[..name_end];
        if name.is_empty() {
            return Err(self.fail("a tag with no name"));
        }
        let attrs = attributes(&body[name_end..]);
        if empty {
            self.pending_end = Some(name);
        }
        Ok(Event::Start { name, attrs, empty })
    }
}

fn attributes(mut rest: &str) -> Vec<(&str, Cow<'_, str>)> {
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        let Some(eq) = rest.find('=') else {
            return out;
        };
        let name = rest[..eq].trim();
        rest = rest[eq + 1..].trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            return out;
        };
        let Some(end) = rest[1..].find(quote) else {
            return out;
        };
        out.push((name, unescape(&rest[1..=end])));
        rest = &rest[end + 2..];
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    #[must_use]
    pub fn local(&self) -> &str {
        local(&self.name)
    }

    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .or_else(|| self.attrs.iter().find(|(key, _)| local(key) == name))
            .map(|(_, value)| value.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|node| match node {
            Node::Element(element) => Some(element),
            Node::Text(_) => None,
        })
    }

    #[must_use]
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|element| element.local() == name)
    }

    pub fn children_named<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s Element> {
        self.elements()
            .filter(move |element| element.local() == name)
    }

    #[must_use]
    pub fn path(&self, names: &[&str]) -> Option<&Element> {
        names
            .iter()
            .try_fold(self, |element, name| element.child(name))
    }

    #[must_use]
    pub fn descendants(&self, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        let mut stack: Vec<&Element> = vec![self];
        while let Some(element) = stack.pop() {
            for child in element.elements().collect::<Vec<_>>().into_iter().rev() {
                stack.push(child);
            }
            if !std::ptr::eq(element, self) && element.local() == name {
                out.push(element);
            }
        }
        out
    }

    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for node in &self.children {
            match node {
                Node::Text(text) => out.push_str(text),
                Node::Element(element) => element.collect_text(out),
            }
        }
    }
}

pub fn parse(text: &str) -> Result<Element, XmlError> {
    let mut reader = Reader::new(text);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    while let Some(event) = reader.next() {
        match event? {
            Event::Start { name, attrs, .. } => stack.push(Element {
                name: name.to_owned(),
                attrs: attrs
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value.into_owned()))
                    .collect(),
                children: Vec::new(),
            }),
            Event::End { name } => {
                if !stack.iter().any(|open| open.name == name) {
                    continue;
                }
                while let Some(done) = stack.pop() {
                    let matched = done.name == name;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(done)),
                        None => {
                            root.get_or_insert(done);
                        }
                    }
                    if matched {
                        break;
                    }
                }
            }
            Event::Text(text) => {
                if let Some(open) = stack.last_mut() {
                    open.children.push(Node::Text(text.into_owned()));
                }
            }
        }
    }
    while let Some(done) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(Node::Element(done)),
            None => {
                root.get_or_insert(done);
            }
        }
    }
    root.ok_or(XmlError {
        offset: 0,
        what: "no element",
    })
}

pub fn parse_bytes(bytes: &[u8]) -> Result<Element, XmlError> {
    parse(&decode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_in_order() {
        let text = "<?xml version=\"1.0\"?><!-- c --><a:b x='1' y=\"&lt;&#x0E81;\"><c/>t&amp;u<![CDATA[<raw>]]></a:b>";
        let mut reader = Reader::new(text);
        let mut events = Vec::new();
        while let Some(event) = reader.next() {
            events.push(event.unwrap());
        }
        assert_eq!(
            events,
            [
                Event::Start {
                    name: "a:b",
                    attrs: vec![("x", Cow::Borrowed("1")), ("y", Cow::Owned("<ກ".into()))],
                    empty: false
                },
                Event::Start {
                    name: "c",
                    attrs: vec![],
                    empty: true
                },
                Event::End { name: "c" },
                Event::Text(Cow::Owned("t&u".into())),
                Event::Text(Cow::Borrowed("<raw>")),
                Event::End { name: "a:b" },
            ]
        );
    }

    #[test]
    fn tree_and_lookups() {
        let root = parse(
            "<p:sld xmlns:p='x'><p:sp><a:t>ສະບາຍດີ</a:t><a:t> ok</a:t></p:sp><p:sp r:id='rId2'/></p:sld>",
        )
        .unwrap();
        assert_eq!(root.local(), "sld");
        assert_eq!(root.children_named("sp").count(), 2);
        assert_eq!(root.child("sp").unwrap().text(), "ສະບາຍດີ ok");
        assert_eq!(root.descendants("t").len(), 2);
        assert_eq!(
            root.children_named("sp").nth(1).unwrap().attr("id"),
            Some("rId2")
        );
        assert_eq!(unescape("a &bogus; &#3713; &amp"), "a &bogus; ກ &amp");
        assert_eq!(
            decode(&[0xFF, 0xFE, b'<', 0, b'a', 0, b'/', 0, b'>', 0]),
            "<a/>"
        );
    }
}
