use convert_layout::model::{
    Align, Block, Border, Cell, Direction, Document, Group, ImageData, Inline, LineHeight, Marker,
    PageSetup, Paragraph, Row, Section, Sides, Table, TableWidth, TextStyle, VMerge,
};

use crate::css::{self, Sheet};
use crate::html::{Dom, Node, NodeId};
use crate::style::{Computed, Display, Length, PX, parse_colour, parse_length};

pub const USER_AGENT: &str = r"
head, script, style, title, meta, link, template, noscript, base, datalist, select, option, button, input, textarea, iframe, object, embed, canvas, video, audio, map { display: none }
html, body, div, p, h1, h2, h3, h4, h5, h6, ul, ol, dl, dt, dd, blockquote, pre, hr, address,
article, aside, footer, header, main, nav, section, figure, figcaption, form, fieldset, legend, center, details, summary, menu { display: block }
li { display: list-item }
table { display: table; border-collapse: separate }
tr { display: table-row }
td, th { display: table-cell; padding: 1px; vertical-align: middle }
thead, tbody, tfoot { display: table-row-group }
caption { display: table-caption; text-align: center }
body { margin: 8px }
p { margin: 1em 0 }
h1 { font-size: 2em; margin: 0.67em 0; font-weight: bold }
h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: bold }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: bold }
h4 { margin: 1.33em 0; font-weight: bold }
h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: bold }
h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: bold }
ul, ol, menu { margin: 1em 0; padding-left: 40px }
ul { list-style-type: disc }
ol { list-style-type: decimal }
ul ul { list-style-type: circle; margin: 0 }
ul ul ul { list-style-type: square }
ol ol, ol ul, ul ol { margin: 0 }
dl { margin: 1em 0 }
dd { margin-left: 40px }
blockquote, figure { margin: 1em 40px }
pre { white-space: pre; margin: 1em 0 }
pre, code, kbd, samp, tt { font-family: monospace }
b, strong, th, dt { font-weight: bold }
i, em, cite, var, dfn, address { font-style: italic }
u, ins { text-decoration: underline }
s, strike, del { text-decoration: line-through }
a[href] { color: #0000ee; text-decoration: underline }
sup { vertical-align: super; font-size: smaller }
sub { vertical-align: sub; font-size: smaller }
small { font-size: smaller }
big { font-size: larger }
mark { background-color: yellow }
center { text-align: center }
th { text-align: center }
hr { margin: 0.5em 0; border-top: 1px solid gray }
fieldset { margin: 0 2px; padding: 0.35em 0.75em 0.625em; border: 2px solid #c0c0c0 }
";

pub type Assets<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

struct Builder<'a> {
    dom: &'a Dom,
    sheets: Vec<Sheet>,
    assets: Assets<'a>,
    images: Vec<ImageData>,
    column: f32,
    notes: Vec<String>,
    page_name: String,
    page_css: String,
}

#[derive(Default)]
struct Pending {
    inlines: Vec<Inline>,
    space_owed: bool,
}

#[derive(Clone, Copy, Default)]
struct Place {
    left: f32,
    right: f32,
}

#[must_use]
pub fn build(dom: &Dom, assets: Assets<'_>) -> (Document, Vec<String>) {
    let mut sheets = vec![css::parse_sheet(USER_AGENT)];
    let mut page_css = String::new();
    for link in dom.find_all("link") {
        let rel = dom.attr(link, "rel").unwrap_or("").to_ascii_lowercase();
        if rel.contains("stylesheet")
            && let Some(href) = dom.attr(link, "href")
            && let Some(bytes) = assets(href)
        {
            let text = String::from_utf8_lossy(&bytes);
            page_css.push_str(&text);
            page_css.push('\n');
            sheets.push(css::parse_sheet(&text));
        }
    }
    for style in dom.find_all("style") {
        if dom_inside_svg(dom, style) {
            continue;
        }
        let media = dom
            .attr(style, "media")
            .unwrap_or("all")
            .to_ascii_lowercase();
        if media.contains("print") || media.contains("all") || media.contains("screen") {
            let text = dom.text_of(style);
            page_css.push_str(&text);
            page_css.push('\n');
            sheets.push(css::parse_sheet(&text));
        }
    }
    let setup = page_setup(&sheets, "");
    let mut builder = Builder {
        dom,
        sheets,
        assets,
        images: Vec::new(),
        column: setup.width - setup.margin.left - setup.margin.right,
        notes: Vec::new(),
        page_name: String::new(),
        page_css,
    };
    let root = Computed::root();
    let mut blocks = Vec::new();
    builder.children(0, &root, Place::default(), &mut blocks);
    let mut sections = vec![Section::new(setup)];
    for block in blocks {
        match block {
            Block::SectionBreak(next) => {
                if sections.last().is_some_and(|s| s.blocks.is_empty()) {
                    if let Some(last) = sections.last_mut() {
                        last.setup = next;
                    }
                } else {
                    sections.push(Section::new(next));
                }
            }
            other => {
                if let Some(last) = sections.last_mut() {
                    last.blocks.push(other);
                }
            }
        }
    }
    let title = dom
        .find_all("title")
        .first()
        .map(|t| dom.text_of(*t).trim().to_owned())
        .filter(|t| !t.is_empty());
    let lang = dom
        .find_all("html")
        .first()
        .and_then(|h| dom.attr(*h, "lang"))
        .map(str::to_owned);
    (
        Document {
            sections,
            images: builder.images,
            title,
            lang,
            collapse_margins: true,
            notes: Vec::new(),
        },
        builder.notes,
    )
}

fn dom_inside_svg(dom: &Dom, node: NodeId) -> bool {
    let mut at = dom.parent(node);
    while let Some(id) = at {
        if dom.tag(id) == Some("svg") {
            return true;
        }
        at = dom.parent(id);
    }
    false
}

fn svg_source(dom: &Dom, node: NodeId, style: &Computed, page_css: &str) -> String {
    fn write(dom: &Dom, node: NodeId, out: &mut String, depth: usize) {
        match &dom.nodes[node] {
            Node::Text { text, .. } => {
                for c in text.chars() {
                    match c {
                        '&' => out.push_str("&amp;"),
                        '<' => out.push_str("&lt;"),
                        '>' => out.push_str("&gt;"),
                        c => out.push(c),
                    }
                }
            }
            Node::Element {
                tag,
                attrs,
                children,
                ..
            } => {
                if depth > 200 {
                    return;
                }
                out.push('<');
                out.push_str(tag);
                for (k, v) in attrs {
                    out.push(' ');
                    out.push_str(k);
                    out.push_str("=\"");
                    for c in v.chars() {
                        match c {
                            '&' => out.push_str("&amp;"),
                            '<' => out.push_str("&lt;"),
                            '"' => out.push_str("&quot;"),
                            c => out.push(c),
                        }
                    }
                    out.push('"');
                }
                out.push('>');
                for &child in children {
                    write(dom, child, out, depth + 1);
                }
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
    }
    let mut out = String::from("<svg");
    let has = |name: &str| dom.attr(node, name).is_some();
    let [r, g, b] = style.colour;
    if !has("color") {
        out.push_str(&format!(" color=\"#{r:02x}{g:02x}{b:02x}\""));
    }
    if !has("font-family") {
        out.push_str(" font-family=\"");
        out.push_str(&style.family.replace('"', "'"));
        out.push('"');
    }
    if !has("font-size") {
        out.push_str(&format!(" font-size=\"{}px\"", style.size / PX));
    }
    if style.bold && !has("font-weight") {
        out.push_str(" font-weight=\"bold\"");
    }
    if style.italic && !has("font-style") {
        out.push_str(" font-style=\"italic\"");
    }
    let mut inner = String::new();
    write(dom, node, &mut inner, 0);
    let body = inner.strip_prefix("<svg").unwrap_or(&inner);
    let close = body.find('>').unwrap_or(0);
    out.push_str(&body[..close]);
    out.push('>');
    if !page_css.trim().is_empty() {
        out.push_str("<style>");
        out.push_str(&page_css.replace('<', " "));
        out.push_str("</style>");
    }
    let body = &body[close + 1..];
    let closing = body.rfind("</svg>").unwrap_or(body.len());
    out.push_str(&body[..closing]);
    let mut have: Vec<String> = Vec::new();
    collect_ids(dom, node, &mut have);
    let mut wanted = references(&inner);
    let mut defs = String::new();
    for _ in 0..3 {
        let mut next = Vec::new();
        for id in wanted {
            if have.contains(&id) {
                continue;
            }
            if let Some(found) = find_svg_id(dom, &id) {
                let mut text = String::new();
                write(dom, found, &mut text, 0);
                next.extend(references(&text));
                defs.push_str(&text);
                collect_ids(dom, found, &mut have);
            }
            have.push(id);
        }
        wanted = next;
    }
    if !defs.is_empty() {
        out.push_str("<defs>");
        out.push_str(&defs);
        out.push_str("</defs>");
    }
    out.push_str("</svg>");
    out
}

fn references(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("#") {
        let before = &text[..at];
        if !(before.ends_with("href=\"")
            || before.ends_with("url(")
            || before.ends_with("url('")
            || before.ends_with("url(\""))
        {
            continue;
        }
        let id: String = text[at + 1..]
            .chars()
            .take_while(|c| !matches!(c, '"' | '\'' | ')' | ' '))
            .collect();
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

fn collect_ids(dom: &Dom, node: NodeId, out: &mut Vec<String>) {
    let mut stack = vec![node];
    while let Some(at) = stack.pop() {
        if let Some(id) = dom.attr(at, "id") {
            out.push(id.to_owned());
        }
        stack.extend(dom.children(at));
    }
}

fn find_svg_id(dom: &Dom, id: &str) -> Option<NodeId> {
    (0..dom.nodes.len()).find(|&n| dom.attr(n, "id") == Some(id) && dom_inside_svg(dom, n))
}

fn page_setup(sheets: &[Sheet], name: &str) -> PageSetup {
    let mut setup = PageSetup::a4();
    setup.margin = Sides::all(0.4 * 72.0);
    let of = |wanted: &str| {
        sheets
            .iter()
            .flat_map(|sheet| sheet.pages.iter())
            .filter(move |(n, _)| n == wanted)
            .flat_map(|(_, d)| d.iter())
            .collect::<Vec<_>>()
    };
    let mut rules = of("");
    if !name.is_empty() {
        rules.extend(of(name));
    }
    for d in rules {
        {
            let value = d.value.to_ascii_lowercase();
            match d.property.as_str() {
                "size" => {
                    let words: Vec<&str> = value.split_whitespace().collect();
                    let landscape = words.contains(&"landscape");
                    let lengths: Vec<f32> =
                        words.iter().filter_map(|w| parse_length(w, 12.0)).collect();
                    if lengths.len() >= 2 {
                        setup.width = lengths[0];
                        setup.height = lengths[1];
                    } else if words.contains(&"letter") {
                        setup.width = 612.0;
                        setup.height = 792.0;
                    } else if words.contains(&"legal") {
                        setup.width = 612.0;
                        setup.height = 1008.0;
                    } else if words.contains(&"a4") {
                        setup.width = 595.276;
                        setup.height = 841.89;
                    } else if words.contains(&"a5") {
                        setup.width = 419.53;
                        setup.height = 595.28;
                    } else if words.contains(&"a3") {
                        setup.width = 841.89;
                        setup.height = 1190.55;
                    }
                    if landscape && setup.width < setup.height {
                        std::mem::swap(&mut setup.width, &mut setup.height);
                    }
                }
                "margin" => {
                    let parts: Vec<f32> = value
                        .split_whitespace()
                        .filter_map(|w| parse_length(w, 12.0))
                        .collect();
                    setup.margin = match parts.as_slice() {
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
                        [] => setup.margin,
                    };
                }
                _ => {}
            }
        }
    }
    setup
}

fn presentational(dom: &Dom, node: NodeId, tag: &str, style: &mut Computed) {
    match dom
        .attr(node, "dir")
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("rtl") => style.direction = Direction::Rtl,
        Some("ltr") => style.direction = Direction::Ltr,
        Some("auto") => style.direction = Direction::Auto,
        _ => {}
    }
    if let Some(colour) = dom.attr(node, "bgcolor").and_then(parse_colour) {
        style.background = Some(colour);
    }
    if let Some(align) = dom.attr(node, "align") {
        let align = align.to_ascii_lowercase();
        if tag != "img" && tag != "table" {
            style.align = match align.as_str() {
                "center" | "middle" => Align::Center,
                "right" => Align::Right,
                "justify" => Align::Justify,
                _ => Align::Left,
            };
            style.align_physical = matches!(align.as_str(), "left" | "right");
        }
    }
    if let Some(valign) = dom.attr(node, "valign") {
        style.valign = match valign.to_ascii_lowercase().as_str() {
            "top" => Some(convert_layout::model::VAlign::Top),
            "bottom" => Some(convert_layout::model::VAlign::Bottom),
            _ => Some(convert_layout::model::VAlign::Middle),
        };
    }
    if let Some(width) = dom.attr(node, "width") {
        style.width = attr_length(width);
    }
    if let Some(height) = dom.attr(node, "height") {
        style.height = attr_length(height);
    }
    if tag == "font" {
        if let Some(colour) = dom.attr(node, "color").and_then(parse_colour) {
            style.colour = colour;
        }
        if let Some(face) = dom.attr(node, "face") {
            style.family = face.to_owned();
        }
        if let Some(size) = dom
            .attr(node, "size")
            .and_then(|s| s.trim().parse::<i32>().ok())
        {
            style.size = match size.clamp(1, 7) {
                1 => 7.5,
                2 => 9.75,
                3 => 12.0,
                4 => 13.5,
                5 => 18.0,
                6 => 24.0,
                _ => 36.0,
            };
        }
    }
}

fn attr_length(text: &str) -> Length {
    let text = text.trim();
    if let Some(p) = text.strip_suffix('%') {
        return p.parse().map_or(Length::Auto, Length::Percent);
    }
    text.trim_end_matches("px")
        .parse::<f32>()
        .map_or(Length::Auto, |px| Length::Pt(px * PX))
}

fn text_style(style: &Computed) -> TextStyle {
    TextStyle {
        family: style.family.clone(),
        family_complex: None,
        size: style.size,
        size_complex: None,
        bold: style.bold,
        italic: style.italic,
        underline: style.underline,
        strike: style.strike,
        colour: style.colour,
        background: style.background,
        shift: style.shift,
        letter_spacing: style.letter_spacing,
        link: None,
        caps: style.upper,
    }
}

fn first_strong_rtl(inlines: &[Inline]) -> bool {
    use convert_layout::bidi::{Class, class};
    for inline in inlines {
        if let Inline::Text { text, .. } = inline {
            for c in text.chars() {
                match class(c) {
                    Class::L => return false,
                    Class::R | Class::AL => return true,
                    _ => {}
                }
            }
        }
    }
    false
}

fn line_height(style: &Computed) -> LineHeight {
    style.line_height.unwrap_or(LineHeight::Multiple(1.0))
}

impl Builder<'_> {
    fn style_of(&self, node: NodeId, parent: &Computed) -> Computed {
        let mut style = parent.inherit();
        let tag = self.dom.tag(node).unwrap_or("");
        let declarations = css::cascade(self.dom, node, &self.sheets);
        presentational(self.dom, node, tag, &mut style);
        style.apply(&declarations, parent.size);
        if let Some(lang) = self.dom.attr(node, "lang") {
            style.lang = Some(lang.to_owned());
        }
        if tag == "a" && self.dom.attr(node, "href").is_none() {
            style.underline = parent.underline;
        }
        style
    }

    fn children(&mut self, node: NodeId, style: &Computed, place: Place, out: &mut Vec<Block>) {
        let mut pending = Pending::default();
        let kids: Vec<NodeId> = self.dom.children(node).to_vec();
        for child in kids {
            match &self.dom.nodes[child] {
                Node::Text { text, .. } => {
                    let text = text.clone();
                    self.text(&text, style, None, &mut pending);
                }
                Node::Element { .. } => {
                    let child_style = self.style_of(child, style);
                    if child_style.display == Display::None {
                        continue;
                    }
                    if child_style.display == Display::Inline
                        && !(child_style.float_or_absolute && self.is_blockish(child))
                    {
                        self.inline(child, &child_style, &mut pending);
                    } else {
                        self.flush(&mut pending, style, place, out);
                        self.block(child, &child_style, place, out);
                    }
                }
            }
        }
        self.flush(&mut pending, style, place, out);
    }

    fn is_blockish(&self, node: NodeId) -> bool {
        self.dom.children(node).iter().any(|c| {
            matches!(
                self.dom.tag(*c),
                Some("div" | "p" | "table" | "ul" | "ol" | "h1" | "h2" | "h3")
            )
        })
    }

    fn flush(
        &mut self,
        pending: &mut Pending,
        style: &Computed,
        place: Place,
        out: &mut Vec<Block>,
    ) {
        let mut inlines = std::mem::take(&mut pending.inlines);
        pending.space_owed = false;
        trim_inlines(&mut inlines, style.pre);
        if inlines.is_empty() {
            return;
        }
        let rtl = match style.direction {
            Direction::Rtl => true,
            Direction::Ltr => false,
            Direction::Auto => first_strong_rtl(&inlines),
        };
        let (start, end) = if rtl {
            (place.right, place.left)
        } else {
            (place.left, place.right)
        };
        out.push(Block::Paragraph(Paragraph {
            inlines,
            align: style.paragraph_align(rtl),
            indent_left: start,
            indent_right: end,
            indent_first: 0.0,
            line_height: line_height(style),
            empty_style: text_style(style),
            direction: if rtl { Direction::Rtl } else { Direction::Ltr },
            preformatted: style.pre,
            ..Paragraph::default()
        }));
    }

    fn text(&mut self, raw: &str, style: &Computed, link: Option<&str>, pending: &mut Pending) {
        let mut ts = text_style(style);
        ts.link = link.map(str::to_owned);
        if style.pre {
            for (index, line) in raw.split('\n').enumerate() {
                if index > 0 {
                    pending.inlines.push(Inline::LineBreak);
                }
                let line = line.trim_end_matches('\r').replace('\t', "        ");
                if !line.is_empty() {
                    pending.inlines.push(Inline::Text {
                        text: line,
                        style: ts.clone(),
                    });
                }
            }
            return;
        }
        let mut text = String::with_capacity(raw.len());
        for c in raw.chars() {
            if matches!(c, ' ' | '\n' | '\t' | '\r' | '\u{c}') {
                if !pending.space_owed {
                    text.push(' ');
                    pending.space_owed = true;
                }
            } else {
                text.push(if style.nowrap && c == ' ' {
                    '\u{a0}'
                } else {
                    c
                });
                pending.space_owed = false;
            }
        }
        if text.is_empty() {
            return;
        }
        let at_start = pending
            .inlines
            .last()
            .is_none_or(|i| matches!(i, Inline::LineBreak));
        if at_start {
            text = text.trim_start_matches(' ').to_owned();
            if text.is_empty() {
                return;
            }
        }
        if let Some(Inline::Text {
            text: before,
            style,
        }) = pending.inlines.last_mut()
            && *style == ts
        {
            before.push_str(&text);
            return;
        }
        pending.inlines.push(Inline::Text { text, style: ts });
    }

    fn inline(&mut self, node: NodeId, style: &Computed, pending: &mut Pending) {
        let tag = self.dom.tag(node).unwrap_or("").to_owned();
        match tag.as_str() {
            "br" => {
                pending.inlines.push(Inline::LineBreak);
                pending.space_owed = true;
                return;
            }
            "img" => {
                self.image(node, style, pending);
                return;
            }
            "svg" => {
                self.inline_svg(node, style, pending);
                return;
            }
            "wbr" => return,
            _ => {}
        }
        let link = if tag == "a" {
            self.dom.attr(node, "href").map(str::to_owned)
        } else {
            None
        };
        let kids: Vec<NodeId> = self.dom.children(node).to_vec();
        for child in kids {
            match &self.dom.nodes[child] {
                Node::Text { text, .. } => {
                    let text = text.clone();
                    let link = link.clone().or_else(|| self.link_above(node));
                    self.text(&text, style, link.as_deref(), pending);
                }
                Node::Element { .. } => {
                    let child_style = self.style_of(child, style);
                    if child_style.display != Display::None {
                        self.inline(child, &child_style, pending);
                    }
                }
            }
        }
    }

    fn link_above(&self, node: NodeId) -> Option<String> {
        let mut at = Some(node);
        while let Some(id) = at {
            if self.dom.tag(id) == Some("a") {
                return self.dom.attr(id, "href").map(str::to_owned);
            }
            at = self.dom.parent(id);
        }
        None
    }

    fn image(&mut self, node: NodeId, style: &Computed, pending: &mut Pending) {
        let Some(src) = self.dom.attr(node, "src") else {
            return;
        };
        let bytes = if let Some(data) = src.strip_prefix("data:") {
            data.split_once(";base64,").and_then(|(_, b64)| base64(b64))
        } else if src.starts_with("http:") || src.starts_with("https:") || src.starts_with("//") {
            self.notes.push(format!(
                "picture not fetched (network is never used): {src}"
            ));
            None
        } else {
            (self.assets)(src)
        };
        let Some(bytes) = bytes else {
            if let Some(alt) = self.dom.attr(node, "alt").filter(|a| !a.is_empty()) {
                let alt = format!("[{alt}]");
                self.text(&alt, style, None, pending);
            }
            if !src.starts_with("http") {
                self.notes.push(format!("picture not found: {src}"));
            }
            return;
        };
        let bytes = if convert_svg::is_svg(&bytes) {
            let folder = src.rsplit_once('/').map_or("", |(dir, _)| dir).to_owned();
            let assets = self.assets;
            convert_svg::embed_files(&bytes, &|name| {
                if folder.is_empty() {
                    assets(name)
                } else {
                    assets(&format!("{folder}/{name}")).or_else(|| assets(name))
                }
            })
        } else {
            bytes
        };
        self.place_picture(bytes, style, pending, src);
    }

    fn inline_svg(&mut self, node: NodeId, style: &Computed, pending: &mut Pending) {
        let source = svg_source(self.dom, node, style, &self.page_css);
        let mut sized = style.clone();
        let absolute = |name: &str| {
            self.dom
                .attr(node, name)
                .is_some_and(|v| !v.trim().ends_with('%') && !v.trim().is_empty())
        };
        if matches!(style.width, Length::Auto)
            && !absolute("width")
            && self.dom.attr(node, "viewbox").is_some()
        {
            sized.width = Length::Percent(100.0);
        }
        self.place_picture(source.into_bytes(), &sized, pending, "inline <svg>");
    }

    fn place_picture(
        &mut self,
        bytes: Vec<u8>,
        style: &Computed,
        pending: &mut Pending,
        src: &str,
    ) {
        let natural = if convert_svg::is_svg(&bytes) {
            match convert_svg::Drawing::parse(&bytes) {
                Ok(drawing) => {
                    if !drawing.notes().is_empty() {
                        self.notes.push(format!(
                            "{src}: drawn without {}",
                            drawing.notes().join(", ")
                        ));
                    }
                    Some((drawing.width, drawing.height))
                }
                Err(_) => None,
            }
        } else {
            #[allow(clippy::cast_precision_loss)]
            convert_layout::image_size(&bytes).map(|(w, h)| (w as f32, h as f32))
        };
        let Some((px_w, px_h)) = natural else {
            self.notes.push(format!(
                "picture not read (JPEG, PNG, GIF and SVG only): {src}"
            ));
            return;
        };
        if convert_gif::Gif::is_gif(&bytes)
            && convert_gif::Gif::read(&bytes).is_ok_and(|g| g.frames > 1)
        {
            self.notes
                .push(format!("{src}: an animation; its first frame is drawn"));
        }
        let (natural_w, natural_h) = (px_w * PX, px_h * PX);
        let w = style.width.resolve(self.column);
        let h = style.height.resolve(self.column);
        let (mut width, mut height) = match (w, h) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (w, w * natural_h / natural_w.max(0.01)),
            (None, Some(h)) => (h * natural_w / natural_h.max(0.01), h),
            (None, None) => (natural_w, natural_h),
        };
        if let Some(max) = style.max_width.resolve(self.column)
            && width > max
        {
            height *= max / width;
            width = max;
        }
        let index = self.images.len();
        self.images.push(ImageData { bytes });
        pending.inlines.push(Inline::Image {
            image: index,
            width,
            height,
        });
        pending.space_owed = false;
    }

    #[allow(clippy::too_many_lines)]
    fn block(&mut self, node: NodeId, style: &Computed, place: Place, out: &mut Vec<Block>) {
        let named = style.page.clone().filter(|p| *p != self.page_name);
        let outer_column = self.column;
        if let Some(name) = &named {
            let setup = page_setup(&self.sheets, name);
            self.column = setup.width - setup.margin.left - setup.margin.right;
            out.push(Block::SectionBreak(setup));
        } else if style.break_before {
            out.push(Block::PageBreak);
        }
        let next_page = named.clone().unwrap_or_else(|| self.page_name.clone());
        let outer_page = std::mem::replace(&mut self.page_name, next_page);
        self.block_inner(node, style, place, out);
        if named.is_some() {
            self.column = outer_column;
            self.page_name = outer_page;
            out.push(Block::SectionBreak(page_setup(
                &self.sheets,
                &self.page_name,
            )));
        }
    }

    #[allow(clippy::too_many_lines)]
    fn block_inner(&mut self, node: NodeId, style: &Computed, place: Place, out: &mut Vec<Block>) {
        let tag = self.dom.tag(node).unwrap_or("").to_owned();
        let margin_top = style.margin.top.resolve(self.column).unwrap_or(0.0);
        let margin_bottom = style.margin.bottom.resolve(self.column).unwrap_or(0.0);
        let margin_left = style.margin.left.resolve(self.column).unwrap_or(0.0);
        let margin_right = style.margin.right.resolve(self.column).unwrap_or(0.0);
        if tag == "hr" {
            let border = style.border.top.or(style.border.bottom);
            out.push(Block::Rule {
                width: border.map_or(0.75, |b| b.width),
                colour: border.map_or([128, 128, 128], |b| b.colour),
                space_before: margin_top,
                space_after: margin_bottom,
            });
            return;
        }
        if tag == "img" || tag == "svg" {
            let mut pending = Pending::default();
            if tag == "img" {
                self.image(node, style, &mut pending);
            } else {
                self.inline_svg(node, style, &mut pending);
            }
            self.flush(&mut pending, style, place, out);
            return;
        }
        match style.display {
            Display::Table => {
                self.table(node, style, place, out);
                if style.break_after {
                    out.push(Block::PageBreak);
                }
                return;
            }
            Display::TableRow | Display::TableRowGroup | Display::TableCell => {
                self.children(node, style, place, out);
                return;
            }
            _ => {}
        }
        let boxed = style.background.is_some()
            || style.border.top.is_some()
            || style.border.bottom.is_some()
            || style.border.left.is_some()
            || style.border.right.is_some();
        let mut inner_place = place;
        let mut inner = Vec::new();
        if boxed {
            inner_place = Place::default();
        } else {
            inner_place.left += margin_left + style.padding.left;
            inner_place.right += margin_right + style.padding.right;
        }
        if style.display == Display::ListItem {
            self.list_item(node, style, inner_place, &mut inner);
        } else {
            self.children(node, style, inner_place, &mut inner);
        }
        if style.columns > 1 && !inner.is_empty() {
            inner = vec![Block::Columns {
                count: style.columns,
                gap: style.column_gap,
                blocks: inner,
            }];
        }
        if inner.is_empty()
            && let Some(h) = style.height.resolve(0.0)
            && h > 0.0
        {
            inner.push(Block::Paragraph(Paragraph {
                line_height: LineHeight::Exact(h),
                empty_style: text_style(style),
                indent_left: inner_place.left,
                ..Paragraph::default()
            }));
        }
        if boxed {
            out.push(Block::Group(Group {
                blocks: inner,
                margin_left: place.left + margin_left,
                margin_right: place.right + margin_right,
                padding: style.padding,
                background: style.background,
                borders: style.border,
                space_before: margin_top,
                space_after: margin_bottom,
            }));
        } else {
            if !inner.is_empty() {
                add_space(
                    &mut inner,
                    margin_top + style.padding.top,
                    margin_bottom + style.padding.bottom,
                );
            } else if margin_top + margin_bottom > 0.0 && tag != "div" {
            }
            if style.indent != 0.0
                && let Some(Block::Paragraph(first)) = inner.first_mut()
            {
                first.indent_first = style.indent;
            }
            out.extend(inner);
        }
        if style.break_after {
            out.push(Block::PageBreak);
        }
    }

    fn list_item(&mut self, node: NodeId, style: &Computed, place: Place, out: &mut Vec<Block>) {
        let marker_text = self.marker_for(node, style);
        let start = out.len();
        self.children(node, style, place, out);
        if out.len() == start {
            out.push(Block::Paragraph(Paragraph {
                indent_left: place.left,
                empty_style: text_style(style),
                ..Paragraph::default()
            }));
        }
        if let Some(text) = marker_text
            && let Some(Block::Paragraph(first)) = out.get_mut(start)
        {
            let mut ms = text_style(style);
            ms.underline = false;
            ms.strike = false;
            ms.link = None;
            first.marker = Some(Marker {
                text,
                style: ms,
                outside: true,
            });
        }
    }

    fn marker_for(&self, node: NodeId, style: &Computed) -> Option<String> {
        let kind = style.list_style.as_str();
        match kind {
            "none" => None,
            "disc" => Some("•".into()),
            "circle" => Some("◦".into()),
            "square" => Some("▪".into()),
            _ => {
                let parent = self.dom.parent(node)?;
                let mut number: i64 = self
                    .dom
                    .attr(parent, "start")
                    .and_then(|s| s.trim().parse().ok())
                    .unwrap_or(1);
                for sibling in self.dom.children(parent) {
                    if *sibling == node {
                        break;
                    }
                    if self.dom.tag(*sibling) == Some("li") {
                        number += 1;
                    }
                }
                if let Some(value) = self
                    .dom
                    .attr(node, "value")
                    .and_then(|v| v.trim().parse().ok())
                {
                    number = value;
                }
                let kind = self
                    .dom
                    .attr(parent, "type")
                    .map(|t| match t {
                        "a" => "lower-alpha",
                        "A" => "upper-alpha",
                        "i" => "lower-roman",
                        "I" => "upper-roman",
                        _ => "decimal",
                    })
                    .filter(|_| kind == "decimal")
                    .unwrap_or(kind);
                Some(format!(
                    "{}.",
                    convert_layout::numbering::format(number, kind)
                ))
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn table(&mut self, node: NodeId, style: &Computed, place: Place, out: &mut Vec<Block>) {
        let dom = self.dom;
        let border_attr = dom
            .attr(node, "border")
            .and_then(|b| {
                if b.is_empty() {
                    Some(1.0)
                } else {
                    b.trim().parse::<f32>().ok()
                }
            })
            .unwrap_or(0.0);
        let padding_attr = dom
            .attr(node, "cellpadding")
            .and_then(|p| p.trim().parse::<f32>().ok());
        let mut rows: Vec<Row> = Vec::new();
        let mut caption: Vec<Block> = Vec::new();
        let mut row_nodes: Vec<(NodeId, Computed, bool)> = Vec::new();
        let mut foot: Vec<(NodeId, Computed, bool)> = Vec::new();
        for child in dom.children(node).to_vec() {
            if dom.tag(child).is_none() {
                continue;
            }
            let child_style = self.style_of(child, style);
            match child_style.display {
                Display::TableRow => row_nodes.push((child, child_style, false)),
                Display::TableRowGroup => {
                    let is_head = dom.tag(child) == Some("thead");
                    let is_foot = dom.tag(child) == Some("tfoot");
                    for row in dom.children(child).to_vec() {
                        if dom.tag(row).is_none() {
                            continue;
                        }
                        let row_style = self.style_of(row, &child_style);
                        if row_style.display == Display::TableRow {
                            if is_foot {
                                foot.push((row, row_style, false));
                            } else {
                                row_nodes.push((row, row_style, is_head));
                            }
                        }
                    }
                }
                Display::TableCaption => {
                    self.children(child, &child_style, Place::default(), &mut caption);
                }
                _ => {}
            }
        }
        row_nodes.extend(foot);
        let cell_border = (border_attr > 0.0).then_some(Border {
            width: 0.75,
            colour: [128, 128, 128],
        });
        for (row_node, row_style, header) in row_nodes {
            let mut cells = Vec::new();
            for cell_node in dom.children(row_node).to_vec() {
                if dom.tag(cell_node).is_none() {
                    continue;
                }
                let cell_style = self.style_of(cell_node, &row_style);
                if cell_style.display != Display::TableCell {
                    continue;
                }
                let mut blocks = Vec::new();
                self.children(cell_node, &cell_style, Place::default(), &mut blocks);
                trim_cell_margins(&mut blocks);
                let span = dom
                    .attr(cell_node, "colspan")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 1000);
                let row_span = dom
                    .attr(cell_node, "rowspan")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .clamp(1, 1000);
                let mut borders = cell_style.border;
                if cell_border.is_some() {
                    for side in [
                        &mut borders.top,
                        &mut borders.right,
                        &mut borders.bottom,
                        &mut borders.left,
                    ] {
                        if side.is_none() {
                            *side = cell_border;
                        }
                    }
                }
                let padding = padding_attr.map_or(cell_style.padding, |p| Sides::all(p * PX));
                cells.push(Cell {
                    blocks,
                    span,
                    row_span,
                    vmerge: VMerge::None,
                    background: cell_style.background.or(row_style.background),
                    borders,
                    padding: Some(padding),
                    valign: cell_style
                        .valign
                        .unwrap_or(convert_layout::model::VAlign::Middle),
                    width: cell_style.width.resolve(self.column),
                });
            }
            rows.push(Row {
                cells,
                min_height: row_style.height.resolve(0.0).unwrap_or(0.0),
                exact_height: None,
                header,
            });
        }
        let margin_top = style.margin.top.resolve(self.column).unwrap_or(0.0);
        let margin_bottom = style.margin.bottom.resolve(self.column).unwrap_or(0.0);
        let width = match style.width {
            Length::Pt(w) => TableWidth::Fixed(w),
            Length::Percent(p) => TableWidth::Percent(p / 100.0),
            Length::Auto => TableWidth::Auto,
        };
        let centred = dom
            .attr(node, "align")
            .is_some_and(|a| a.eq_ignore_ascii_case("center"))
            || (matches!(style.margin.left, Length::Auto)
                && matches!(style.margin.right, Length::Auto));
        if !caption.is_empty() {
            out.extend(caption);
        }
        let table_border = style.border.top;
        out.push(Block::Table(Table {
            columns: Vec::new(),
            width,
            rows,
            indent: place.left + style.margin.left.resolve(self.column).unwrap_or(0.0),
            align: if centred { Align::Center } else { Align::Left },
            space_before: margin_top,
            space_after: margin_bottom,
            padding: Sides::all(0.75),
            outer_border: table_border,
            collapse: style.border_collapse,
        }));
    }
}

fn add_space(blocks: &mut [Block], before: f32, after: f32) {
    if let Some(first) = blocks.first_mut() {
        match first {
            Block::Paragraph(p) => p.space_before = p.space_before.max(before),
            Block::Table(t) => t.space_before = t.space_before.max(before),
            Block::Group(g) => g.space_before = g.space_before.max(before),
            Block::Rule { space_before, .. } => *space_before = space_before.max(before),
            Block::PageBreak | Block::SectionBreak(_) | Block::Columns { .. } => {}
        }
    }
    if let Some(last) = blocks.last_mut() {
        match last {
            Block::Paragraph(p) => p.space_after = p.space_after.max(after),
            Block::Table(t) => t.space_after = t.space_after.max(after),
            Block::Group(g) => g.space_after = g.space_after.max(after),
            Block::Rule { space_after, .. } => *space_after = space_after.max(after),
            Block::PageBreak | Block::SectionBreak(_) | Block::Columns { .. } => {}
        }
    }
}

fn trim_cell_margins(blocks: &mut [Block]) {
    if let Some(Block::Paragraph(p)) = blocks.first_mut() {
        p.space_before = 0.0;
    }
    if let Some(Block::Paragraph(p)) = blocks.last_mut() {
        p.space_after = 0.0;
    }
}

fn trim_inlines(inlines: &mut Vec<Inline>, pre: bool) {
    if pre {
        return;
    }
    while let Some(Inline::Text { text, .. }) = inlines.last_mut() {
        let trimmed = text.trim_end_matches(' ').len();
        text.truncate(trimmed);
        if text.is_empty() {
            inlines.pop();
        } else {
            break;
        }
    }
    let mut after_break = true;
    inlines.retain_mut(|inline| match inline {
        Inline::Text { text, .. } => {
            if after_break {
                let t = text.trim_start_matches(' ').to_owned();
                *text = t;
            }
            after_break = false;
            !text.is_empty()
        }
        Inline::LineBreak => {
            after_break = true;
            true
        }
        _ => {
            after_break = false;
            true
        }
    });
    if inlines.iter().all(|i| matches!(i, Inline::LineBreak)) {
        inlines.clear();
    }
}

#[must_use]
pub fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0_u32;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xFF).ok()?);
        }
    }
    Some(out)
}
