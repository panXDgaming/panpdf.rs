mod theme;

use std::fmt::Write as _;
use std::sync::Arc;

use convert_ooxml::{Package, Rels, emu, faces, hex, rel};
use convert_structure::model::{
    Align, Block, Document, ListKind, Page, Paragraph, Picture, Rect, Role, Shift, Table,
};
use convert_structure::script::{self, Script};
use convert_structure::{FontProvider, Options, Output, Request};
use convert_xml::escape;
use theme::{A_NS, P_NS, R_NS};

pub use convert_structure;

const PRESENTATION: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const SLIDE: &str = "application/vnd.openxmlformats-officedocument.presentationml.slide+xml";
const MASTER: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml";
const LAYOUT: &str = "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml";
const THEME: &str = "application/vnd.openxmlformats-officedocument.theme+xml";
const PRES_PROPS: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presProps+xml";

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
                stored += 1;
                let name = format!("image{stored}.{}", picture.format.extension());
                package.file(
                    &format!("ppt/media/{name}"),
                    std::mem::take(&mut picture.data),
                );
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
        notes: Vec::new(),
        dropped: String::new(),
    })
}

pub fn write_output(document: &Document, _request: &Request) -> Result<Output, String> {
    Ok(Output {
        main: write(document).map_err(|error| error.to_string())?,
        attachments: Vec::new(),
        notes: Vec::new(),
        dropped: String::new(),
    })
}

convert_wasm::export_tool!(
    convert_structure::Options::default(),
    crate::write_output,
    "pptx"
);

#[derive(Clone, Copy)]
struct Place {
    scale: f64,
    dx: f64,
    dy: f64,
}

impl Place {
    fn rect(&self, rect: &Rect) -> (i64, i64, i64, i64) {
        (
            emu(self.dx + rect.x0 * self.scale),
            emu(self.dy + rect.y0 * self.scale),
            emu((rect.width() * self.scale).max(1.0)),
            emu((rect.height() * self.scale).max(1.0)),
        )
    }
}

struct Slide {
    xml: String,
    rels: Rels,
    next_id: usize,
}

fn boxes(blocks: &[Block]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (at, block) in blocks.iter().enumerate() {
        let Block::Paragraph(paragraph) = block else {
            continue;
        };
        let joins = groups
            .last()
            .and_then(|group| group.last())
            .is_some_and(|&last| {
                if last + 1 != at {
                    return false;
                }
                let Block::Paragraph(previous) = &blocks[last] else {
                    return false;
                };
                let (a, b) = (previous.frame, paragraph.frame);
                let gap = b.y0 - a.y1;
                let em = previous.size().max(paragraph.size()).max(4.0);
                let overlap = a.x1.min(b.x1) - a.x0.max(b.x0);
                gap > -0.5 * em && gap < 1.2 * em && overlap > 0.5 * a.width().min(b.width())
            });
        if joins {
            if let Some(group) = groups.last_mut() {
                group.push(at);
            }
        } else {
            groups.push(vec![at]);
        }
    }
    groups
}

fn run_xml(out: &mut String, text: &str, style: &convert_structure::model::Style, scale: f64) {
    if text.is_empty() {
        return;
    }
    let complex = text
        .chars()
        .map(Script::of)
        .find(|s| s.is_complex())
        .unwrap_or(Script::Latin);
    let lang = script::dominant(text).language().unwrap_or("en-US");
    let slots = faces(style, complex);
    let size = ((style.size * scale * 100.0).round() as i64).clamp(100, 400_000);
    let _ = write!(out, "<a:r><a:rPr lang=\"{lang}\" sz=\"{size}\"");
    if style.bold {
        out.push_str(" b=\"1\"");
    }
    if style.italic {
        out.push_str(" i=\"1\"");
    }
    if style.underline {
        out.push_str(" u=\"sng\"");
    }
    match style.baseline {
        Shift::Up => out.push_str(" baseline=\"30000\""),
        Shift::Down => out.push_str(" baseline=\"-25000\""),
        Shift::None => {}
    }
    let (latin, _) = escape(&slots.latin);
    let (cs, _) = escape(&slots.complex);
    let _ = write!(
        out,
        " dirty=\"0\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill><a:latin typeface=\"{latin}\"/><a:ea typeface=\"{latin}\"/><a:cs typeface=\"{cs}\"/></a:rPr><a:t>",
        hex(style.color)
    );
    let (text, _) = escape(text);
    out.push_str(&text);
    out.push_str("</a:t></a:r>");
}

fn paragraph_xml(out: &mut String, paragraph: &Paragraph, space_before: f64, scale: f64) {
    let algn = match paragraph.align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
        Align::Justify => "just",
    };
    let (bullet, margin) = match &paragraph.role {
        Role::ListItem(item) => {
            let indent = emu(18.0 * scale);
            let margin = emu((18.0 + 18.0 * f64::from(item.level)) * scale);
            let bullet = match item.kind {
                ListKind::Bullet => format!(
                    "<a:buFont typeface=\"Arial\"/><a:buChar char=\"{}\"/>",
                    escape(&item.marker).0
                ),
                ListKind::Decimal(n) => format!(
                    "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\" startAt=\"{n}\"/>"
                ),
                ListKind::LowerLetter(n) => format!(
                    "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"alphaLcParenR\" startAt=\"{n}\"/>"
                ),
                ListKind::UpperLetter(n) => format!(
                    "<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"alphaUcPeriod\" startAt=\"{n}\"/>"
                ),
                ListKind::Kept => String::from("<a:buNone/>"),
            };
            (bullet, format!(" marL=\"{margin}\" indent=\"-{indent}\""))
        }
        _ => ("<a:buNone/>".to_owned(), String::new()),
    };
    let _ = write!(out, "<a:p><a:pPr{margin} algn=\"{algn}\">");
    if paragraph.lines > 1 && paragraph.pitch > paragraph.size() * 1.05 {
        let _ = write!(
            out,
            "<a:lnSpc><a:spcPts val=\"{}\"/></a:lnSpc>",
            ((paragraph.pitch * scale * 100.0).round() as i64).clamp(0, 158_400)
        );
    }
    if space_before > 0.5 {
        let _ = write!(
            out,
            "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>",
            ((space_before * scale * 100.0).round() as i64).clamp(0, 158_400)
        );
    }
    out.push_str(&bullet);
    out.push_str("</a:pPr>");
    for run in &paragraph.runs {
        run_xml(out, &run.text, &run.style, scale);
    }
    out.push_str("</a:p>");
}

impl Slide {
    fn id(&mut self) -> usize {
        self.next_id += 1;
        self.next_id
    }

    fn text_box(&mut self, paragraphs: &[&Paragraph], place: Place) {
        let frame = paragraphs
            .iter()
            .skip(1)
            .fold(paragraphs[0].frame, |r, p| r.union(&p.frame));
        let widened = Rect::new(
            frame.x0,
            frame.y0,
            frame.x1 + 0.04 * frame.width() + 2.0,
            frame.y1,
        );
        let (x, y, cx, cy) = place.rect(&widened);
        let id = self.id();
        let _ = write!(
            self.xml,
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Text {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/></p:spPr><p:txBody><a:bodyPr wrap=\"square\" lIns=\"0\" tIns=\"0\" rIns=\"0\" bIns=\"0\" rtlCol=\"0\"><a:noAutofit/></a:bodyPr><a:lstStyle/>"
        );
        let mut bottom = paragraphs[0].frame.y0;
        for (at, paragraph) in paragraphs.iter().enumerate() {
            let before = if at == 0 {
                0.0
            } else {
                (paragraph.frame.y0 - bottom).clamp(0.0, 48.0)
            };
            paragraph_xml(&mut self.xml, paragraph, before, place.scale);
            bottom = paragraph.frame.y1;
        }
        self.xml.push_str("</p:txBody></p:sp>");
    }

    fn picture(&mut self, picture: &Picture, place: Place, media: &mut Vec<(String, Vec<u8>)>) {
        let name = match &picture.stored {
            Some(name) => name.clone(),
            None => {
                let name = format!("picture{}.{}", media.len() + 1, picture.format.extension());
                media.push((name.clone(), picture.data.clone()));
                name
            }
        };
        let rel_id = self.rels.add(rel::IMAGE, &format!("../media/{name}"));
        let (x, y, cx, cy) = place.rect(&picture.frame);
        let id = self.id();
        let _ = write!(
            self.xml,
            "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Picture {id}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rel_id}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"
        );
    }

    fn table(&mut self, table: &Table, place: Place) {
        let (x, y, cx, cy) = place.rect(&table.frame);
        let id = self.id();
        let _ = write!(
            self.xml,
            "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr/><a:tblGrid>"
        );
        for width in &table.columns {
            let _ = write!(self.xml, "<a:gridCol w=\"{}\"/>", emu(width * place.scale));
        }
        self.xml.push_str("</a:tblGrid>");
        let line = "<a:solidFill><a:srgbClr val=\"000000\"/></a:solidFill>";
        for (r, row) in table.rows.iter().enumerate() {
            let _ = write!(self.xml, "<a:tr h=\"{}\">", emu(row.height * place.scale));
            for (c, cell) in row.cells.iter().enumerate() {
                let mut attributes = String::new();
                if cell.covered {
                    let from_above = (0..r).rev().any(|r2| {
                        let owner = &table.rows[r2].cells[c];
                        !owner.covered && owner.span.1 > r - r2
                    }) || (0..r).rev().any(|r2| {
                        (0..c).any(|c2| {
                            let owner = &table.rows[r2].cells[c2];
                            !owner.covered && owner.span.1 > r - r2 && owner.span.0 > c - c2
                        })
                    });
                    attributes.push_str(if from_above {
                        " vMerge=\"1\""
                    } else {
                        " hMerge=\"1\""
                    });
                    if from_above
                        && (0..c).rev().any(|c2| {
                            let owner = &row.cells[c2];
                            !owner.covered && owner.span.0 > c - c2
                        })
                    {
                        attributes.push_str(" hMerge=\"1\"");
                    }
                } else {
                    if cell.span.0 > 1 {
                        let _ = write!(attributes, " gridSpan=\"{}\"", cell.span.0);
                    }
                    if cell.span.1 > 1 {
                        let _ = write!(attributes, " rowSpan=\"{}\"", cell.span.1);
                    }
                }
                let _ = write!(
                    self.xml,
                    "<a:tc{attributes}><a:txBody><a:bodyPr/><a:lstStyle/>"
                );
                if cell.paragraphs.is_empty() || cell.covered {
                    self.xml
                        .push_str("<a:p><a:endParaRPr lang=\"en-US\" dirty=\"0\"/></a:p>");
                } else {
                    for paragraph in &cell.paragraphs {
                        paragraph_xml(&mut self.xml, paragraph, 0.0, place.scale);
                    }
                }
                let _ = write!(
                    self.xml,
                    "</a:txBody><a:tcPr marL=\"{m}\" marR=\"{m}\" marT=\"{m}\" marB=\"{m}\"><a:lnL w=\"6350\">{line}</a:lnL><a:lnR w=\"6350\">{line}</a:lnR><a:lnT w=\"6350\">{line}</a:lnT><a:lnB w=\"6350\">{line}</a:lnB>",
                    m = emu(2.0 * place.scale)
                );
                match cell.fill {
                    Some(fill) => {
                        let _ = write!(
                            self.xml,
                            "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
                            hex(fill)
                        );
                    }
                    None => self.xml.push_str("<a:noFill/>"),
                }
                self.xml.push_str("</a:tcPr></a:tc>");
            }
            self.xml.push_str("</a:tr>");
        }
        self.xml
            .push_str("</a:tbl></a:graphicData></a:graphic></p:graphicFrame>");
    }
}

pub fn write(document: &Document) -> Result<Vec<u8>, convert_zip::ZipError> {
    write_into(Package::new(), document)
}

pub fn write_into(
    mut package: Package,
    document: &Document,
) -> Result<Vec<u8>, convert_zip::ZipError> {
    let pages: Vec<_> = document
        .pages
        .iter()
        .filter(|p| p.refused.is_none() && p.width > 0.0)
        .collect();
    let (width, height) = pages
        .first()
        .map_or((720.0, 540.0), |p| (p.width, p.height));
    let slide_w = width.clamp(72.0, 4032.0);
    let slide_h = height.clamp(72.0, 4032.0);
    package.default_type("png", "image/png");
    package.default_type("jpeg", "image/jpeg");
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    let mut presentation_rels = Rels::new();
    presentation_rels.add(rel::SLIDE_MASTER, "slideMasters/slideMaster1.xml");
    presentation_rels.add(rel::PRES_PROPS, "presProps.xml");
    presentation_rels.add(rel::THEME, "theme/theme1.xml");
    let mut slide_ids = String::new();
    for (number, page) in pages.iter().enumerate() {
        let scale = (slide_w / page.width).min(slide_h / page.height);
        let place = Place {
            scale,
            dx: (slide_w - page.width * scale) / 2.0,
            dy: (slide_h - page.height * scale) / 2.0,
        };
        let mut slide = Slide {
            xml: format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sld xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\"><p:cSld><p:spTree>{}",
                theme::tree_start()
            ),
            rels: Rels::new(),
            next_id: 1,
        };
        slide
            .rels
            .add(rel::SLIDE_LAYOUT, "../slideLayouts/slideLayout1.xml");
        for block in &page.blocks {
            if let Block::Picture(picture) = block
                && picture.background
            {
                slide.picture(picture, place, &mut media);
            }
        }
        let heads: Vec<&Paragraph> = page.header.iter().chain(&page.footer).collect();
        for head in heads {
            slide.text_box(&[head], place);
        }
        let groups = boxes(&page.blocks);
        let mut group_of = vec![None; page.blocks.len()];
        for (g, group) in groups.iter().enumerate() {
            for &at in group {
                group_of[at] = Some(g);
            }
        }
        for (at, block) in page.blocks.iter().enumerate() {
            match block {
                Block::Paragraph(_) => {
                    let Some(g) = group_of[at] else { continue };
                    if groups[g][0] != at {
                        continue;
                    }
                    let paragraphs: Vec<&Paragraph> = groups[g]
                        .iter()
                        .filter_map(|&i| match &page.blocks[i] {
                            Block::Paragraph(p) => Some(p),
                            _ => None,
                        })
                        .collect();
                    slide.text_box(&paragraphs, place);
                }
                Block::Table(table) => slide.table(table, place),
                Block::Picture(picture) => {
                    if !picture.background {
                        slide.picture(picture, place, &mut media);
                    }
                }
            }
        }
        slide.xml.push_str(
            "</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
        );
        let n = number + 1;
        package.part(
            &format!("ppt/slides/slide{n}.xml"),
            SLIDE,
            slide.xml.into_bytes(),
        );
        package.file(
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            slide.rels.xml().into_bytes(),
        );
        let id = presentation_rels.add(rel::SLIDE, &format!("slides/slide{n}.xml"));
        let _ = write!(slide_ids, "<p:sldId id=\"{}\" r:id=\"{id}\"/>", 255 + n);
    }
    let presentation = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:presentation xmlns:a=\"{A_NS}\" xmlns:r=\"{R_NS}\" xmlns:p=\"{P_NS}\" saveSubsetFonts=\"1\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst>{}<p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>",
        if slide_ids.is_empty() {
            String::new()
        } else {
            format!("<p:sldIdLst>{slide_ids}</p:sldIdLst>")
        },
        emu(slide_w),
        emu(slide_h)
    );
    package.part(
        "ppt/presentation.xml",
        PRESENTATION,
        presentation.into_bytes(),
    );
    package.file(
        "ppt/_rels/presentation.xml.rels",
        presentation_rels.xml().into_bytes(),
    );
    package.part(
        "ppt/presProps.xml",
        PRES_PROPS,
        theme::properties().into_bytes(),
    );
    package.part(
        "ppt/slideMasters/slideMaster1.xml",
        MASTER,
        theme::master().into_bytes(),
    );
    let mut master_rels = Rels::new();
    master_rels.add(rel::SLIDE_LAYOUT, "../slideLayouts/slideLayout1.xml");
    master_rels.add(rel::THEME, "../theme/theme1.xml");
    package.file(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        master_rels.xml().into_bytes(),
    );
    package.part(
        "ppt/slideLayouts/slideLayout1.xml",
        LAYOUT,
        theme::layout().into_bytes(),
    );
    let mut layout_rels = Rels::new();
    layout_rels.add(rel::SLIDE_MASTER, "../slideMasters/slideMaster1.xml");
    package.file(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        layout_rels.xml().into_bytes(),
    );
    package.part(
        "ppt/theme/theme1.xml",
        THEME,
        theme::theme(convert_ooxml::LAO_FACE).into_bytes(),
    );
    for (name, bytes) in media {
        package.file(&format!("ppt/media/{name}"), bytes);
    }
    package.properties_and_root("ppt/presentation.xml", "", "PanPDF");
    package.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use convert_structure::model::{Run, Style};

    fn para(y0: f64, y1: f64) -> Block {
        Block::Paragraph(Paragraph {
            runs: vec![Run {
                text: "ພາສາ".into(),
                style: Style {
                    size: 10.0,
                    ..Style::default()
                },
            }],
            frame: Rect::new(50.0, y0, 300.0, y1),
            lines: 1,
            ..Paragraph::default()
        })
    }

    #[test]
    fn paragraphs_that_stand_together_share_a_box() {
        let blocks = vec![para(100.0, 112.0), para(114.0, 126.0), para(200.0, 212.0)];
        assert_eq!(boxes(&blocks), vec![vec![0, 1], vec![2]]);
    }

    #[test]
    fn lao_runs_name_the_complex_script_face() {
        let mut out = String::new();
        run_xml(
            &mut out,
            "ພາສາ",
            &Style {
                size: 12.0,
                ..Style::default()
            },
            1.0,
        );
        assert!(out.contains("lang=\"lo-LA\""));
        assert!(out.contains("sz=\"1200\""));
        assert!(out.contains(&format!("<a:cs typeface=\"{}\"/>", convert_ooxml::LAO_FACE)));
    }
}
