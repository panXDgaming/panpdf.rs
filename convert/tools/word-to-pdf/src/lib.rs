pub mod props;

use std::collections::HashMap;

use convert_layout::model::{
    Align, Block, Cell, Direction, Document, FIELD_PAGE, FIELD_PAGES, ImageData, Inline,
    LineHeight, Marker, PageSetup, Paragraph, Row, Section, Sides, TabStop, Table, TableWidth,
    TextStyle, VAlign, VMerge, VerticalShift,
};
use convert_office_read::{Element, Package};

use props::{Borders, ParaProps, RunProps, Styles, margins, twips};

pub fn run(
    inputs: &[convert_structure::bytes_tool::Input],
    _settings: &convert_structure::bytes_tool::Settings,
    fonts: &convert_structure::bytes_tool::Fonts,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<convert_structure::bytes_tool::Files, String> {
    let provider = fonts.layout_provider();
    let mut made = convert_structure::bytes_tool::Files::default();
    let mut failures = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        if !progress(index, inputs.len()) {
            return Err("cancelled".into());
        }
        match convert(&input.bytes, provider.clone()) {
            Ok((pdf, notes)) => {
                made.files.push((pdf_name(&input.name), pdf));
                made.notes
                    .extend(notes.into_iter().map(|n| format!("{}: {n}", input.name)));
            }
            Err(why) => failures.push(format!("{}: {why}", input.name)),
        }
    }
    progress(inputs.len(), inputs.len());
    if made.files.is_empty() && !failures.is_empty() {
        return Err(failures.join("\n"));
    }
    made.notes.extend(failures);
    Ok(made)
}

#[must_use]
pub fn pdf_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = base.rsplit_once('.').map_or(base, |(stem, _)| stem);
    format!("{stem}.pdf")
}

convert_wasm::export_bytes_tool!(crate::run);

pub fn convert(
    docx: &[u8],
    fonts: Option<std::sync::Arc<dyn convert_structure::FontProvider>>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let (document, notes) = read(docx)?;
    let book = convert_layout::font_book(fonts);
    let pdf = convert_layout::render(&document, book)?;
    Ok((pdf, notes))
}

pub fn read(docx: &[u8]) -> Result<(Document, Vec<String>), String> {
    let package = Package::open(docx).map_err(|e| format!("not a Word document: {e:?}"))?;
    let main = package
        .main_part()
        .unwrap_or_else(|| "word/document.xml".to_owned());
    let root = package
        .xml(&main)
        .map_err(|e| format!("not a Word document: {e:?}"))?;
    let rels = package.rels(&main);
    let part_of = |kind: &str| {
        rels.iter()
            .find(|r| r.is(kind) && !r.external)
            .and_then(|r| package.xml(&r.target).ok())
    };
    let styles_xml = part_of("styles");
    let numbering_xml = part_of("numbering");
    let theme_xml = part_of("theme");
    let styles = Styles::read(
        styles_xml.as_ref(),
        numbering_xml.as_ref(),
        theme_xml.as_ref(),
    );
    let footnotes = part_of("footnotes");
    let mut reader = Reader {
        package: &package,
        styles: &styles,
        images: Vec::new(),
        counters: HashMap::new(),
        footnote_xml: footnotes,
        doc_notes: Vec::new(),
        notes: Vec::new(),
        image_cache: HashMap::new(),
        last_style: None,
    };
    let body = root.child("body").ok_or("the document has no body")?;
    let mut sections: Vec<Section> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut previous: Carried = (None, None, None, None);
    let mut finish_section = |reader: &mut Reader<'_>,
                              sect: Option<&Element>,
                              blocks: Vec<Block>,
                              sections: &mut Vec<Section>| {
        let mut section = reader.section(sect, &main, &mut previous);
        section.blocks = blocks;
        sections.push(section);
    };
    let children: Vec<&Element> = body.elements().collect();
    for el in children {
        match el.local() {
            "sectPr" => {}
            _ => {
                let sect = el.path(&["pPr", "sectPr"]).filter(|_| el.local() == "p");
                reader.block(
                    el,
                    &mut blocks,
                    &Context::default(),
                    &rels_map(&package, &main),
                );
                if let Some(sect) = sect {
                    let done = std::mem::take(&mut blocks);
                    finish_section(&mut reader, Some(sect), done, &mut sections);
                }
            }
        }
    }
    finish_section(&mut reader, body.child("sectPr"), blocks, &mut sections);
    let title = package
        .xml("docProps/core.xml")
        .ok()
        .and_then(|core| core.child("title").map(Element::text))
        .filter(|t| !t.trim().is_empty());
    let notes = std::mem::take(&mut reader.notes);
    Ok((
        Document {
            sections,
            images: reader.images,
            title,
            lang: None,
            collapse_margins: false,
            notes: std::mem::take(&mut reader.doc_notes),
        },
        notes,
    ))
}

fn rels_map(package: &Package<'_>, part: &str) -> HashMap<String, (String, bool)> {
    package
        .rels(part)
        .into_iter()
        .map(|r| (r.id, (r.target, r.external)))
        .collect()
}

#[derive(Clone, Default)]
struct Context {
    table_style: Option<String>,
}

struct Reader<'a> {
    package: &'a Package<'a>,
    styles: &'a Styles,
    images: Vec<ImageData>,
    image_cache: HashMap<String, usize>,
    counters: HashMap<(String, usize), i64>,
    footnote_xml: Option<Element>,
    doc_notes: Vec<Vec<Block>>,
    notes: Vec<String>,
    last_style: Option<String>,
}

type Rels = HashMap<String, (String, bool)>;

type Carried = (
    Option<Vec<Block>>,
    Option<Vec<Block>>,
    Option<Vec<Block>>,
    Option<Vec<Block>>,
);

struct Field {
    instr: String,
    result: bool,
}

impl Reader<'_> {
    fn section(&mut self, sect: Option<&Element>, main: &str, previous: &mut Carried) -> Section {
        let mut setup = PageSetup::letter();
        let Some(sect) = sect else {
            return Section::new(setup);
        };
        if let Some(size) = sect.child("pgSz") {
            if let Some(w) = size.attr("w").and_then(twips) {
                setup.width = w;
            }
            if let Some(h) = size.attr("h").and_then(twips) {
                setup.height = h;
            }
        }
        if let Some(m) = sect.child("pgMar") {
            let get = |name: &str| m.attr(name).and_then(twips);
            setup.margin = Sides {
                top: get("top").unwrap_or(72.0).abs(),
                right: get("right").unwrap_or(72.0),
                bottom: get("bottom").unwrap_or(72.0).abs(),
                left: get("left").unwrap_or(72.0),
            };
            setup.header_distance = get("header").unwrap_or(36.0);
            setup.footer_distance = get("footer").unwrap_or(36.0);
        }
        let mut section = Section::new(setup);
        if let Some(cols) = sect.child("cols") {
            section.columns = cols
                .attr("num")
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, 12);
            section.column_gap = cols.attr("space").and_then(twips).unwrap_or(36.0);
        }
        let rels = rels_map(self.package, main);
        let title_page = sect
            .child("titlePg")
            .is_some_and(|t| !matches!(t.attr("val"), Some("0" | "false")));
        let mut read_part = |kind: &str, which: &str| -> Option<Vec<Block>> {
            let reference = sect
                .children_named(kind)
                .find(|r| r.attr("type").unwrap_or("default") == which)?;
            let (target, _) = rels.get(reference.attr("id")?)?;
            let xml = self.package.xml(target).ok()?;
            let part_rels = rels_map(self.package, target);
            let mut blocks = Vec::new();
            for el in xml.elements() {
                self.block(el, &mut blocks, &Context::default(), &part_rels);
            }
            Some(blocks)
        };
        let header = read_part("headerReference", "default");
        let footer = read_part("footerReference", "default");
        let first_header = read_part("headerReference", "first");
        let first_footer = read_part("footerReference", "first");
        if header.is_some() {
            previous.0.clone_from(&header);
        }
        if footer.is_some() {
            previous.1.clone_from(&footer);
        }
        if first_header.is_some() {
            previous.2.clone_from(&first_header);
        }
        if first_footer.is_some() {
            previous.3.clone_from(&first_footer);
        }
        section.header = previous.0.clone().unwrap_or_default();
        section.footer = previous.1.clone().unwrap_or_default();
        if title_page {
            section.first_header = Some(previous.2.clone().unwrap_or_default());
            section.first_footer = Some(previous.3.clone().unwrap_or_default());
        }
        section
    }

    fn block(&mut self, el: &Element, out: &mut Vec<Block>, ctx: &Context, rels: &Rels) {
        match el.local() {
            "p" => self.paragraph(el, out, ctx, rels),
            "tbl" => self.table(el, out, rels),
            "sdt" => {
                if let Some(content) = el.child("sdtContent") {
                    for c in content.elements() {
                        self.block(c, out, ctx, rels);
                    }
                }
            }
            "customXml" | "ins" | "moveTo" | "smartTag" => {
                for c in el.elements() {
                    self.block(c, out, ctx, rels);
                }
            }
            "AlternateContent" => {
                if let Some(choice) = el.child("Choice").or_else(|| el.child("Fallback")) {
                    for c in choice.elements() {
                        self.block(c, out, ctx, rels);
                    }
                }
            }
            _ => {}
        }
    }

    fn run_props(&self, base: &RunProps, own: Option<&Element>) -> RunProps {
        let mut props = base.clone();
        if let Some(own) = own {
            let direct = RunProps::read(own);
            if let Some(style) = &direct.style {
                props.over(&self.styles.char_props(style));
            }
            props.over(&direct);
        }
        props
    }

    fn family(&self, name: &Option<String>, theme: &Option<String>) -> Option<String> {
        theme
            .as_deref()
            .and_then(|t| self.styles.theme_font(t))
            .or_else(|| name.clone())
    }

    fn text_styles(&self, props: &RunProps) -> (TextStyle, TextStyle) {
        let latin_family = self
            .family(&props.ascii, &props.ascii_theme)
            .unwrap_or_else(|| "Times New Roman".into());
        let complex_family = self
            .family(&props.cs, &props.cs_theme)
            .unwrap_or_else(|| latin_family.clone());
        let size = props.size.unwrap_or(10.0);
        let size_cs = props.size_cs.unwrap_or(size);
        let small = |s: f32| match props.shift {
            Some(VerticalShift::Super | VerticalShift::Sub) => s * 0.65,
            _ => s,
        };
        let colour = props.colour.flatten().unwrap_or([0, 0, 0]);
        let base = TextStyle {
            family: latin_family,
            family_complex: Some(complex_family),
            size: small(size),
            size_complex: Some(small(size_cs)),
            bold: props.bold.unwrap_or(false),
            italic: props.italic.unwrap_or(false),
            underline: props.underline.unwrap_or(false),
            strike: props.strike.unwrap_or(false),
            colour,
            background: props.highlight.flatten(),
            shift: props.shift.unwrap_or_default(),
            letter_spacing: props.spacing.unwrap_or(0.0),
            link: None,
            caps: props.caps.unwrap_or(false) || props.small_caps.unwrap_or(false),
        };
        let mut complex = base.clone();
        complex.bold = props.bold_cs.or(props.bold).unwrap_or(false);
        complex.italic = props.italic_cs.or(props.italic).unwrap_or(false);
        (base, complex)
    }

    #[allow(clippy::too_many_lines)]
    fn paragraph(&mut self, el: &Element, out: &mut Vec<Block>, ctx: &Context, rels: &Rels) {
        let own = el.child("pPr").map(ParaProps::read).unwrap_or_default();
        let style_id = own.style.clone();
        let (mut props, mut run_base) = self.styles.para_props(style_id.as_deref());
        if let Some(table) = &ctx.table_style {
            for s in self.styles.chain(table) {
                let mut p = s.para.clone();
                p.over(&props);
                props = p;
                let mut r = s.run.clone();
                r.over(&run_base);
                run_base = r;
            }
        }
        let mut numbering = props.num.clone();
        if let Some(level) = props.ilvl
            && let Some(n) = &mut numbering
        {
            n.1 = level;
        }
        if let Some(n) = &own.num {
            numbering = Some(n.clone());
        } else if let (Some(level), Some(n)) = (own.ilvl, numbering.as_mut()) {
            n.1 = level;
        }
        let mut marker: Option<Marker> = None;
        if let Some((num, level)) = &numbering
            && num != "0"
            && let Some((lvl, abs, start)) = self
                .styles
                .level(num, *level)
                .map(|(l, a, s)| (l.clone(), a, s))
        {
            let mut p = props.clone();
            p.over(&lvl.para);
            p.over(&own);
            props = p;
            let key = (abs.clone(), *level);
            let value = self.counters.get(&key).map_or(start, |v| v + 1);
            self.counters.insert(key, value);
            let deeper: Vec<(String, usize)> = self
                .counters
                .keys()
                .filter(|(a, l)| *a == abs && *l > *level)
                .cloned()
                .collect();
            for k in deeper {
                self.counters.remove(&k);
            }
            let levels = self.styles.levels(num).cloned().unwrap_or_default();
            let mut text = lvl.text.clone();
            for (index, l) in levels.iter().enumerate().take(*level + 1) {
                let pattern = format!("%{}", index + 1);
                if text.contains(&pattern) {
                    let n = if index == *level {
                        value
                    } else {
                        self.counters
                            .get(&(abs.clone(), index))
                            .copied()
                            .unwrap_or(l.start)
                    };
                    let shown = if index < *level && l.format == "bullet" {
                        String::new()
                    } else {
                        convert_layout::numbering::format(n, &l.format)
                    };
                    text = text.replace(&pattern, &shown);
                }
            }
            if lvl.format == "bullet" {
                text = bullet(&text, lvl.run.ascii.as_deref());
            }
            if lvl.format != "none" && !text.is_empty() {
                let mut mprops = run_base.clone();
                if let Some(mark) = &props.mark {
                    mprops.over(mark);
                }
                mprops.over(&lvl.run);
                mprops.underline = Some(false);
                if lvl.format == "bullet" {
                    mprops.ascii = None;
                    mprops.ascii_theme = None;
                }
                let (style, _) = self.text_styles(&mprops);
                if lvl.suffix == "space" {
                    text.push(' ');
                }
                marker = Some(Marker {
                    text,
                    style,
                    outside: false,
                });
            }
        } else {
            props.over(&own);
        }
        let mut mark_props = run_base.clone();
        if let Some(mark) = &props.mark {
            mark_props.over(mark);
        }
        let (empty_style, _) = self.text_styles(&mark_props);

        let mut paragraph = Paragraph {
            align: props.align.unwrap_or(Align::Left),
            indent_left: props.left.unwrap_or(0.0),
            indent_right: props.right.unwrap_or(0.0),
            indent_first: props.first.unwrap_or(0.0),
            space_before: if props.before_auto == Some(true) {
                14.0
            } else {
                props.before.unwrap_or(0.0)
            },
            space_after: if props.after_auto == Some(true) {
                14.0
            } else {
                props.after.unwrap_or(0.0)
            },
            line_height: props.line.unwrap_or(LineHeight::Multiple(1.0)),
            marker,
            keep_with_next: props.keep_next.unwrap_or(false),
            page_break_before: props.page_break_before.unwrap_or(false),
            background: props.shading.flatten(),
            borders: props.borders.map(|b| b.sides()).unwrap_or_default(),
            tabs: props
                .tabs
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(pos, right)| TabStop {
                    pos: pos - props.left.unwrap_or(0.0),
                    right,
                })
                .collect(),
            empty_style,
            direction: if props.bidi == Some(true) {
                Direction::Rtl
            } else {
                Direction::Ltr
            },
            ..Paragraph::default()
        };
        if props.contextual == Some(true)
            && self.last_style.as_deref() == style_id.as_deref()
            && let Some(Block::Paragraph(prev)) = out.last_mut()
        {
            prev.space_after = 0.0;
            paragraph.space_before = 0.0;
        }
        let mut extra: Vec<Block> = Vec::new();
        let mut field: Vec<Field> = Vec::new();
        let mut pieces: Vec<Vec<Inline>> = vec![Vec::new()];
        self.inlines(
            el,
            &run_base,
            rels,
            None,
            &mut field,
            &mut pieces,
            &mut extra,
        );
        let count = pieces.len();
        for (index, inlines) in pieces.into_iter().enumerate() {
            let mut p = paragraph.clone();
            if index > 0 {
                out.push(Block::PageBreak);
                p.marker = None;
                p.space_before = 0.0;
                p.page_break_before = false;
            }
            if index + 1 < count {
                p.space_after = 0.0;
            }
            p.inlines = inlines;
            out.push(Block::Paragraph(p));
        }
        out.extend(extra);
        self.last_style = style_id;
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn inlines(
        &mut self,
        el: &Element,
        base: &RunProps,
        rels: &Rels,
        link: Option<&str>,
        field: &mut Vec<Field>,
        pieces: &mut Vec<Vec<Inline>>,
        extra: &mut Vec<Block>,
    ) {
        for c in el.elements() {
            match c.local() {
                "r" => self.run(c, base, rels, link, field, pieces, extra),
                "hyperlink" => {
                    let target = c
                        .attr("r:id")
                        .or_else(|| c.attr("id"))
                        .and_then(|id| rels.get(id))
                        .map(|(t, _)| t.clone())
                        .or_else(|| c.attr("anchor").map(|a| format!("#{a}")));
                    self.inlines(c, base, rels, target.as_deref(), field, pieces, extra);
                }
                "ins" | "moveTo" | "smartTag" | "customXml" | "fldSimple" | "bdo" | "dir" => {
                    if c.local() == "fldSimple" {
                        let instr = c.attr("instr").unwrap_or("").trim().to_ascii_uppercase();
                        if let Some(marker) = field_marker(&instr) {
                            let props = self.run_props(base, c.path(&["r", "rPr"]));
                            let (style, _) = self.text_styles(&props);
                            push_text(pieces, marker, &style);
                            continue;
                        }
                    }
                    self.inlines(c, base, rels, link, field, pieces, extra);
                }
                "sdt" => {
                    if let Some(content) = c.child("sdtContent") {
                        self.inlines(content, base, rels, link, field, pieces, extra);
                    }
                }
                "AlternateContent" => {
                    if let Some(choice) = c.child("Choice").or_else(|| c.child("Fallback")) {
                        self.inlines(choice, base, rels, link, field, pieces, extra);
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn run(
        &mut self,
        r: &Element,
        base: &RunProps,
        rels: &Rels,
        link: Option<&str>,
        field: &mut Vec<Field>,
        pieces: &mut Vec<Vec<Inline>>,
        extra: &mut Vec<Block>,
    ) {
        let props = self.run_props(base, r.child("rPr"));
        if props.vanish == Some(true) {
            return;
        }
        let (mut latin, mut complex) = self.text_styles(&props);
        if let Some(link) = link {
            latin.link = Some(link.to_owned());
            complex.link = Some(link.to_owned());
        }
        let hiding = |field: &Vec<Field>| {
            field
                .iter()
                .any(|f| !f.result || field_marker(&f.instr).is_some())
        };
        for c in r.elements() {
            match c.local() {
                "fldChar" => match c.attr("fldCharType") {
                    Some("begin") => field.push(Field {
                        instr: String::new(),
                        result: false,
                    }),
                    Some("separate") => {
                        if let Some(f) = field.last_mut() {
                            f.result = true;
                            let instr = f.instr.trim().to_ascii_uppercase();
                            if let Some(marker) = field_marker(&instr) {
                                push_text(pieces, marker, &latin);
                            }
                        }
                    }
                    Some("end") => {
                        if let Some(f) = field.pop()
                            && !f.result
                        {
                            let instr = f.instr.trim().to_ascii_uppercase();
                            if let Some(marker) = field_marker(&instr) {
                                push_text(pieces, marker, &latin);
                            }
                        }
                    }
                    _ => {}
                },
                "instrText" => {
                    if let Some(f) = field.last_mut() {
                        f.instr.push_str(&c.text());
                    }
                }
                _ if hiding(field) => {}
                "t" => {
                    let text = c.text();
                    push_split(pieces, &text, &latin, &complex);
                }
                "tab" | "ptab" => pieces
                    .last_mut()
                    .unwrap_or(&mut Vec::new())
                    .push(Inline::Tab {
                        style: latin.clone(),
                    }),
                "br" | "cr" => {
                    if c.attr("type") == Some("page") {
                        pieces.push(Vec::new());
                    } else if let Some(last) = pieces.last_mut() {
                        last.push(Inline::LineBreak);
                    }
                }
                "noBreakHyphen" => push_text(pieces, "\u{2011}", &latin),
                "softHyphen" => {}
                "sym" => {
                    let code = c
                        .attr("char")
                        .and_then(|h| u32::from_str_radix(h, 16).ok())
                        .map(|v| if v >= 0xF000 { v - 0xF000 } else { v });
                    if let Some(ch) = code.and_then(char::from_u32) {
                        let font = c.attr("font").unwrap_or("");
                        let shown = symbol(ch, font);
                        push_text(pieces, &shown, &latin);
                    }
                }
                "footnoteReference" | "endnoteReference" => {
                    let (number, note) = self.footnote(c.attr("id").unwrap_or(""), &latin, rels);
                    let mut sup = latin.clone();
                    sup.shift = VerticalShift::Super;
                    sup.size *= 0.65;
                    push_text(pieces, &number, &sup);
                    if let (Some(note), Some(last)) = (note, pieces.last_mut()) {
                        last.push(Inline::Anchor(note));
                    }
                }
                "drawing" => self.drawing(c, rels, pieces, extra),
                "pict" | "object" => {
                    for image in c.descendants("imagedata") {
                        if let Some(id) = image.attr("r:id").or_else(|| image.attr("id")) {
                            self.picture(id, None, rels, pieces);
                        }
                    }
                    for tb in c.descendants("txbxContent") {
                        for el in tb.elements() {
                            self.block(el, extra, &Context::default(), rels);
                        }
                    }
                }
                "AlternateContent" => {
                    if let Some(choice) = c.child("Choice").or_else(|| c.child("Fallback")) {
                        let mut wrapper = r.clone();
                        wrapper.children = choice.children.clone();
                        self.run(&wrapper, base, rels, link, field, pieces, extra);
                    }
                }
                _ => {}
            }
        }
    }

    fn footnote(&mut self, id: &str, style: &TextStyle, rels: &Rels) -> (String, Option<usize>) {
        let label = (self.doc_notes.len() + 1).to_string();
        let _ = rels;
        let Some(xml) = self.footnote_xml.clone() else {
            return (label, None);
        };
        let Some(note) = xml.elements().find(|f| f.attr("id") == Some(id)) else {
            return (label, None);
        };
        let note_rels = rels_map(self.package, "word/footnotes.xml");
        let mut blocks = Vec::new();
        let outer = self.last_style.take();
        for el in note.elements() {
            self.block(el, &mut blocks, &Context::default(), &note_rels);
        }
        self.last_style = outer;
        if let Some(Block::Paragraph(p)) =
            blocks.iter_mut().find(|b| matches!(b, Block::Paragraph(_)))
        {
            let mut ms = style.clone();
            ms.size = (ms.size * 0.8).max(6.0);
            ms.link = None;
            p.marker = Some(Marker {
                text: label.clone(),
                style: ms,
                outside: true,
            });
        }
        self.doc_notes.push(blocks);
        (label, Some(self.doc_notes.len() - 1))
    }

    fn drawing(
        &mut self,
        d: &Element,
        rels: &Rels,
        pieces: &mut [Vec<Inline>],
        extra: &mut Vec<Block>,
    ) {
        for holder in d.elements() {
            let extent = holder.child("extent");
            #[allow(clippy::cast_precision_loss)]
            let size = extent.and_then(|e| {
                let cx = e.attr("cx")?.parse::<f64>().ok()? / 12_700.0;
                let cy = e.attr("cy")?.parse::<f64>().ok()? / 12_700.0;
                #[allow(clippy::cast_possible_truncation)]
                Some((cx as f32, cy as f32))
            });
            for blip in holder.descendants("blip") {
                if let Some(id) = blip.attr("r:embed").or_else(|| blip.attr("embed")) {
                    self.picture(id, size, rels, pieces);
                }
            }
            for tb in holder.descendants("txbxContent") {
                for el in tb.elements() {
                    self.block(el, extra, &Context::default(), rels);
                }
            }
        }
    }

    fn picture(
        &mut self,
        id: &str,
        size: Option<(f32, f32)>,
        rels: &Rels,
        pieces: &mut [Vec<Inline>],
    ) {
        let Some((target, external)) = rels.get(id) else {
            return;
        };
        if *external {
            self.notes
                .push(format!("linked picture not fetched: {target}"));
            return;
        }
        let index = if let Some(i) = self.image_cache.get(target) {
            *i
        } else {
            let Ok(bytes) = self.package.bytes(target) else {
                return;
            };
            if convert_layout::image_size(&bytes).is_none() {
                self.notes.push(format!(
                    "picture not drawn (JPEG, PNG and GIF only): {target}"
                ));
                return;
            }
            self.images.push(ImageData { bytes });
            let i = self.images.len() - 1;
            self.image_cache.insert(target.clone(), i);
            i
        };
        #[allow(clippy::cast_precision_loss)]
        let (width, height) = size.unwrap_or_else(|| {
            convert_layout::image_size(&self.images[index].bytes)
                .map_or((100.0, 100.0), |(w, h)| (w as f32 * 0.75, h as f32 * 0.75))
        });
        if let Some(last) = pieces.last_mut() {
            last.push(Inline::Image {
                image: index,
                width,
                height,
            });
        }
    }

    #[allow(clippy::too_many_lines)]
    fn table(&mut self, el: &Element, out: &mut Vec<Block>, rels: &Rels) {
        let pr = el.child("tblPr");
        let style_id = pr
            .and_then(|p| p.child("tblStyle"))
            .and_then(|s| s.attr("val"))
            .map(str::to_owned)
            .or_else(|| self.styles.default_table.clone());
        let mut borders = Borders::default();
        let mut cell_margin: Sides<Option<f32>> = Sides::default();
        if let Some(id) = &style_id {
            for s in self.styles.chain(id) {
                if let Some(b) = &s.table_borders {
                    borders.over(b);
                }
                if let Some(m) = s.cell_margins {
                    over_margins(&mut cell_margin, m);
                }
            }
        }
        if let Some(b) = pr.and_then(|p| p.child("tblBorders")) {
            borders.over(&Borders::read(b));
        }
        if let Some(m) = pr.and_then(|p| p.child("tblCellMar")) {
            over_margins(&mut cell_margin, margins(m));
        }
        let default_pad = Sides {
            top: cell_margin.top.unwrap_or(0.0),
            right: cell_margin.right.unwrap_or(5.4),
            bottom: cell_margin.bottom.unwrap_or(0.0),
            left: cell_margin.left.unwrap_or(5.4),
        };
        let columns: Vec<f32> = el
            .child("tblGrid")
            .map(|g| {
                g.children_named("gridCol")
                    .filter_map(|c| c.attr("w").and_then(twips))
                    .collect()
            })
            .unwrap_or_default();
        let width = pr.and_then(|p| p.child("tblW")).and_then(|w| {
            let value: f32 = w.attr("w")?.trim_end_matches('%').parse().ok()?;
            match w.attr("type") {
                Some("pct") => Some(TableWidth::Percent(if w.attr("w")?.ends_with('%') {
                    value / 100.0
                } else {
                    value / 5000.0
                })),
                Some("dxa") if value > 0.0 => Some(TableWidth::Fixed(value / 20.0)),
                _ => None,
            }
        });
        let align = match pr.and_then(|p| p.child("jc")).and_then(|j| j.attr("val")) {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        };
        let indent = pr
            .and_then(|p| p.child("tblInd"))
            .and_then(|i| i.attr("w"))
            .and_then(twips)
            .unwrap_or(0.0);
        let ctx = Context {
            table_style: style_id.clone(),
        };
        let rows_el: Vec<&Element> = el.children_named("tr").collect();
        let row_count = rows_el.len();
        let mut rows = Vec::new();
        for (r, tr) in rows_el.iter().enumerate() {
            let trpr = tr.child("trPr");
            let header = trpr
                .and_then(|p| p.child("tblHeader"))
                .is_some_and(|h| !matches!(h.attr("val"), Some("0" | "false")));
            let height = trpr.and_then(|p| p.child("trHeight"));
            let h = height
                .and_then(|h| h.attr("val"))
                .and_then(twips)
                .unwrap_or(0.0);
            let exact = height.is_some_and(|h| h.attr("hRule") == Some("exact"));
            let skip_before = trpr
                .and_then(|p| p.child("gridBefore"))
                .and_then(|g| g.attr("val"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let mut cells = Vec::new();
            if skip_before > 0 {
                cells.push(Cell {
                    span: skip_before,
                    ..Cell::default()
                });
            }
            let tcs: Vec<&Element> = tr.children_named("tc").collect();
            let mut col = skip_before;
            for tc in &tcs {
                let tcpr = tc.child("tcPr");
                let span = tcpr
                    .and_then(|p| p.child("gridSpan"))
                    .and_then(|g| g.attr("val"))
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1);
                let vmerge = tcpr
                    .and_then(|p| p.child("vMerge"))
                    .map_or(VMerge::None, |v| {
                        if v.attr("val") == Some("restart") {
                            VMerge::Restart
                        } else {
                            VMerge::Continue
                        }
                    });
                let mut blocks = Vec::new();
                for c in tc.elements() {
                    if c.local() != "tcPr" {
                        self.block(c, &mut blocks, &ctx, rels);
                    }
                }
                let last_col = col + span >= columns.len().max(1);
                let mut sides = Borders {
                    top: if r == 0 {
                        borders.top
                    } else {
                        borders.inside_h
                    },
                    bottom: if r + 1 == row_count {
                        borders.bottom
                    } else {
                        borders.inside_h
                    },
                    left: if col == 0 {
                        borders.left
                    } else {
                        borders.inside_v
                    },
                    right: if last_col {
                        borders.right
                    } else {
                        borders.inside_v
                    },
                    ..Borders::default()
                };
                if let Some(b) = tcpr.and_then(|p| p.child("tcBorders")) {
                    sides.over(&Borders::read(b));
                }
                let mut pad = default_pad;
                if let Some(m) = tcpr.and_then(|p| p.child("tcMar")) {
                    let m = margins(m);
                    pad = Sides {
                        top: m.top.unwrap_or(pad.top),
                        right: m.right.unwrap_or(pad.right),
                        bottom: m.bottom.unwrap_or(pad.bottom),
                        left: m.left.unwrap_or(pad.left),
                    };
                }
                cells.push(Cell {
                    blocks,
                    span,
                    row_span: 1,
                    vmerge,
                    background: tcpr
                        .and_then(|p| p.child("shd"))
                        .and_then(|s| s.attr("fill"))
                        .and_then(props::hex_colour),
                    borders: sides.sides(),
                    padding: Some(pad),
                    valign: match tcpr
                        .and_then(|p| p.child("vAlign"))
                        .and_then(|v| v.attr("val"))
                    {
                        Some("center") => VAlign::Middle,
                        Some("bottom") => VAlign::Bottom,
                        _ => VAlign::Top,
                    },
                    width: None,
                });
                col += span;
            }
            rows.push(Row {
                cells,
                min_height: if exact { 0.0 } else { h },
                exact_height: exact.then_some(h),
                header,
            });
        }
        out.push(Block::Table(Table {
            columns,
            width: width.unwrap_or(TableWidth::Columns),
            rows,
            indent,
            align,
            space_before: 0.0,
            space_after: 0.0,
            padding: default_pad,
            outer_border: None,
            collapse: true,
        }));
    }
}

fn over_margins(mine: &mut Sides<Option<f32>>, other: Sides<Option<f32>>) {
    if other.top.is_some() {
        mine.top = other.top;
    }
    if other.right.is_some() {
        mine.right = other.right;
    }
    if other.bottom.is_some() {
        mine.bottom = other.bottom;
    }
    if other.left.is_some() {
        mine.left = other.left;
    }
}

fn field_marker(instr: &str) -> Option<&'static str> {
    let word = instr.split_whitespace().next()?;
    match word {
        "PAGE" => Some(FIELD_PAGE),
        "NUMPAGES" | "SECTIONPAGES" => Some(FIELD_PAGES),
        _ => None,
    }
}

fn bullet(text: &str, font: Option<&str>) -> String {
    let font = font.unwrap_or("").to_ascii_lowercase();
    text.chars()
        .map(|c| {
            let c = if u32::from(c) >= 0xF000 {
                char::from_u32(u32::from(c) - 0xF000).unwrap_or(c)
            } else {
                c
            };
            symbol(c, &font).chars().next().unwrap_or('•')
        })
        .collect()
}

fn symbol(c: char, font: &str) -> String {
    let font = font.to_ascii_lowercase();
    if font.contains("symbol") {
        return match c {
            '\u{B7}' => "•".into(),
            '\u{A8}' => "♣".into(),
            _ => c.to_string(),
        };
    }
    if font.contains("wingdings") {
        return match c {
            '\u{A7}' => "▪".into(),
            '\u{A8}' => "□".into(),
            '\u{FC}' => "✓".into(),
            '\u{D8}' => "➢".into(),
            '\u{76}' => "❖".into(),
            '\u{6F}' => "□".into(),
            '\u{71}' => "❑".into(),
            '\u{6E}' => "■".into(),
            '\u{9F}' => "•".into(),
            _ => "•".into(),
        };
    }
    if font.contains("courier") && c == 'o' {
        return "◦".into();
    }
    c.to_string()
}

fn push_text(pieces: &mut [Vec<Inline>], text: &str, style: &TextStyle) {
    if let Some(last) = pieces.last_mut() {
        if let Some(Inline::Text {
            text: before,
            style: s,
        }) = last.last_mut()
            && s == style
        {
            before.push_str(text);
            return;
        }
        last.push(Inline::Text {
            text: text.to_owned(),
            style: style.clone(),
        });
    }
}

fn push_split(pieces: &mut [Vec<Inline>], text: &str, latin: &TextStyle, complex: &TextStyle) {
    if latin == complex {
        push_text(pieces, text, latin);
        return;
    }
    let mut current = String::new();
    let mut in_complex: Option<bool> = None;
    for c in text.chars() {
        let class = if convert_layout::fonts::is_complex(c) {
            Some(true)
        } else if c.is_alphabetic() {
            Some(false)
        } else {
            None
        };
        if let (Some(now), Some(was)) = (class, in_complex)
            && now != was
        {
            push_text(pieces, &current, if was { complex } else { latin });
            current.clear();
        }
        if class.is_some() {
            in_complex = class;
        }
        current.push(c);
    }
    if !current.is_empty() {
        push_text(
            pieces,
            &current,
            if in_complex == Some(true) {
                complex
            } else {
                latin
            },
        );
    }
}
