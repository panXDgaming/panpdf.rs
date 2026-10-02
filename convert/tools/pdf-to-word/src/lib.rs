use std::fmt::Write as _;
use std::sync::Arc;

use convert_ooxml::{Package, Rels, emu, faces, half_points, hex, rel, twips};
use convert_structure::model::{
    Align, Block, Document, ListKind, Page, Paragraph, Picture, Role, Shift, Table,
};
use convert_structure::script::{self, Script};
use convert_structure::{FontProvider, Options, Output, Request};
use convert_xml::escape;

pub use convert_structure;

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

const MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";

pub fn convert(
    pdf: Vec<u8>,
    request: &Request,
    fonts: Option<Arc<dyn FontProvider>>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String> {
    let mut package = Package::new();
    let mut stored = 0;
    let mut sink = |page: &mut Page| {
        for block in &mut page.blocks {
            if let Block::Picture(picture) = block {
                let data = std::mem::take(&mut picture.data);
                if picture.background {
                    continue;
                }
                stored += 1;
                let name = format!("image{stored}.{}", picture.format.extension());
                package.file(&format!("word/media/{name}"), data);
                picture.stored = Some(name);
            }
        }
    };
    let document = convert_structure::read(
        pdf,
        request,
        fonts,
        &Options::default(),
        progress,
        &mut sink,
    )?;
    Ok(Output {
        main: write_into(package, &document).map_err(|error| error.to_string())?,
        attachments: Vec::new(),
        notes: notes(&document),
        dropped: String::new(),
    })
}

pub fn write_output(document: &Document, _request: &Request) -> Result<Output, String> {
    Ok(Output {
        main: write(document).map_err(|error| error.to_string())?,
        attachments: Vec::new(),
        notes: notes(document),
        dropped: String::new(),
    })
}

convert_wasm::export_tool!(
    convert_structure::Options::default(),
    crate::write_output,
    "docx"
);

#[must_use]
pub fn notes(document: &Document) -> Vec<String> {
    let mut notes = Vec::new();
    for page in &document.pages {
        if let Some(why) = &page.refused {
            notes.push(format!("page {}: {why}", page.index + 1));
        }
        for note in &page.notes {
            notes.push(format!("page {}: {note}", page.index + 1));
        }
    }
    notes
}

struct Margins {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

fn margins(document: &Document) -> Margins {
    let mut left = f64::INFINITY;
    let mut right = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    for page in &document.pages {
        for block in &page.blocks {
            if let Block::Picture(picture) = block
                && picture.background
            {
                continue;
            }
            let frame = block.frame();
            left = left.min(frame.x0);
            right = right.min(page.width - frame.x1);
            top = top.min(frame.y0);
            bottom = bottom.min(page.height - frame.y1);
        }
    }
    let clamp = |v: f64| {
        if v.is_finite() {
            v.clamp(18.0, 72.0)
        } else {
            72.0
        }
    };
    Margins {
        left: clamp(left),
        right: clamp(right),
        top: clamp(top),
        bottom: clamp(bottom),
    }
}

#[derive(Default)]
struct Lists {
    instances: Vec<(usize, u32)>,
    current: Option<(usize, usize, u32)>,
}

impl Lists {
    fn abstract_of(kind: &ListKind) -> Option<(usize, u32)> {
        match kind {
            ListKind::Bullet => Some((0, 1)),
            ListKind::Decimal(n) => Some((1, *n)),
            ListKind::LowerLetter(n) => Some((2, *n)),
            ListKind::UpperLetter(n) => Some((3, *n)),
            ListKind::Kept => None,
        }
    }

    fn number(&mut self, kind: &ListKind) -> Option<usize> {
        let (abstract_id, value) = Self::abstract_of(kind)?;
        if let Some((current_abstract, id, last)) = self.current
            && current_abstract == abstract_id
            && (abstract_id == 0 || value == last + 1)
        {
            self.current = Some((abstract_id, id, value));
            return Some(id);
        }
        self.instances.push((abstract_id, value));
        let id = self.instances.len();
        self.current = Some((abstract_id, id, value));
        Some(id)
    }

    fn interrupt(&mut self) {
        self.current = None;
    }
}

struct Writer<'d> {
    body: String,
    rels: Rels,
    media: Vec<(String, Vec<u8>)>,
    lists: Lists,
    margins: Margins,
    text_width: f64,
    pictures: usize,
    document: &'d Document,
}

pub fn write(document: &Document) -> Result<Vec<u8>, convert_zip::ZipError> {
    write_into(Package::new(), document)
}

pub fn write_into(
    mut package: Package,
    document: &Document,
) -> Result<Vec<u8>, convert_zip::ZipError> {
    let margins = margins(document);
    let first = document
        .pages
        .iter()
        .find(|page| page.refused.is_none() && page.width > 0.0);
    let (page_width, page_height) = first.map_or((595.0, 842.0), |page| (page.width, page.height));
    let mut writer = Writer {
        body: String::with_capacity(64 * 1024),
        rels: Rels::new(),
        media: Vec::new(),
        lists: Lists::default(),
        text_width: (page_width - margins.left - margins.right).max(72.0),
        margins,
        pictures: 0,
        document,
    };
    writer.rels.add(rel::STYLES, "styles.xml");
    writer.rels.add(rel::NUMBERING, "numbering.xml");
    writer.rels.add(rel::SETTINGS, "settings.xml");
    let header = document
        .pages
        .iter()
        .find(|page| !page.header.is_empty())
        .map(|page| &page.header);
    let footer = document
        .pages
        .iter()
        .find(|page| !page.footer.is_empty())
        .map(|page| &page.footer);
    let header_id = header.map(|_| writer.rels.add(rel::HEADER, "header1.xml"));
    let footer_id = footer.map(|_| writer.rels.add(rel::FOOTER, "footer1.xml"));

    let mut size = (page_width, page_height);
    let mut first_page = true;
    for page in &document.pages {
        if page.refused.is_some() || page.width <= 0.0 {
            continue;
        }
        let page_size = (page.width, page.height);
        if !first_page {
            if (page_size.0 - size.0).abs() > 1.0 || (page_size.1 - size.1).abs() > 1.0 {
                let section = writer.section(size, header_id.as_deref(), footer_id.as_deref());
                let _ = write!(writer.body, "<w:p><w:pPr>{section}</w:pPr></w:p>");
                size = page_size;
            } else {
                writer
                    .body
                    .push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
            }
        }
        first_page = false;
        writer.text_width = (page.width - writer.margins.left - writer.margins.right).max(72.0);
        let mut last_was_table = false;
        for block in &page.blocks {
            match block {
                Block::Paragraph(paragraph) => {
                    writer.paragraph(paragraph, false);
                    last_was_table = false;
                }
                Block::Table(table) => {
                    if last_was_table {
                        writer.body.push_str("<w:p/>");
                    }
                    writer.lists.interrupt();
                    writer.table(table);
                    last_was_table = true;
                }
                Block::Picture(picture) => {
                    if picture.background {
                        continue;
                    }
                    writer.lists.interrupt();
                    writer.picture(picture, page.width);
                    last_was_table = false;
                }
            }
        }
        if last_was_table {
            writer.body.push_str("<w:p/>");
        }
    }
    let final_section = writer.section(size, header_id.as_deref(), footer_id.as_deref());

    let mut document_xml = String::with_capacity(writer.body.len() + 1024);
    document_xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    let _ = write!(
        document_xml,
        "<w:document xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\" xmlns:wp=\"{WP_NS}\" xmlns:a=\"{A_NS}\" xmlns:pic=\"{PIC_NS}\"><w:body>"
    );
    document_xml.push_str(&writer.body);
    document_xml.push_str(&final_section);
    document_xml.push_str("</w:body></w:document>");

    package.default_type("png", "image/png");
    package.default_type("jpeg", "image/jpeg");
    package.part("word/document.xml", MAIN, document_xml.into_bytes());
    package.part("word/styles.xml", STYLES, styles(document).into_bytes());
    package.part(
        "word/numbering.xml",
        NUMBERING,
        numbering(&writer.lists).into_bytes(),
    );
    package.part(
        "word/settings.xml",
        SETTINGS,
        SETTINGS_XML.as_bytes().to_vec(),
    );
    if let Some(header) = header {
        let xml = writer.part_of("w:hdr", header);
        package.part("word/header1.xml", HEADER, xml.into_bytes());
    }
    if let Some(footer) = footer {
        let xml = writer.part_of("w:ftr", footer);
        package.part("word/footer1.xml", FOOTER, xml.into_bytes());
    }
    for (name, bytes) in std::mem::take(&mut writer.media) {
        package.file(&format!("word/media/{name}"), bytes);
    }
    package.file(
        "word/_rels/document.xml.rels",
        writer.rels.xml().into_bytes(),
    );
    package.properties_and_root("word/document.xml", "", "PanPDF");
    package.finish()
}

const SETTINGS_XML: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n",
    "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:defaultTabStop w:val=\"720\"/>",
    "<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat>",
    "</w:settings>"
);

impl Writer<'_> {
    fn section(
        &self,
        (width, height): (f64, f64),
        header: Option<&str>,
        footer: Option<&str>,
    ) -> String {
        let m = &self.margins;
        let mut out = String::from("<w:sectPr>");
        if let Some(id) = header {
            let _ = write!(out, "<w:headerReference w:type=\"default\" r:id=\"{id}\"/>");
        }
        if let Some(id) = footer {
            let _ = write!(out, "<w:footerReference w:type=\"default\" r:id=\"{id}\"/>");
        }
        let orient = if width > height {
            " w:orient=\"landscape\""
        } else {
            ""
        };
        let _ = write!(
            out,
            "<w:pgSz w:w=\"{}\" w:h=\"{}\"{orient}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/></w:sectPr>",
            twips(width),
            twips(height),
            twips(m.top),
            twips(m.right),
            twips(m.bottom),
            twips(m.left),
            twips((m.top / 2.0).max(12.0)),
            twips((m.bottom / 2.0).max(12.0)),
        );
        out
    }

    fn paragraph(&mut self, paragraph: &Paragraph, in_cell: bool) {
        let mut ppr = String::new();
        match &paragraph.role {
            Role::Heading(level) => {
                let _ = write!(ppr, "<w:pStyle w:val=\"Heading{level}\"/>");
                self.lists.interrupt();
            }
            Role::ListItem(item) => {
                if let Some(id) = self.lists.number(&item.kind) {
                    let _ = write!(
                        ppr,
                        "<w:pStyle w:val=\"ListParagraph\"/><w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{id}\"/></w:numPr>",
                        item.level.min(2)
                    );
                }
            }
            Role::Body => self.lists.interrupt(),
        }
        let before = if in_cell {
            0.0
        } else {
            paragraph.space_before.min(36.0)
        };
        let line = if paragraph.lines > 1 && paragraph.pitch > paragraph.size() * 1.05 {
            format!(
                " w:line=\"{}\" w:lineRule=\"exact\"",
                twips(paragraph.pitch)
            )
        } else {
            String::new()
        };
        let _ = write!(
            ppr,
            "<w:spacing w:before=\"{}\" w:after=\"0\"{line}/>",
            twips(before)
        );
        let is_list =
            matches!(&paragraph.role, Role::ListItem(item) if item.kind != ListKind::Kept);
        if !in_cell && !is_list && matches!(paragraph.align, Align::Left | Align::Justify) {
            let left =
                (paragraph.frame.x0 - self.margins.left - paragraph.first_indent.max(0.0)).max(0.0);
            let first = paragraph.first_indent;
            let first = if first > 1.0 {
                format!(" w:firstLine=\"{}\"", twips(first))
            } else if first < -1.0 {
                format!(" w:hanging=\"{}\"", twips(-first))
            } else {
                String::new()
            };
            if left > 1.0 || !first.is_empty() {
                let _ = write!(
                    ppr,
                    "<w:ind w:left=\"{}\"{first}/>",
                    twips(left.min(self.text_width - 36.0).max(0.0))
                );
            }
        }
        match paragraph.align {
            Align::Center => ppr.push_str("<w:jc w:val=\"center\"/>"),
            Align::Right => ppr.push_str("<w:jc w:val=\"right\"/>"),
            Align::Justify => ppr.push_str("<w:jc w:val=\"both\"/>"),
            Align::Left => {}
        }
        let _ = write!(self.body, "<w:p><w:pPr>{ppr}</w:pPr>");
        for run in &paragraph.runs {
            write_run(&mut self.body, &run.text, &run.style);
        }
        self.body.push_str("</w:p>");
    }

    fn table(&mut self, table: &Table) {
        let total: f64 = table.columns.iter().sum();
        let scale = if total > self.text_width {
            self.text_width / total
        } else {
            1.0
        };
        let _ = write!(
            self.body,
            "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"{}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/><w:tblLook w:val=\"04A0\" w:firstRow=\"1\" w:lastRow=\"0\" w:firstColumn=\"1\" w:lastColumn=\"0\" w:noHBand=\"0\" w:noVBand=\"1\"/></w:tblPr><w:tblGrid>",
            twips(total * scale)
        );
        for width in &table.columns {
            let _ = write!(self.body, "<w:gridCol w:w=\"{}\"/>", twips(width * scale));
        }
        self.body.push_str("</w:tblGrid>");
        for (r, row) in table.rows.iter().enumerate() {
            let _ = write!(
                self.body,
                "<w:tr><w:trPr><w:trHeight w:val=\"{}\" w:hRule=\"atLeast\"/></w:trPr>",
                twips(row.height * 0.9)
            );
            let mut c = 0;
            while c < row.cells.len() {
                let cell = &row.cells[c];
                if cell.covered {
                    let above = (0..r).rev().find_map(|r2| {
                        let owner = &table.rows[r2].cells[c];
                        (!owner.covered && owner.span.1 > r - r2).then_some(owner.span.0)
                    });
                    if let Some(columns) = above {
                        let width: f64 = table.columns[c..(c + columns).min(table.columns.len())]
                            .iter()
                            .sum();
                        let _ = write!(
                            self.body,
                            "<w:tc><w:tcPr><w:tcW w:w=\"{}\" w:type=\"dxa\"/>{}<w:vMerge/></w:tcPr><w:p/></w:tc>",
                            twips(width * scale),
                            if columns > 1 {
                                format!("<w:gridSpan w:val=\"{columns}\"/>")
                            } else {
                                String::new()
                            }
                        );
                        c += columns.max(1);
                    } else {
                        c += 1;
                    }
                    continue;
                }
                let (columns, rows) = cell.span;
                let width: f64 = table.columns[c..(c + columns).min(table.columns.len())]
                    .iter()
                    .sum();
                let mut tcpr = format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", twips(width * scale));
                if columns > 1 {
                    let _ = write!(tcpr, "<w:gridSpan w:val=\"{columns}\"/>");
                }
                if rows > 1 {
                    tcpr.push_str("<w:vMerge w:val=\"restart\"/>");
                }
                if let Some(fill) = cell.fill {
                    let _ = write!(
                        tcpr,
                        "<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
                        hex(fill)
                    );
                }
                let _ = write!(self.body, "<w:tc><w:tcPr>{tcpr}</w:tcPr>");
                if cell.paragraphs.is_empty() {
                    self.body.push_str("<w:p/>");
                }
                for paragraph in &cell.paragraphs {
                    self.paragraph(paragraph, true);
                }
                self.lists.interrupt();
                self.body.push_str("</w:tc>");
                c += columns.max(1);
            }
            self.body.push_str("</w:tr>");
        }
        self.body.push_str("</w:tbl>");
    }

    fn picture(&mut self, picture: &Picture, page_width: f64) {
        self.pictures += 1;
        let n = self.pictures;
        let name = match &picture.stored {
            Some(name) => name.clone(),
            None => {
                let name = format!("picture{n}.{}", picture.format.extension());
                self.media.push((name.clone(), picture.data.clone()));
                name
            }
        };
        let id = self.rels.add(rel::IMAGE, &format!("media/{name}"));
        let mut width = picture.frame.width();
        let mut height = picture.frame.height();
        if width > self.text_width {
            height *= self.text_width / width;
            width = self.text_width;
        }
        let center = picture.frame.center().0;
        let jc = if (center - page_width / 2.0).abs() < 12.0 {
            "<w:jc w:val=\"center\"/>"
        } else {
            ""
        };
        let left = if jc.is_empty() {
            (picture.frame.x0 - self.margins.left).clamp(0.0, (self.text_width - width).max(0.0))
        } else {
            0.0
        };
        let (cx, cy) = (emu(width), emu(height));
        let _ = write!(
            self.body,
            "<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\"/><w:ind w:left=\"{}\"/>{jc}</w:pPr><w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{n}\" name=\"Picture {n}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"{PIC_NS}\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{n}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>",
            twips(left)
        );
    }

    fn part_of(&self, root: &str, paragraphs: &[Paragraph]) -> String {
        let mut out =
            String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
        let _ = write!(out, "<{root} xmlns:w=\"{W_NS}\" xmlns:r=\"{R_NS}\">");
        for paragraph in paragraphs {
            let jc = match paragraph.align {
                Align::Center => "<w:jc w:val=\"center\"/>",
                Align::Right => "<w:jc w:val=\"right\"/>",
                _ => {
                    let center = paragraph.frame.center().0;
                    let width = self.document.pages.first().map_or(595.0, |page| page.width);
                    if (center - width / 2.0).abs() < 20.0 {
                        "<w:jc w:val=\"center\"/>"
                    } else if center > width * 0.66 {
                        "<w:jc w:val=\"right\"/>"
                    } else {
                        ""
                    }
                }
            };
            let _ = write!(
                out,
                "<w:p><w:pPr><w:pStyle w:val=\"{}\"/>{jc}</w:pPr>",
                if root == "w:hdr" { "Header" } else { "Footer" }
            );
            let text = paragraph.text();
            let digits_only =
                text.trim().chars().all(|c| c.is_ascii_digit()) && !text.trim().is_empty();
            if digits_only {
                let style = paragraph
                    .runs
                    .first()
                    .map(|run| run.style.clone())
                    .unwrap_or_default();
                let mut rpr = String::new();
                run_properties(&mut rpr, &style, Script::Latin);
                let _ = write!(
                    out,
                    "<w:r>{rpr}<w:fldChar w:fldCharType=\"begin\"/></w:r><w:r>{rpr}<w:instrText xml:space=\"preserve\"> PAGE </w:instrText></w:r><w:r>{rpr}<w:fldChar w:fldCharType=\"separate\"/></w:r><w:r>{rpr}<w:t>{}</w:t></w:r><w:r>{rpr}<w:fldChar w:fldCharType=\"end\"/></w:r>",
                    text.trim()
                );
            } else {
                for run in &paragraph.runs {
                    write_run(&mut out, &run.text, &run.style);
                }
            }
            out.push_str("</w:p>");
        }
        let _ = write!(out, "</{root}>");
        out
    }
}

fn complex_script(text: &str) -> Option<Script> {
    let mut best: Option<(Script, usize)> = None;
    for (script, piece) in script::split(text) {
        if script.is_complex() {
            let n = piece.chars().count();
            if best.is_none_or(|(_, m)| n > m) {
                best = Some((script, n));
            }
        }
    }
    best.map(|(script, _)| script)
}

fn run_properties(out: &mut String, style: &convert_structure::model::Style, complex: Script) {
    let slots = faces(style, complex);
    let (latin, _) = escape(&slots.latin);
    let (east, _) = escape(&slots.east_asian);
    let (cs, _) = escape(&slots.complex);
    out.push_str("<w:rPr>");
    let _ = write!(
        out,
        "<w:rFonts w:ascii=\"{latin}\" w:hAnsi=\"{latin}\" w:eastAsia=\"{east}\" w:cs=\"{cs}\"/>"
    );
    if style.bold {
        out.push_str("<w:b/><w:bCs/>");
    }
    if style.italic {
        out.push_str("<w:i/><w:iCs/>");
    }
    if style.underline {
        out.push_str("<w:u w:val=\"single\"/>");
    }
    if style.color != [0, 0, 0] {
        let _ = write!(out, "<w:color w:val=\"{}\"/>", hex(style.color));
    }
    if style.size > 0.0 {
        let size = half_points(style.size);
        let _ = write!(out, "<w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/>");
    }
    match style.baseline {
        Shift::Up => out.push_str("<w:vertAlign w:val=\"superscript\"/>"),
        Shift::Down => out.push_str("<w:vertAlign w:val=\"subscript\"/>"),
        Shift::None => {}
    }
    if let Some(tag) = complex.is_complex().then(|| complex.language()).flatten() {
        if complex.is_right_to_left() {
            out.push_str("<w:rtl/>");
        }
        let _ = write!(out, "<w:lang w:val=\"en-US\" w:bidi=\"{tag}\"/>");
    }
    out.push_str("</w:rPr>");
}

fn write_run(out: &mut String, text: &str, style: &convert_structure::model::Style) {
    if text.is_empty() {
        return;
    }
    let complex = complex_script(text).unwrap_or(Script::Latin);
    out.push_str("<w:r>");
    run_properties(out, style, complex);
    for (at, piece) in text.split('\n').enumerate() {
        if at > 0 {
            out.push_str("<w:br/>");
        }
        for (k, part) in piece.split('\t').enumerate() {
            if k > 0 {
                out.push_str("<w:tab/>");
            }
            if !part.is_empty() {
                let (escaped, _) = escape(part);
                let _ = write!(out, "<w:t xml:space=\"preserve\">{escaped}</w:t>");
            }
        }
    }
    out.push_str("</w:r>");
}

fn body_family(document: &Document) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for page in &document.pages {
        for block in &page.blocks {
            if let Block::Paragraph(paragraph) = block {
                for run in &paragraph.runs {
                    if run.style.legacy || run.style.family.is_empty() {
                        continue;
                    }
                    let n = run.text.chars().count();
                    match counts.iter_mut().find(|(f, _)| *f == run.style.family) {
                        Some(entry) => entry.1 += n,
                        None => counts.push((run.style.family.clone(), n)),
                    }
                }
            }
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map_or_else(
        || convert_ooxml::LATIN_FACE.to_owned(),
        |(family, _)| family,
    )
}

fn styles(document: &Document) -> String {
    let body = if document.body_size > 0.0 {
        document.body_size
    } else {
        11.0
    };
    let (family, _) = escape(&body_family(document));
    let size = half_points(body);
    let lao = convert_ooxml::LAO_FACE;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    let _ = write!(
        out,
        "<w:styles xmlns:w=\"{W_NS}\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"{family}\" w:hAnsi=\"{family}\" w:eastAsia=\"{family}\" w:cs=\"{lao}\"/><w:sz w:val=\"{size}\"/><w:szCs w:val=\"{size}\"/><w:lang w:val=\"en-US\" w:bidi=\"lo-LA\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>"
    );
    out.push_str("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>");
    for level in 1..=6_u8 {
        let _ = write!(
            out,
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:uiPriority w:val=\"9\"/><w:qFormat/><w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:bCs/></w:rPr></w:style>",
            level - 1
        );
    }
    out.push_str("<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\"><w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:uiPriority w:val=\"34\"/><w:qFormat/><w:pPr><w:ind w:left=\"720\"/></w:pPr></w:style>");
    out.push_str("<w:style w:type=\"paragraph\" w:styleId=\"Header\"><w:name w:val=\"header\"/><w:basedOn w:val=\"Normal\"/></w:style>");
    out.push_str("<w:style w:type=\"paragraph\" w:styleId=\"Footer\"><w:name w:val=\"footer\"/><w:basedOn w:val=\"Normal\"/></w:style>");
    out.push_str("<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    out.push_str("<w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/><w:basedOn w:val=\"TableNormal\"/><w:tblPr><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders><w:tblCellMar><w:left w:w=\"57\" w:type=\"dxa\"/><w:right w:w=\"57\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    out.push_str("</w:styles>");
    out
}

fn numbering(lists: &Lists) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    let _ = write!(out, "<w:numbering xmlns:w=\"{W_NS}\">");
    let formats = [
        ("bullet", "\u{2022}"),
        ("decimal", "%1."),
        ("lowerLetter", "%1)"),
        ("upperLetter", "%1."),
    ];
    for (id, (format, text)) in formats.iter().enumerate() {
        let _ = write!(
            out,
            "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"
        );
        for level in 0..3_u32 {
            let text = text.replace("%1", &format!("%{}", level + 1));
            let _ = write!(
                out,
                "<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{format}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>",
                720 + 360 * level
            );
        }
        out.push_str("</w:abstractNum>");
    }
    for (at, (abstract_id, start)) in lists.instances.iter().enumerate() {
        let _ = write!(
            out,
            "<w:num w:numId=\"{}\"><w:abstractNumId w:val=\"{abstract_id}\"/><w:lvlOverride w:ilvl=\"0\"><w:startOverride w:val=\"{start}\"/></w:lvlOverride></w:num>",
            at + 1
        );
    }
    out.push_str("</w:numbering>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_structure::model::{Cell, ListItem, Page, Rect, Run, Style, TableRow};

    fn para(text: &str, role: Role) -> Paragraph {
        Paragraph {
            role,
            runs: vec![Run {
                text: text.into(),
                style: Style {
                    family: "Saysettha OT".into(),
                    size: 12.0,
                    ..Style::default()
                },
            }],
            frame: Rect::new(72.0, 72.0, 300.0, 90.0),
            lines: 1,
            ..Paragraph::default()
        }
    }

    #[test]
    fn a_document_with_every_block_is_a_valid_package() {
        let table = Table {
            frame: Rect::new(72.0, 100.0, 272.0, 140.0),
            columns: vec![100.0, 100.0],
            rows: vec![
                TableRow {
                    height: 20.0,
                    cells: vec![
                        Cell {
                            paragraphs: vec![para("ຫົວ", Role::Body)],
                            span: (2, 1),
                            ..Cell::default()
                        },
                        Cell {
                            covered: true,
                            span: (1, 1),
                            ..Cell::default()
                        },
                    ],
                },
                TableRow {
                    height: 20.0,
                    cells: vec![
                        Cell {
                            paragraphs: vec![para("a", Role::Body)],
                            span: (1, 1),
                            ..Cell::default()
                        },
                        Cell {
                            paragraphs: vec![para("b & <c>", Role::Body)],
                            span: (1, 1),
                            ..Cell::default()
                        },
                    ],
                },
            ],
        };
        let document = Document {
            pages: vec![Page {
                width: 595.0,
                height: 842.0,
                blocks: vec![
                    Block::Paragraph(para("ບົດທີ 1", Role::Heading(1))),
                    Block::Paragraph(para(
                        "ລາຍການ",
                        Role::ListItem(ListItem {
                            marker: "1.".into(),
                            kind: ListKind::Decimal(1),
                            level: 0,
                        }),
                    )),
                    Block::Table(table),
                ],
                ..Page::default()
            }],
            body_size: 12.0,
        };
        let bytes = write(&document).unwrap();
        assert_eq!(&bytes[0..4], b"PK\x03\x04");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("word/document.xml"));
    }

    #[test]
    fn lao_runs_carry_their_complex_script_twins() {
        let mut out = String::new();
        write_run(
            &mut out,
            "ພາສາລາວ",
            &Style {
                size: 14.0,
                bold: true,
                ..Style::default()
            },
        );
        assert!(out.contains("w:bidi=\"lo-LA\""));
        assert!(out.contains("<w:bCs/>"));
        assert!(out.contains("<w:szCs w:val=\"28\"/>"));
        assert!(out.contains(&format!("w:cs=\"{}\"", convert_ooxml::LAO_FACE)));
    }
}
