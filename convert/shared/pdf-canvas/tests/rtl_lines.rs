use std::sync::Arc;

use convert_pdf_canvas::{Canvas, Direction, FontBook, Line, Span, layout, layout_directed};

fn book() -> FontBook {
    let package = format!(
        "{}/../../../fonts/packaged",
        env!("CARGO_MANIFEST_DIR")
    );
    let provider = pdf_font::system_fonts::SystemFontProvider::discover_in(&[package.into()]);
    FontBook::new(Some(Arc::new(provider)))
}

fn drawn(line: &Line) -> String {
    line.pieces
        .iter()
        .flat_map(|p| {
            p.shaped.clusters[p.clusters.clone()]
                .iter()
                .map(|c| p.shaped.text[c.range.clone()].to_owned())
        })
        .collect()
}

fn set(text: &str, direction: Direction, width: Option<f32>) -> Vec<Line> {
    let mut canvas = Canvas::new();
    let mut book = book();
    let spans = [Span::new(text, "DejaVu Sans", 12.0)];
    layout_directed(&mut canvas, &mut book, &spans, width, direction)
}

#[test]
fn a_right_to_left_line_is_in_visual_order() {
    let lines = set(
        "D03.8 \u{05E9}\u{05DC}\u{05D5}\u{05DD}",
        Direction::Rtl,
        None,
    );
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert!(line.rtl);
    assert_eq!(drawn(line), "\u{05DD}\u{05D5}\u{05DC}\u{05E9} D03.8");
    assert_eq!(line.text(), "D03.8 \u{05E9}\u{05DC}\u{05D5}\u{05DD}");
}

#[test]
fn right_to_left_words_in_a_left_to_right_line() {
    let lines = set(
        "a \u{05D0}\u{05D1} \u{05D2}\u{05D3} b",
        Direction::Ltr,
        None,
    );
    let line = &lines[0];
    assert!(!line.rtl);
    assert_eq!(drawn(line), "a \u{05D3}\u{05D2} \u{05D1}\u{05D0} b");
}

#[test]
fn context_order_follows_the_first_letter() {
    let lines = set(
        "\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} PanPDF",
        Direction::Auto,
        None,
    );
    assert!(lines[0].rtl);
    assert!(drawn(&lines[0]).starts_with("PanPDF "));
    let lines = set(
        "PanPDF \u{0645}\u{0631}\u{062D}\u{0628}\u{0627}",
        Direction::Auto,
        None,
    );
    assert!(!lines[0].rtl);
    assert!(drawn(&lines[0]).starts_with("PanPDF "));
}

#[test]
fn arabic_is_joined() {
    let word = "\u{0628}\u{064A}\u{062A}";
    let lines = set(word, Direction::Rtl, None);
    let mut canvas = Canvas::new();
    let mut book = book();
    let pick = book.face(&mut canvas, "DejaVu Sans", false, false).unwrap();
    let alone: Vec<u16> = canvas
        .shape(pick.font, word)
        .clusters
        .iter()
        .flat_map(|c| c.glyphs.iter().map(|g| g.gid))
        .collect();
    let joined: Vec<u16> = lines[0]
        .pieces
        .iter()
        .flat_map(|p| {
            p.shaped
                .clusters
                .iter()
                .flat_map(|c| c.glyphs.iter().map(|g| g.gid))
        })
        .collect();
    assert!(!joined.is_empty());
    assert!(
        joined.iter().all(|g| !alone.contains(g)),
        "{joined:?} {alone:?}"
    );
    assert_eq!(lines[0].text(), word);
}

#[test]
fn brackets_are_mirrored_at_right_to_left_levels() {
    let lines = set("\u{05D0} (\u{05D1})", Direction::Rtl, None);
    let line = &lines[0];
    assert_eq!(drawn(line), ")\u{05D1}( \u{05D0}");
    let glyph = |t: &str| {
        line.pieces
            .iter()
            .flat_map(|p| p.shaped.clusters.iter().map(move |c| (p, c)))
            .find(|(p, c)| &p.shaped.text[c.range.clone()] == t)
            .map(|(_, c)| c.glyphs[0].gid)
    };
    let mut canvas = Canvas::new();
    let mut b = book();
    let pick = b.face(&mut canvas, "DejaVu Sans", false, false).unwrap();
    let close = canvas.shape(pick.font, ")").clusters[0].glyphs[0].gid;
    assert_eq!(glyph("("), Some(close));
}

#[test]
fn lines_break_in_logical_order() {
    let text = "\u{0647}\u{0630}\u{0647} \u{0641}\u{0642}\u{0631}\u{0629} \u{0639}\u{0631}\u{0628}\u{064A}\u{0629} \u{0637}\u{0648}\u{064A}\u{0644}\u{0629}";
    let lines = set(text, Direction::Rtl, Some(60.0));
    assert!(lines.len() >= 2, "{}", lines.len());
    let said: String = lines.iter().map(Line::text).collect::<Vec<_>>().join(" ");
    assert_eq!(said, text);
    assert!(lines.iter().all(|l| l.width <= 60.5));
}

#[test]
fn left_to_right_text_is_untouched() {
    let mut canvas = Canvas::new();
    let mut book = book();
    let spans = [Span::new("plain text, ສະບາຍດີ 12", "DejaVu Sans", 12.0)];
    let a = layout(&mut canvas, &mut book, &spans, Some(80.0));
    let b = layout_directed(&mut canvas, &mut book, &spans, Some(80.0), Direction::Auto);
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert!(x.said.is_none() && y.said.is_none() && !y.rtl);
        assert_eq!(drawn(x), drawn(y));
        assert!((x.width - y.width).abs() < 1e-3);
    }
}
