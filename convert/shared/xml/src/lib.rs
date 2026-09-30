use std::fmt::Write as _;

#[must_use]
pub fn allowed(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

#[must_use]
pub fn escape(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let dropped = escape_into(&mut out, text);
    (out, dropped)
}

pub fn escape_into(out: &mut String, text: &str) -> usize {
    let mut dropped = 0;
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c if allowed(c) => out.push(c),
            _ => dropped += 1,
        }
    }
    dropped
}

#[derive(Debug, Default)]
pub struct Writer {
    out: String,
    open: Vec<&'static str>,
    dropped: usize,
}

impl Writer {
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(4096)
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        let mut out = String::with_capacity(capacity);
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        Self {
            out,
            open: Vec::new(),
            dropped: 0,
        }
    }

    #[must_use]
    pub fn fragment() -> Self {
        Self::default()
    }

    fn start_tag(&mut self, name: &str, attributes: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        for (key, value) in attributes {
            self.out.push(' ');
            self.out.push_str(key);
            self.out.push_str("=\"");
            self.dropped += escape_into(&mut self.out, value);
            self.out.push('"');
        }
    }

    pub fn open(&mut self, name: &'static str, attributes: &[(&str, &str)]) -> &mut Self {
        self.start_tag(name, attributes);
        self.out.push('>');
        self.open.push(name);
        self
    }

    pub fn empty(&mut self, name: &str, attributes: &[(&str, &str)]) -> &mut Self {
        self.start_tag(name, attributes);
        self.out.push_str("/>");
        self
    }

    pub fn leaf(&mut self, name: &str, attributes: &[(&str, &str)], text: &str) -> &mut Self {
        self.start_tag(name, attributes);
        self.out.push('>');
        self.dropped += escape_into(&mut self.out, text);
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
        self
    }

    pub fn text(&mut self, text: &str) -> &mut Self {
        self.dropped += escape_into(&mut self.out, text);
        self
    }

    pub fn raw(&mut self, markup: &str) -> &mut Self {
        self.out.push_str(markup);
        self
    }

    pub fn close(&mut self) -> &mut Self {
        if let Some(name) = self.open.pop() {
            let _ = write!(self.out, "</{name}>");
        }
        self
    }

    #[must_use]
    pub fn depth(&self) -> usize {
        self.open.len()
    }

    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    #[must_use]
    pub fn finish(mut self) -> String {
        while !self.open.is_empty() {
            self.close();
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_markup_and_quotes() {
        assert_eq!(escape("a<b>&\"c\"").0, "a&lt;b&gt;&amp;&quot;c&quot;");
    }

    #[test]
    fn drops_what_xml_forbids_and_counts_it() {
        let (text, dropped) = escape("a\u{1}b\u{FFFE}c\td\u{FFFD}");
        assert_eq!(text, "abc\td\u{FFFD}");
        assert_eq!(dropped, 2);
    }

    #[test]
    fn keeps_lao_and_thai_and_astral() {
        let text = "ພາສາລາວ ภาษาไทย 𝔸";
        assert_eq!(escape(text), (text.to_owned(), 0));
    }

    #[test]
    fn closes_what_is_open() {
        let mut writer = Writer::fragment();
        writer.open("a", &[("x", "1&2")]).open("b", &[]).text("t");
        writer.empty("c", &[]);
        assert_eq!(writer.depth(), 2);
        assert_eq!(writer.finish(), "<a x=\"1&amp;2\"><b>t<c/></b></a>");
    }
}
