use std::fmt::Write as _;
use std::sync::Arc;

use convert_ooxml::{Package, Rels, hex, rel};
use convert_structure::model::{Block, Document, Paragraph, Table};
use convert_structure::script::{self, Script};
use convert_structure::{FontProvider, Options, Output, Request};
use convert_xml::escape;

pub use convert_structure;

const MAIN_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WORKBOOK: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const STYLES: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const SHARED: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";

pub const OPTIONS: Options = Options {
    pictures: false,
    tables: true,
};

pub fn convert(
    pdf: Vec<u8>,
    request: &Request,
    fonts: Option<Arc<dyn FontProvider>>,
    progress: &mut dyn FnMut(usize, usize) -> bool,
) -> Result<Output, String> {
    let document = convert_structure::read(pdf, request, fonts, &OPTIONS, progress, &mut |_| {})?;
    write_output(&document, request)
}

pub fn write_output(document: &Document, _request: &Request) -> Result<Output, String> {
    let (main, dropped) = write(document).map_err(|error| error.to_string())?;
    Ok(Output {
        main,
        attachments: Vec::new(),
        notes: Vec::new(),
        dropped,
    })
}

convert_wasm::export_tool!(crate::OPTIONS, crate::write_output, "xlsx");

#[must_use]
pub fn number_of(text: &str) -> Option<(f64, bool)> {
    let mut t = text.trim();
    if t.is_empty() || t.len() > 32 {
        return None;
    }
    let mut negative = false;
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        negative = true;
        t = inner.trim();
    }
    let percent = t.ends_with('%');
    if percent {
        t = t[..t.len() - 1].trim_end();
    }
    if let Some(rest) = t.strip_prefix(['-', '\u{2212}']) {
        negative = !negative;
        t = rest;
    } else if let Some(rest) = t.strip_prefix('+') {
        t = rest;
    }
    let (whole, fraction) = match t.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (t, None),
    };
    if whole.is_empty() && fraction.is_none() {
        return None;
    }
    let groups: Vec<&str> = whole.split(',').collect();
    if groups.len() > 1
        && (groups[0].is_empty() || groups[0].len() > 3 || groups[1..].iter().any(|g| g.len() != 3))
    {
        return None;
    }
    let digits: String = groups.concat();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = fraction
        && (f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    let significant = digits.trim_start_matches('0').len() + fraction.map_or(0, str::len);
    if significant > 15 || (digits.is_empty() && fraction.is_none()) {
        return None;
    }
    let literal = format!(
        "{}{}.{}",
        if negative { "-" } else { "" },
        if digits.is_empty() { "0" } else { &digits },
        fraction.unwrap_or("0")
    );
    let value: f64 = literal.parse().ok()?;
    Some((if percent { value / 100.0 } else { value }, percent))
}

#[must_use]
pub fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + u8::try_from(index % 26).unwrap_or(0));
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

#[derive(Default)]
struct Strings {
    list: Vec<String>,
    index: std::collections::HashMap<String, usize>,
    uses: usize,
}

impl Strings {
    fn id(&mut self, text: &str) -> usize {
        self.uses += 1;
        if let Some(id) = self.index.get(text) {
            return *id;
        }
        let id = self.list.len();
        self.list.push(text.to_owned());
        self.index.insert(text.to_owned(), id);
        id
    }
}

#[derive(Default)]
struct Formats {
    list: Vec<(bool, Option<[u8; 3]>, bool, bool)>,
    fills: Vec<[u8; 3]>,
}

impl Formats {
    fn id(&mut self, bold: bool, fill: Option<[u8; 3]>, percent: bool, bordered: bool) -> usize {
        if let Some(fill) = fill
            && !self.fills.contains(&fill)
        {
            self.fills.push(fill);
        }
        let key = (bold, fill, percent, bordered);
        if let Some(at) = self.list.iter().position(|k| *k == key) {
            return at + 1;
        }
        self.list.push(key);
        self.list.len()
    }
}

fn cell_text(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|p| p.text_with_marker().trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn has_header(table: &Table) -> bool {
    let Some(first) = table.rows.first() else {
        return false;
    };
    let cells: Vec<_> = first.cells.iter().filter(|c| !c.covered).collect();
    let texts: Vec<String> = cells.iter().map(|c| cell_text(&c.paragraphs)).collect();
    if texts.iter().all(String::is_empty) || texts.iter().any(|t| number_of(t).is_some()) {
        return false;
    }
    let set_apart = cells.iter().any(|c| c.fill.is_some())
        || cells
            .iter()
            .filter(|c| !c.paragraphs.is_empty())
            .all(|c| c.paragraphs.iter().all(Paragraph::all_bold));
    let numbers_below = table.rows.iter().skip(1).any(|row| {
        row.cells
            .iter()
            .any(|c| !c.covered && number_of(&cell_text(&c.paragraphs)).is_some())
    });
    table.rows.len() > 1 && (set_apart || numbers_below)
}

struct Sheet {
    name: String,
    xml: String,
}

fn sheet_of(table: &Table, strings: &mut Strings, formats: &mut Formats) -> String {
    let header = has_header(table);
    let mut out = String::with_capacity(4096);
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"{MAIN_NS}\" xmlns:r=\"{R_NS}\">"
    );
    if header {
        out.push_str("<sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>");
    }
    out.push_str("<cols>");
    for (at, width) in table.columns.iter().enumerate() {
        let chars = (width / 5.25).clamp(3.0, 80.0);
        let _ = write!(
            out,
            "<col min=\"{0}\" max=\"{0}\" width=\"{chars:.2}\" customWidth=\"1\"/>",
            at + 1
        );
    }
    out.push_str("</cols><sheetData>");
    let mut merges = Vec::new();
    for (r, row) in table.rows.iter().enumerate() {
        let _ = write!(out, "<row r=\"{}\">", r + 1);
        for (c, cell) in row.cells.iter().enumerate() {
            let reference = format!("{}{}", column_name(c), r + 1);
            let bold_row = header && r == 0;
            if cell.covered {
                let style = formats.id(false, None, false, true);
                let _ = write!(out, "<c r=\"{reference}\" s=\"{style}\"/>");
                continue;
            }
            if cell.span.0 > 1 || cell.span.1 > 1 {
                merges.push(format!(
                    "{reference}:{}{}",
                    column_name(c + cell.span.0 - 1),
                    r + cell.span.1
                ));
            }
            let text = cell_text(&cell.paragraphs);
            let bold = bold_row
                || (!cell.paragraphs.is_empty() && cell.paragraphs.iter().all(Paragraph::all_bold));
            if text.is_empty() {
                let style = formats.id(bold, cell.fill, false, true);
                let _ = write!(out, "<c r=\"{reference}\" s=\"{style}\"/>");
            } else if let Some((value, percent)) = number_of(&text).filter(|_| !bold_row) {
                let style = formats.id(bold, cell.fill, percent, true);
                let _ = write!(out, "<c r=\"{reference}\" s=\"{style}\"><v>{value}</v></c>");
            } else {
                let style = formats.id(bold, cell.fill, false, true);
                let id = strings.id(&text);
                let _ = write!(
                    out,
                    "<c r=\"{reference}\" s=\"{style}\" t=\"s\"><v>{id}</v></c>"
                );
            }
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData>");
    if !merges.is_empty() {
        let _ = write!(out, "<mergeCells count=\"{}\">", merges.len());
        for merge in merges {
            let _ = write!(out, "<mergeCell ref=\"{merge}\"/>");
        }
        out.push_str("</mergeCells>");
    }
    out.push_str("</worksheet>");
    out
}

fn text_sheet(document: &Document, strings: &mut Strings) -> String {
    let mut out = String::with_capacity(4096);
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<worksheet xmlns=\"{MAIN_NS}\" xmlns:r=\"{R_NS}\"><cols><col min=\"1\" max=\"1\" width=\"100\" customWidth=\"1\"/></cols><sheetData>"
    );
    let mut r = 0;
    for page in &document.pages {
        for block in &page.blocks {
            let Block::Paragraph(paragraph) = block else {
                continue;
            };
            let text = paragraph.text_with_marker();
            if text.trim().is_empty() {
                continue;
            }
            r += 1;
            if let Some((value, _)) = number_of(&text) {
                let _ = write!(out, "<row r=\"{r}\"><c r=\"A{r}\"><v>{value}</v></c></row>");
            } else {
                let id = strings.id(text.trim());
                let _ = write!(
                    out,
                    "<row r=\"{r}\"><c r=\"A{r}\" t=\"s\"><v>{id}</v></c></row>"
                );
            }
        }
    }
    out.push_str("</sheetData></worksheet>");
    out
}

fn default_font(document: &Document) -> &'static str {
    let mut text = String::new();
    for page in &document.pages {
        for block in &page.blocks {
            if let Block::Paragraph(p) = block {
                text.push_str(&p.text());
            }
        }
    }
    match script::dominant(&text) {
        Script::Lao => convert_ooxml::LAO_FACE,
        Script::Thai => convert_ooxml::THAI_FACE,
        _ => "Calibri",
    }
}

fn styles(formats: &Formats, font: &str) -> String {
    let mut out = String::new();
    let (font, _) = escape(font);
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<styleSheet xmlns=\"{MAIN_NS}\"><fonts count=\"2\"><font><sz val=\"11\"/><name val=\"{font}\"/></font><font><b/><sz val=\"11\"/><name val=\"{font}\"/></font></fonts>"
    );
    let _ = write!(
        out,
        "<fills count=\"{}\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill>",
        2 + formats.fills.len()
    );
    for fill in &formats.fills {
        let _ = write!(
            out,
            "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{}\"/><bgColor indexed=\"64\"/></patternFill></fill>",
            hex(*fill)
        );
    }
    out.push_str("</fills><borders count=\"2\"><border><left/><right/><top/><bottom/><diagonal/></border><border><left style=\"thin\"><color auto=\"1\"/></left><right style=\"thin\"><color auto=\"1\"/></right><top style=\"thin\"><color auto=\"1\"/></top><bottom style=\"thin\"><color auto=\"1\"/></bottom><diagonal/></border></borders>");
    out.push_str("<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>");
    let _ = write!(
        out,
        "<cellXfs count=\"{}\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>",
        formats.list.len() + 1
    );
    for (bold, fill, percent, bordered) in &formats.list {
        let fill_id = fill
            .and_then(|f| formats.fills.iter().position(|x| *x == f))
            .map_or(0, |at| at + 2);
        let _ = write!(
            out,
            "<xf numFmtId=\"{}\" fontId=\"{}\" fillId=\"{fill_id}\" borderId=\"{}\" xfId=\"0\" applyFont=\"1\" applyBorder=\"1\" applyAlignment=\"1\"{}{}><alignment vertical=\"top\" wrapText=\"1\"/></xf>",
            if *percent { 10 } else { 0 },
            u8::from(*bold),
            u8::from(*bordered),
            if fill_id > 0 { " applyFill=\"1\"" } else { "" },
            if *percent {
                " applyNumberFormat=\"1\""
            } else {
                ""
            },
        );
    }
    out.push_str("</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>");
    out
}

pub fn write(document: &Document) -> Result<(Vec<u8>, String), convert_zip::ZipError> {
    let mut strings = Strings::default();
    let mut formats = Formats::default();
    let mut sheets = Vec::new();
    let mut dropped = String::new();
    for page in &document.pages {
        for block in &page.blocks {
            match block {
                Block::Table(table) => {
                    let name = format!("Table {} (p{})", sheets.len() + 1, page.index + 1);
                    let xml = sheet_of(table, &mut strings, &mut formats);
                    sheets.push(Sheet { name, xml });
                }
                Block::Paragraph(p) => {
                    dropped.push_str(&p.text_with_marker());
                    dropped.push('\n');
                }
                Block::Picture(_) => {}
            }
        }
    }
    if sheets.is_empty() {
        dropped.clear();
        let xml = text_sheet(document, &mut strings);
        sheets.push(Sheet {
            name: "Text".to_owned(),
            xml,
        });
    }
    dropped.push_str(&document.running_heads());

    let mut package = Package::new();
    let mut rels = Rels::new();
    let mut workbook = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<workbook xmlns=\"{MAIN_NS}\" xmlns:r=\"{R_NS}\"><bookViews><workbookView/></bookViews><sheets>"
    );
    for (at, sheet) in sheets.iter().enumerate() {
        let target = format!("worksheets/sheet{}.xml", at + 1);
        let id = rels.add(rel::WORKSHEET, &target);
        let (name, _) = escape(&sheet.name);
        let _ = write!(
            workbook,
            "<sheet name=\"{name}\" sheetId=\"{}\" r:id=\"{id}\"/>",
            at + 1
        );
    }
    workbook.push_str("</sheets></workbook>");
    rels.add(rel::STYLES, "styles.xml");
    rels.add(rel::SHARED_STRINGS, "sharedStrings.xml");
    package.part("xl/workbook.xml", WORKBOOK, workbook.into_bytes());
    for (at, sheet) in sheets.into_iter().enumerate() {
        package.part(
            &format!("xl/worksheets/sheet{}.xml", at + 1),
            WORKSHEET,
            sheet.xml.into_bytes(),
        );
    }
    package.part(
        "xl/styles.xml",
        STYLES,
        styles(&formats, default_font(document)).into_bytes(),
    );
    let mut shared = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<sst xmlns=\"{MAIN_NS}\" count=\"{}\" uniqueCount=\"{}\">",
        strings.uses,
        strings.list.len()
    );
    for text in &strings.list {
        let (text, _) = escape(text);
        let _ = write!(shared, "<si><t>{text}</t></si>");
    }
    shared.push_str("</sst>");
    package.part("xl/sharedStrings.xml", SHARED, shared.into_bytes());
    package.file("xl/_rels/workbook.xml.rels", rels.xml().into_bytes());
    package.properties_and_root("xl/workbook.xml", "", "PanPDF");
    Ok((package.finish()?, dropped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_not_numbers() {
        assert_eq!(number_of("1,234.50"), Some((1234.5, false)));
        assert_eq!(number_of("-12"), Some((-12.0, false)));
        assert_eq!(number_of("(1,200)"), Some((-1200.0, false)));
        assert_eq!(number_of("45%"), Some((0.45, true)));
        assert_eq!(number_of("0.5"), Some((0.5, false)));
        assert_eq!(number_of(".5"), Some((0.5, false)));
        assert_eq!(number_of("0"), Some((0.0, false)));
        for text in [
            "007",
            "1,23",
            "12a",
            "",
            "໑໒",
            "1.2.3",
            "12345678901234567",
            "020 555 1234",
            "1,2345",
        ] {
            assert_eq!(number_of(text), None, "{text}");
        }
    }

    #[test]
    fn column_names() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }
}
