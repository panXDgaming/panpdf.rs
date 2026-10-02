use std::collections::HashMap;

pub type NodeId = usize;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Element {
        tag: String,
        attrs: Vec<(String, String)>,
        children: Vec<NodeId>,
        parent: Option<NodeId>,
    },
    Text {
        text: String,
        parent: Option<NodeId>,
    },
}

#[derive(Clone, Debug)]
pub struct Dom {
    pub nodes: Vec<Node>,
}

impl Dom {
    #[must_use]
    pub fn tag(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id] {
            Node::Element { tag, .. } => Some(tag),
            Node::Text { .. } => None,
        }
    }

    #[must_use]
    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        match &self.nodes[id] {
            Node::Element { attrs, .. } => attrs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str()),
            Node::Text { .. } => None,
        }
    }

    #[must_use]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        match &self.nodes[id] {
            Node::Element { children, .. } => children,
            Node::Text { .. } => &[],
        }
    }

    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes[id] {
            Node::Element { parent, .. } | Node::Text { parent, .. } => *parent,
        }
    }

    #[must_use]
    pub fn find_all(&self, tag: &str) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![0];
        while let Some(id) = stack.pop() {
            if self.tag(id) == Some(tag) {
                out.push(id);
            }
            stack.extend(self.children(id).iter().rev());
        }
        out
    }

    #[must_use]
    pub fn text_of(&self, id: NodeId) -> String {
        let mut out = String::new();
        let mut stack = vec![id];
        while let Some(at) = stack.pop() {
            match &self.nodes[at] {
                Node::Text { text, .. } => out.push_str(text),
                Node::Element { children, .. } => stack.extend(children.iter().rev()),
            }
        }
        out
    }

    fn push(&mut self, node: Node) -> NodeId {
        let id = self.nodes.len();
        let parent = match &node {
            Node::Element { parent, .. } | Node::Text { parent, .. } => *parent,
        };
        self.nodes.push(node);
        if let Some(parent) = parent
            && let Node::Element { children, .. } = &mut self.nodes[parent]
        {
            children.push(id);
        }
        id
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

const RAW_TEXT: &[&str] = &["script", "style", "textarea", "title", "xmp"];

const CLOSES_P: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "div",
    "dl",
    "fieldset",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "table",
    "ul",
    "figcaption",
    "details",
];

#[must_use]
pub fn parse(source: &str) -> Dom {
    let mut dom = Dom {
        nodes: vec![Node::Element {
            tag: "#root".into(),
            attrs: Vec::new(),
            children: Vec::new(),
            parent: None,
        }],
    };
    let mut open: Vec<NodeId> = vec![0];
    let bytes = source.as_bytes();
    let mut at = 0;
    let mut text_start = 0;
    while at < bytes.len() {
        if bytes[at] != b'<' {
            at += 1;
            continue;
        }
        let rest = &source[at..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            flush_text(&mut dom, &open, &source[text_start..at]);
            let end = comment.find("-->").map_or(source.len(), |e| at + 4 + e + 3);
            at = end;
            text_start = at;
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            flush_text(&mut dom, &open, &source[text_start..at]);
            let end = rest.find("]]>").map_or(source.len(), |e| at + e);
            push_text(&mut dom, &open, &source[at + 9..end.max(at + 9)]);
            at = (end + 3).min(source.len());
            text_start = at;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            flush_text(&mut dom, &open, &source[text_start..at]);
            at = rest.find('>').map_or(source.len(), |e| at + e + 1);
            text_start = at;
            continue;
        }
        let closing = rest.starts_with("</");
        let name_from = at + if closing { 2 } else { 1 };
        let name_len = source[name_from..]
            .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
            .unwrap_or(source.len() - name_from);
        let name = &source[name_from..name_from + name_len];
        if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            at += 1;
            continue;
        }
        flush_text(&mut dom, &open, &source[text_start..at]);
        let tag = name.to_ascii_lowercase();
        let (attrs, self_closing, end) = read_attributes(source, name_from + name_len);
        at = end;
        text_start = at;
        if closing {
            close(&dom, &mut open, &tag);
            continue;
        }
        implied_ends(&dom, &mut open, &tag);
        let parent = *open.last().unwrap_or(&0);
        let id = dom.push(Node::Element {
            tag: tag.clone(),
            attrs,
            children: Vec::new(),
            parent: Some(parent),
        });
        if VOID.contains(&tag.as_str()) || self_closing {
            continue;
        }
        if RAW_TEXT.contains(&tag.as_str()) {
            let close_tag = format!("</{tag}");
            let lower = source[at..].to_ascii_lowercase();
            let end = lower.find(&close_tag).map_or(source.len(), |e| at + e);
            let raw = &source[at..end];
            if !raw.is_empty() {
                let text = if tag == "textarea" || tag == "title" {
                    decode_entities(raw)
                } else {
                    raw.to_owned()
                };
                dom.push(Node::Text {
                    text,
                    parent: Some(id),
                });
            }
            at = source[end..]
                .find('>')
                .map_or(source.len(), |e| end + e + 1);
            text_start = at;
            continue;
        }
        open.push(id);
    }
    flush_text(&mut dom, &open, &source[text_start..]);
    dom
}

fn flush_text(dom: &mut Dom, open: &[NodeId], raw: &str) {
    if raw.is_empty() {
        return;
    }
    push_text(dom, open, &decode_entities(raw));
}

fn push_text(dom: &mut Dom, open: &[NodeId], text: &str) {
    let parent = *open.last().unwrap_or(&0);
    if let Some(&last) = dom.children(parent).last()
        && let Node::Text { text: before, .. } = &mut dom.nodes[last]
    {
        before.push_str(text);
        return;
    }
    dom.push(Node::Text {
        text: text.to_owned(),
        parent: Some(parent),
    });
}

fn read_attributes(source: &str, from: usize) -> (Vec<(String, String)>, bool, usize) {
    let bytes = source.as_bytes();
    let mut attrs = Vec::new();
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
        let name_start = at;
        while at < bytes.len()
            && !bytes[at].is_ascii_whitespace()
            && !matches!(bytes[at], b'=' | b'>')
            && !(bytes[at] == b'/' && bytes.get(at + 1) == Some(&b'>'))
        {
            at += 1;
        }
        let name = source[name_start..at].to_ascii_lowercase();
        while at < bytes.len() && bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        let mut value = String::new();
        if at < bytes.len() && bytes[at] == b'=' {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if at < bytes.len() && (bytes[at] == b'"' || bytes[at] == b'\'') {
                let quote = bytes[at];
                let start = at + 1;
                let end = source[start..]
                    .find(quote as char)
                    .map_or(source.len(), |e| start + e);
                value = decode_entities(&source[start..end]);
                at = (end + 1).min(source.len());
            } else {
                let start = at;
                while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
                    at += 1;
                }
                value = decode_entities(&source[start..at]);
            }
        }
        if !name.is_empty() && !attrs.iter().any(|(key, _)| *key == name) {
            attrs.push((name, value));
        }
        if at == name_start {
            at += 1;
        }
    }
}

fn close(dom: &Dom, open: &mut Vec<NodeId>, tag: &str) {
    if let Some(position) = open.iter().rposition(|id| dom.tag(*id) == Some(tag))
        && position > 0
    {
        open.truncate(position);
    }
}

fn implied_ends(dom: &Dom, open: &mut Vec<NodeId>, tag: &str) {
    let innermost = |open: &Vec<NodeId>| open.last().and_then(|id| dom.tag(*id).map(str::to_owned));
    let close_up_to = |open: &mut Vec<NodeId>, names: &[&str], stop: &[&str]| {
        for position in (1..open.len()).rev() {
            let Some(name) = dom.tag(open[position]) else {
                continue;
            };
            if stop.contains(&name) {
                return;
            }
            if names.contains(&name) {
                open.truncate(position);
                return;
            }
        }
    };
    if CLOSES_P.contains(&tag) && open.iter().any(|id| dom.tag(*id) == Some("p")) {
        close_up_to(
            open,
            &["p"],
            &["table", "td", "th", "li", "div", "blockquote"],
        );
    }
    match tag {
        "li" => close_up_to(open, &["li"], &["ul", "ol", "menu"]),
        "dt" | "dd" => close_up_to(open, &["dt", "dd"], &["dl"]),
        "tr" => close_up_to(open, &["tr"], &["table"]),
        "td" | "th" => close_up_to(open, &["td", "th"], &["tr", "table"]),
        "thead" | "tbody" | "tfoot" => close_up_to(open, &["thead", "tbody", "tfoot"], &["table"]),
        "option" => close_up_to(open, &["option"], &["select"]),
        _ => {}
    }
    if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
        && let Some(inner) = innermost(open)
        && matches!(inner.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
    {
        open.pop();
    }
}

#[must_use]
pub fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let names = entity_names();
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find('&') {
        out.push_str(&rest[..position]);
        rest = &rest[position..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(rest.len(), |e| e + 1);
        let body = &rest[1..end];
        let with_semicolon = rest[end..].starts_with(';');
        let decoded = if let Some(number) = body.strip_prefix('#') {
            let value = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                number.parse::<u32>().ok()
            };
            value.map(|v| char::from_u32(v).unwrap_or('\u{FFFD}'))
        } else {
            names.get(body).copied()
        };
        match decoded {
            Some(c) if !body.is_empty() => {
                out.push(c);
                rest = &rest[end + usize::from(with_semicolon)..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity_names() -> HashMap<&'static str, char> {
    [
        ("amp", '&'),
        ("lt", '<'),
        ("gt", '>'),
        ("quot", '"'),
        ("apos", '\''),
        ("nbsp", '\u{A0}'),
        ("copy", '©'),
        ("reg", '®'),
        ("trade", '™'),
        ("hellip", '…'),
        ("mdash", '—'),
        ("ndash", '–'),
        ("lsquo", '‘'),
        ("rsquo", '’'),
        ("ldquo", '“'),
        ("rdquo", '”'),
        ("laquo", '«'),
        ("raquo", '»'),
        ("bull", '•'),
        ("middot", '·'),
        ("deg", '°'),
        ("plusmn", '±'),
        ("times", '×'),
        ("divide", '÷'),
        ("euro", '€'),
        ("pound", '£'),
        ("yen", '¥'),
        ("cent", '¢'),
        ("sect", '§'),
        ("para", '¶'),
        ("frac12", '½'),
        ("frac14", '¼'),
        ("frac34", '¾'),
        ("sup2", '²'),
        ("sup3", '³'),
        ("larr", '←'),
        ("rarr", '→'),
        ("uarr", '↑'),
        ("darr", '↓'),
        ("harr", '↔'),
        ("le", '≤'),
        ("ge", '≥'),
        ("ne", '≠'),
        ("infin", '∞'),
        ("shy", '\u{AD}'),
        ("zwj", '\u{200D}'),
        ("zwnj", '\u{200C}'),
        ("ensp", '\u{2002}'),
        ("emsp", '\u{2003}'),
        ("thinsp", '\u{2009}'),
        ("eacute", 'é'),
        ("egrave", 'è'),
        ("ecirc", 'ê'),
        ("aacute", 'á'),
        ("agrave", 'à'),
        ("acirc", 'â'),
        ("auml", 'ä'),
        ("ouml", 'ö'),
        ("uuml", 'ü'),
        ("Auml", 'Ä'),
        ("Ouml", 'Ö'),
        ("Uuml", 'Ü'),
        ("szlig", 'ß'),
        ("ccedil", 'ç'),
        ("ntilde", 'ñ'),
        ("oacute", 'ó'),
        ("iacute", 'í'),
        ("uacute", 'ú'),
        ("check", '✓'),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_close_each_other_and_lists_close_items() {
        let dom = parse("<p>one<p>two<ul><li>a<li>b</ul>");
        let body: Vec<_> = dom.children(0).to_vec();
        assert_eq!(body.len(), 3);
        assert_eq!(dom.text_of(body[1]), "two");
        let items = dom.find_all("li");
        assert_eq!(items.len(), 2);
        assert_eq!(dom.text_of(items[1]), "b");
    }

    #[test]
    fn entities_and_lao_text_survive() {
        let dom = parse("<p class=x>ສະບາຍດີ &amp; &#x0E81;&nbsp;ok</p>");
        let p = dom.find_all("p")[0];
        assert_eq!(dom.attr(p, "class"), Some("x"));
        assert_eq!(dom.text_of(p), "ສະບາຍດີ & ກ\u{a0}ok");
    }

    #[test]
    fn style_is_raw_text_and_cells_close() {
        let dom = parse("<style>p > a { x: 1 }</style><table><tr><td>1<td>2<tr><td>3</table>");
        assert_eq!(dom.text_of(dom.find_all("style")[0]), "p > a { x: 1 }");
        assert_eq!(dom.find_all("tr").len(), 2);
        assert_eq!(dom.find_all("td").len(), 3);
    }
}
