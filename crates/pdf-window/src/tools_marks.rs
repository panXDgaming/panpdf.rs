use eframe::egui;

use pdf_convert::Tool;

const PAGE_WIDTH: f32 = 46.0;
const PAGE_HEIGHT: f32 = 54.0;
const INK: egui::Color32 = egui::Color32::from_rgb(0x1d, 0x24, 0x33);
const FOLD: egui::Color32 = egui::Color32::from_rgb(0xe7, 0xea, 0xf1);
const PAGE: egui::Color32 = egui::Color32::from_rgb(0xfd, 0xfd, 0xfe);

const PDF: egui::Color32 = egui::Color32::from_rgb(0xc2, 0x40, 0x3a);
const WORD: egui::Color32 = egui::Color32::from_rgb(0x2b, 0x57, 0x9a);
const EXCEL: egui::Color32 = egui::Color32::from_rgb(0x1f, 0x7a, 0x4d);
const POWERPOINT: egui::Color32 = egui::Color32::from_rgb(0xc4, 0x53, 0x2b);
const PICTURE: egui::Color32 = egui::Color32::from_rgb(0xb0, 0x7a, 0x0c);
const WEB: egui::Color32 = egui::Color32::from_rgb(0x7a, 0x4f, 0xc0);
const PLAIN: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x46, 0x55);
const LOCK: egui::Color32 = egui::Color32::from_rgb(0x1c, 0x5f, 0xa8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Badge {
    Letters(&'static str),
    Scan,
    Squeeze,
    Mend,
    Closed,
    Open,
    Signature,
    Blackout,
    Sides,
}

const fn badge_of(tool: Tool) -> (Badge, egui::Color32) {
    match tool {
        Tool::PdfToWord | Tool::WordToPdf => (Badge::Letters("W"), WORD),
        Tool::PdfToExcel | Tool::ExcelToPdf => (Badge::Letters("X"), EXCEL),
        Tool::PdfToPowerPoint | Tool::PowerPointToPdf => (Badge::Letters("P"), POWERPOINT),
        Tool::PdfToImage | Tool::ImageToPdf => (Badge::Letters("JPG"), PICTURE),
        Tool::PdfToHtml | Tool::HtmlToPdf => (Badge::Letters("</>"), WEB),
        Tool::PdfToMarkdown => (Badge::Letters("MD"), PLAIN),
        Tool::PdfToText => (Badge::Letters("TXT"), PLAIN),
        Tool::PdfToPdfA => (Badge::Letters("A"), PDF),
        Tool::ScanToPdf => (Badge::Scan, PICTURE),
        Tool::Compress => (Badge::Squeeze, PDF),
        Tool::Repair => (Badge::Mend, PDF),
        Tool::Ocr => (Badge::Letters("OCR"), PLAIN),
        Tool::Unlock => (Badge::Open, LOCK),
        Tool::Sign => (Badge::Signature, LOCK),
        Tool::Redact => (Badge::Blackout, INK),
        Tool::Compare => (Badge::Sides, PLAIN),
        Tool::Protect => (Badge::Closed, LOCK),
    }
}

pub(crate) fn mark_size(height: f32) -> egui::Vec2 {
    egui::vec2(height * PAGE_WIDTH / PAGE_HEIGHT, height)
}

pub(crate) fn mark(painter: &egui::Painter, at: egui::Pos2, height: f32, tool: Tool) {
    let (badge, colour) = badge_of(tool);
    draw(painter, at, height, badge, colour);
}

pub(crate) fn mark_for_file(painter: &egui::Painter, at: egui::Pos2, height: f32, name: &str) {
    let (badge, colour) = match name
        .rsplit_once('.')
        .map(|(_, ending)| ending.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => (Badge::Letters("PDF"), PDF),
        Some("docx") => (Badge::Letters("W"), WORD),
        Some("xlsx") => (Badge::Letters("X"), EXCEL),
        Some("pptx") => (Badge::Letters("P"), POWERPOINT),
        Some("jpg" | "jpeg") => (Badge::Letters("JPG"), PICTURE),
        Some("png") => (Badge::Letters("PNG"), PICTURE),
        Some("gif") => (Badge::Letters("GIF"), PICTURE),
        Some("html" | "htm" | "xhtml") => (Badge::Letters("</>"), WEB),
        Some("css") => (Badge::Letters("CSS"), WEB),
        Some("svg") => (Badge::Letters("SVG"), WEB),
        Some("md") => (Badge::Letters("MD"), PLAIN),
        Some("txt") => (Badge::Letters("TXT"), PLAIN),
        _ => (Badge::Letters(""), PLAIN),
    };
    draw(painter, at, height, badge, colour);
}

fn draw(painter: &egui::Painter, at: egui::Pos2, height: f32, badge: Badge, colour: egui::Color32) {
    let unit = height / PAGE_HEIGHT;
    let point = |x: f32, y: f32| egui::pos2(at.x + x * unit, at.y + y * unit);
    let weight = (2.4 * unit).max(1.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            point(5.0, 3.0),
            point(29.0, 3.0),
            point(41.0, 15.0),
            point(41.0, 51.0),
            point(5.0, 51.0),
        ],
        PAGE,
        egui::Stroke::new(weight, INK),
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![point(29.0, 3.0), point(41.0, 15.0), point(29.0, 15.0)],
        FOLD,
        egui::Stroke::new(weight * 0.8, INK),
    ));
    let (width, rise) = match badge {
        Badge::Letters(letters) => (
            match letters.chars().count() {
                0 | 1 => 20.0,
                2 => 26.0,
                _ => 31.0,
            },
            24.0,
        ),
        _ => (28.0, 24.0),
    };
    let left = 23.0 - width / 2.0;
    let area = egui::Rect::from_min_max(point(left, rise), point(left + width, rise + 22.0));
    painter.rect_filled(area, 3.0 * unit, colour);
    match badge {
        Badge::Letters(letters) => {
            painter.text(
                area.center() + egui::vec2(0.0, 0.5 * unit),
                egui::Align2::CENTER_CENTER,
                letters,
                egui::FontId::monospace(13.0 * unit),
                egui::Color32::WHITE,
            );
        }
        glyph => glyph_in(painter, area, glyph, (1.9 * unit).max(1.0)),
    }
}

fn glyph_in(painter: &egui::Painter, area: egui::Rect, badge: Badge, weight: f32) {
    let white = egui::Stroke::new(weight, egui::Color32::WHITE);
    let at = |x: f32, y: f32| {
        egui::pos2(
            area.left() + area.width() * x,
            area.top() + area.height() * y,
        )
    };
    let line = |points: &[(f32, f32)]| {
        let points: Vec<egui::Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
        painter.add(egui::Shape::line(points, white));
    };
    match badge {
        Badge::Letters(_) => {}
        Badge::Scan => {
            for (corner_x, corner_y, toward_x, toward_y) in [
                (0.22, 0.2, 1.0, 1.0),
                (0.78, 0.2, -1.0, 1.0),
                (0.22, 0.8, 1.0, -1.0),
                (0.78, 0.8, -1.0, -1.0),
            ] {
                line(&[
                    (corner_x + 0.16 * toward_x, corner_y),
                    (corner_x, corner_y),
                    (corner_x, corner_y + 0.28 * toward_y),
                ]);
            }
        }
        Badge::Squeeze => {
            line(&[(0.14, 0.24), (0.38, 0.5), (0.14, 0.76)]);
            line(&[(0.86, 0.24), (0.62, 0.5), (0.86, 0.76)]);
            line(&[(0.5, 0.3), (0.5, 0.7)]);
        }
        Badge::Mend => {
            line(&[(0.5, 0.16), (0.5, 0.84)]);
            line(&[(0.3, 0.5), (0.7, 0.5)]);
        }
        Badge::Closed | Badge::Open => {
            let lifted = badge == Badge::Open;
            painter.rect_filled(
                egui::Rect::from_min_max(at(0.3, 0.5), at(0.7, 0.9)),
                1.0,
                egui::Color32::WHITE,
            );
            if lifted {
                line(&[
                    (0.38, 0.5),
                    (0.38, 0.3),
                    (0.5, 0.14),
                    (0.62, 0.3),
                    (0.62, 0.36),
                ]);
            } else {
                line(&[
                    (0.38, 0.5),
                    (0.38, 0.3),
                    (0.5, 0.14),
                    (0.62, 0.3),
                    (0.62, 0.5),
                ]);
            }
        }
        Badge::Signature => {
            line(&[
                (0.14, 0.62),
                (0.26, 0.26),
                (0.38, 0.74),
                (0.5, 0.34),
                (0.62, 0.64),
                (0.86, 0.46),
            ]);
            line(&[(0.14, 0.88), (0.86, 0.88)]);
        }
        Badge::Blackout => {
            line(&[(0.16, 0.24), (0.84, 0.24)]);
            painter.rect_filled(
                egui::Rect::from_min_max(at(0.16, 0.42), at(0.84, 0.64)),
                1.0,
                egui::Color32::WHITE,
            );
            line(&[(0.16, 0.82), (0.58, 0.82)]);
        }
        Badge::Sides => {
            for (left, right) in [(0.12, 0.44), (0.56, 0.88)] {
                line(&[
                    (left, 0.2),
                    (right, 0.2),
                    (right, 0.8),
                    (left, 0.8),
                    (left, 0.2),
                ]);
            }
            line(&[(0.24, 0.42), (0.34, 0.42)]);
            line(&[(0.66, 0.42), (0.78, 0.42)]);
            line(&[(0.66, 0.6), (0.78, 0.6)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pdf_convert::Tool;

    use super::{Badge, badge_of, mark_size};

    #[test]
    fn the_five_security_tools_and_the_two_that_shrink_and_mend_are_told_apart_by_their_marks() {
        let marks: BTreeSet<String> = Tool::ALL
            .into_iter()
            .filter(|tool| {
                matches!(
                    tool,
                    Tool::Unlock
                        | Tool::Sign
                        | Tool::Redact
                        | Tool::Compare
                        | Tool::Protect
                        | Tool::Compress
                        | Tool::Repair
                )
            })
            .map(|tool| format!("{:?}", badge_of(tool).0))
            .collect();
        assert_eq!(marks.len(), 7, "{marks:?}");
    }

    #[test]
    fn a_mark_keeps_the_proportions_of_the_websites_page() {
        let size = mark_size(54.0);
        assert!((size.x - 46.0).abs() < 1e-4 && (size.y - 54.0).abs() < 1e-4);
        let small = mark_size(27.0);
        assert!((small.x * 2.0 - 46.0).abs() < 1e-4);
    }

    #[test]
    fn a_letter_badge_carries_the_short_label_of_its_format() {
        assert_eq!(badge_of(Tool::PdfToWord).0, Badge::Letters("W"));
        assert_eq!(badge_of(Tool::WordToPdf).0, badge_of(Tool::PdfToWord).0);
        assert_eq!(badge_of(Tool::PdfToImage).0, Badge::Letters("JPG"));
    }
}
