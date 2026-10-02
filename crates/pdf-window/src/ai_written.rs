use eframe::egui;
use pdf_agent::markup::tree::{Align, Block, Inline, List};

use pdf_app::wording::{Lang, Message};

const HEADINGS: [f32; 6] = [1.45, 1.25, 1.12, 1.05, 1.0, 1.0];

const ABOVE_A_HEADING: f32 = 0.85;
const BETWEEN_BLOCKS: f32 = 0.45;

const INDENT: f32 = 1.15;
const QUOTE_INDENT: f32 = 0.9;

pub(crate) fn written(ui: &mut egui::Ui, text: &str, salt: &str, lang: Lang) {
    let text = pdf_agent::markup::laying_out::mathematics_as_text(text);
    let blocks = pdf_agent::markup::document(&text);
    draw_blocks(ui, &blocks, &mut Place::new(salt, lang));
}

struct Place<'a> {
    salt: &'a str,
    lang: Lang,
    fences: usize,
    started: bool,
}

impl<'a> Place<'a> {
    fn new(salt: &'a str, lang: Lang) -> Self {
        Self {
            salt,
            lang,
            fences: 0,
            started: false,
        }
    }

    fn space_above(&mut self, ui: &mut egui::Ui, share: f32) {
        if self.started {
            ui.add_space(body(ui) * share);
        }
        self.started = true;
    }
}

fn body(ui: &egui::Ui) -> f32 {
    egui::TextStyle::Body.resolve(ui.style()).size
}

fn draw_blocks(ui: &mut egui::Ui, blocks: &[Block], place: &mut Place) {
    for block in blocks {
        draw_block(ui, block, place);
    }
}

fn draw_block(ui: &mut egui::Ui, block: &Block, place: &mut Place) {
    match block {
        Block::Heading { level, inlines } => {
            place.space_above(ui, ABOVE_A_HEADING);
            let size = HEADINGS
                .get(usize::from(level.saturating_sub(1)))
                .copied()
                .unwrap_or(1.0)
                * body(ui);
            let style = Style {
                size,
                strong: true,
                ..Style::plain(ui)
            };
            lines_of(ui, inlines, style, place.lang);
        }
        Block::Paragraph { inlines } => {
            place.space_above(ui, BETWEEN_BLOCKS);
            lines_of(ui, inlines, Style::plain(ui), place.lang);
        }
        Block::Code { info, text } => {
            place.space_above(ui, BETWEEN_BLOCKS);
            place.fences += 1;
            fenced(ui, (info, text), (place.salt, place.fences), place.lang);
        }
        Block::Break => {
            place.space_above(ui, BETWEEN_BLOCKS);
            ui.separator();
        }
        Block::Quote { blocks } => {
            place.space_above(ui, BETWEEN_BLOCKS);
            quoted(ui, blocks, place);
        }
        Block::List(list) => {
            place.space_above(ui, BETWEEN_BLOCKS);
            listed(ui, list, place);
        }
        Block::Table { head, rows, align } => {
            place.space_above(ui, BETWEEN_BLOCKS);
            table(ui, (head, rows, align), place);
        }
        Block::Html(_) | Block::Footnotes(_) => {}
    }
}

fn quoted(ui: &mut egui::Ui, blocks: &[Block], place: &mut Place) {
    let colour = ui.visuals().weak_text_color().gamma_multiply(0.6);
    let step = body(ui);
    let shown = ui.horizontal(|ui| {
        ui.add_space(step * QUOTE_INDENT + 2.0);
        ui.vertical(|ui| {
            let mut inside = Place {
                salt: place.salt,
                lang: place.lang,
                fences: place.fences,
                started: false,
            };
            draw_blocks(ui, blocks, &mut inside);
            place.fences = inside.fences;
        });
    });
    let rect = shown.response.rect;
    let bar = egui::Rect::from_min_max(
        rect.left_top(),
        egui::pos2(rect.left() + 2.0, rect.bottom()),
    );
    ui.painter().rect_filled(bar, 1.0, colour);
}

fn listed(ui: &mut egui::Ui, list: &List, place: &mut Place) {
    let step = body(ui);
    for (at, item) in list.items.iter().enumerate() {
        if list.loose && at > 0 {
            ui.add_space(step * BETWEEN_BLOCKS);
        }
        let mark = match list.first {
            None => "\u{2022}".to_owned(),
            Some(first) => format!("{}.", first.saturating_add(at as u64)),
        };
        ui.horizontal_top(|ui| {
            ui.add_space(step * INDENT * 0.5);
            ui.allocate_ui_with_layout(
                egui::vec2(step * 1.4, step),
                egui::Layout::right_to_left(egui::Align::TOP),
                |ui| {
                    ui.add_space(step * 0.35);
                    ui.weak(egui::RichText::new(mark).size(step));
                },
            );
            ui.vertical(|ui| {
                let mut inside = Place {
                    salt: place.salt,
                    lang: place.lang,
                    fences: place.fences,
                    started: false,
                };
                draw_blocks(ui, item, &mut inside);
                place.fences = inside.fences;
            });
        });
    }
}

type Table<'a> = (&'a [Vec<Inline>], &'a [Vec<Vec<Inline>>], &'a [Align]);

fn table(ui: &mut egui::Ui, (head, rows, align): Table<'_>, place: &mut Place) {
    let salt = format!("{}-table-{}", place.salt, place.fences);
    place.fences += 1;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::Grid::new(salt)
            .striped(true)
            .num_columns(head.len().max(1))
            .show(ui, |ui| {
                for (at, cell) in head.iter().enumerate() {
                    set_cell(ui, cell, align.get(at).copied(), true, place.lang);
                }
                ui.end_row();
                for row in rows {
                    for (at, cell) in row.iter().enumerate() {
                        set_cell(ui, cell, align.get(at).copied(), false, place.lang);
                    }
                    ui.end_row();
                }
            });
    });
}

fn set_cell(ui: &mut egui::Ui, cell: &[Inline], align: Option<Align>, heading: bool, lang: Lang) {
    let layout = match align.unwrap_or_default() {
        Align::Start | Align::Left => egui::Layout::left_to_right(egui::Align::Center),
        Align::Middle => egui::Layout::top_down(egui::Align::Center),
        Align::End => egui::Layout::right_to_left(egui::Align::Center),
    };
    ui.with_layout(layout, |ui| {
        let style = Style {
            strong: heading,
            ..Style::plain(ui)
        };
        run(ui, cell, style, lang, None);
    });
}

fn fenced(ui: &mut egui::Ui, (info, text): (&str, &str), (salt, nth): (&str, usize), lang: Lang) {
    let visuals = ui.visuals();
    let frame = egui::Frame::group(ui.style())
        .fill(visuals.extreme_bg_color)
        .stroke(egui::Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
        .inner_margin(egui::Margin::symmetric(8, 6));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let named = info.split_whitespace().next().unwrap_or_default();
            ui.weak(egui::RichText::new(named).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::format::quiet_icon_button(
                    ui,
                    crate::icons::Icon::Copy,
                    &Message::AiCopyCode.say(lang),
                )
                .clicked()
                {
                    ui.ctx().copy_text(text.to_owned());
                }
            });
        });
        egui::ScrollArea::horizontal()
            .id_salt(format!("{salt}-code-{nth}"))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text.trim_end_matches('\n')).monospace())
                        .selectable(true)
                        .wrap_mode(egui::TextWrapMode::Extend),
                );
            });
    });
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "one per inline mark, and the marks combine"
)]
#[derive(Clone, Copy)]
struct Style {
    size: f32,
    strong: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<egui::Color32>,
}

impl Style {
    fn plain(ui: &egui::Ui) -> Self {
        Self {
            size: body(ui),
            strong: false,
            italic: false,
            strike: false,
            code: false,
            link: None,
        }
    }

    fn apply(self, text: &str) -> egui::RichText {
        let mut rich = egui::RichText::new(text).size(self.size);
        if self.strong {
            rich = rich.strong();
        }
        if self.italic {
            rich = rich.italics();
        }
        if self.strike {
            rich = rich.strikethrough();
        }
        if self.code {
            rich = rich.code();
        }
        if let Some(colour) = self.link {
            rich = rich.color(colour).underline();
        }
        rich
    }
}

fn split_lines(inlines: &[Inline]) -> Vec<Vec<Inline>> {
    let mut lines: Vec<Vec<Inline>> = vec![Vec::new()];
    for piece in inlines {
        match piece {
            Inline::Hard => lines.push(Vec::new()),
            Inline::Emphasis(inside) => split_inside(&mut lines, inside, Inline::Emphasis),
            Inline::Strong(inside) => split_inside(&mut lines, inside, Inline::Strong),
            Inline::Strike(inside) => split_inside(&mut lines, inside, Inline::Strike),
            Inline::Highlight(inside) => split_inside(&mut lines, inside, Inline::Highlight),
            Inline::Link { to, title, text } => {
                split_inside(&mut lines, text, |text| Inline::Link {
                    to: to.clone(),
                    title: title.clone(),
                    text,
                });
            }
            other => {
                if let Some(line) = lines.last_mut() {
                    line.push(other.clone());
                }
            }
        }
    }
    lines
}

fn split_inside(
    lines: &mut Vec<Vec<Inline>>,
    inside: &[Inline],
    make: impl Fn(Vec<Inline>) -> Inline,
) {
    for (at, part) in split_lines(inside).into_iter().enumerate() {
        if at > 0 {
            lines.push(Vec::new());
        }
        if !part.is_empty()
            && let Some(line) = lines.last_mut()
        {
            line.push(make(part));
        }
    }
}

fn lines_of(ui: &mut egui::Ui, inlines: &[Inline], style: Style, lang: Lang) {
    let mut lines = split_lines(inlines);
    while lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    for line in &lines {
        wrapped(ui, line, style, lang);
    }
}

fn wrapped(ui: &mut egui::Ui, inlines: &[Inline], style: Style, lang: Lang) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        run(ui, inlines, style, lang, None);
    });
}

fn words(ui: &mut egui::Ui, rich: egui::RichText, address: Option<&str>) {
    let Some(to) = address else {
        ui.add(egui::Label::new(rich).selectable(true).wrap());
        return;
    };
    let response = ui
        .add(egui::Label::new(rich).sense(egui::Sense::click()).wrap())
        .on_hover_text(to);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() && web_address(to) {
        ui.ctx().open_url(egui::OpenUrl::new_tab(to));
    }
}

fn run(ui: &mut egui::Ui, inlines: &[Inline], style: Style, lang: Lang, address: Option<&str>) {
    for piece in inlines {
        match piece {
            Inline::Text(text) => words(ui, style.apply(text), address),
            Inline::Soft | Inline::Hard => {
                ui.add(egui::Label::new(style.apply(" ")).selectable(false));
            }
            Inline::Code(text) => words(
                ui,
                Style {
                    code: true,
                    ..style
                }
                .apply(text),
                address,
            ),
            Inline::Emphasis(inside) => run(
                ui,
                inside,
                Style {
                    italic: true,
                    ..style
                },
                lang,
                address,
            ),
            Inline::Strong(inside) | Inline::Highlight(inside) => run(
                ui,
                inside,
                Style {
                    strong: true,
                    ..style
                },
                lang,
                address,
            ),
            Inline::Strike(inside) => run(
                ui,
                inside,
                Style {
                    strike: true,
                    ..style
                },
                lang,
                address,
            ),
            Inline::Html(_) => {}
            Inline::Check(_) | Inline::Note(_) | Inline::Superscript(_) | Inline::Subscript(_) => {
                words(ui, style.apply(&piece.plain()), address);
            }
            Inline::Link { to, text, .. } => {
                let style = Style {
                    link: Some(ui.visuals().hyperlink_color),
                    ..style
                };
                run(ui, text, style, lang, Some(to));
            }
            Inline::Image { at, text, .. } => {
                let shown = if text.is_empty() {
                    Message::AiAPicture.say(lang)
                } else {
                    pdf_agent::markup::tree::plain(text)
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                    crate::icons::Icon::Picture.draw(
                        ui.painter(),
                        rect,
                        ui.visuals().weak_text_color(),
                    );
                    ui.add(
                        egui::Label::new(
                            Style {
                                italic: true,
                                ..style
                            }
                            .apply(&shown),
                        )
                        .selectable(true)
                        .wrap(),
                    )
                    .on_hover_text(at);
                });
            }
        }
    }
}

fn web_address(to: &str) -> bool {
    let lowered = to.trim().to_ascii_lowercase();
    lowered.starts_with("https://") || lowered.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use pdf_agent::markup::tree::Inline;
    use pdf_app::wording::Lang;

    use super::{split_lines, web_address, written};

    #[test]
    fn only_a_page_on_the_web_is_opened() {
        assert!(web_address("https://example.org/a"));
        assert!(web_address("HTTP://example.org"));
        assert!(web_address("  https://example.org  "));
        assert!(!web_address("file:///etc/shadow"));
        assert!(!web_address("javascript:alert(1)"));
        assert!(!web_address("/home/someone/.ssh/id_rsa"));
        assert!(!web_address("mailto:somebody@example.org"));
        assert!(!web_address(""));
    }

    fn text(said: &str) -> Inline {
        Inline::Text(said.to_owned())
    }

    #[test]
    fn a_hard_break_inside_strong_text_still_breaks_the_line() {
        let lines = split_lines(&[Inline::Strong(vec![text("one"), Inline::Hard, text("two")])]);
        assert_eq!(
            lines,
            vec![
                vec![Inline::Strong(vec![text("one")])],
                vec![Inline::Strong(vec![text("two")])],
            ]
        );
    }

    #[test]
    fn a_hard_break_deep_inside_marks_and_a_link_breaks_each_piece_of_it() {
        let to = "https://example.org".to_owned();
        let lines = split_lines(&[
            text("a"),
            Inline::Emphasis(vec![Inline::Link {
                to: to.clone(),
                title: String::new(),
                text: vec![text("b"), Inline::Hard, text("c")],
            }]),
            Inline::Hard,
            text("d"),
        ]);
        assert_eq!(
            lines,
            vec![
                vec![
                    text("a"),
                    Inline::Emphasis(vec![Inline::Link {
                        to: to.clone(),
                        title: String::new(),
                        text: vec![text("b")]
                    }])
                ],
                vec![Inline::Emphasis(vec![Inline::Link {
                    to,
                    title: String::new(),
                    text: vec![text("c")]
                }])],
                vec![text("d")],
            ]
        );
    }

    #[test]
    fn lines_with_no_break_stay_one_line_and_a_trailing_break_is_empty() {
        assert_eq!(
            split_lines(&[text("a"), Inline::Soft, text("b")]),
            vec![vec![text("a"), Inline::Soft, text("b")]]
        );
        assert_eq!(
            split_lines(&[text("a"), Inline::Hard]),
            vec![vec![text("a")], Vec::new()]
        );
        assert_eq!(split_lines(&[]), vec![Vec::<Inline>::new()]);
    }

    fn frame(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events,
            ..egui::RawInput::default()
        }
    }

    fn opened_after_clicking(source: &str, along: f32) -> Vec<String> {
        let ctx = egui::Context::default();
        let draw = |raw: egui::RawInput| {
            let mut seen = (egui::Rect::NOTHING, 14.0);
            let output = ctx.run_ui(raw, |ui| {
                let before = ui.cursor().min;
                written(ui, source, "test", Lang::English);
                seen = (
                    egui::Rect::from_min_max(before, ui.min_rect().max),
                    egui::TextStyle::Body.resolve(ui.style()).size,
                );
            });
            (output, seen)
        };
        let (_, (start, size)) = draw(frame(Vec::new()));
        let at = egui::pos2(start.left() + along * size, start.top() + size * 0.7);
        let _ = draw(frame(vec![egui::Event::PointerMoved(at)]));
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let _ = draw(frame(vec![press(true)]));
        let (output, _) = draw(frame(vec![press(false)]));
        output
            .platform_output
            .commands
            .into_iter()
            .filter_map(|command| match command {
                egui::OutputCommand::OpenUrl(open) => Some(open.url),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn clicking_the_words_of_a_link_opens_its_page() {
        let opened = opened_after_clicking("see [the docs](https://example.org/x) now", 5.0);
        assert_eq!(opened, ["https://example.org/x"]);
    }

    #[test]
    fn clicking_the_words_of_a_link_that_is_in_bold_opens_its_page() {
        let opened = opened_after_clicking("**[docs](https://example.org/b)** here", 1.0);
        assert_eq!(opened, ["https://example.org/b"]);
    }

    #[test]
    fn clicking_text_that_is_not_a_link_opens_nothing() {
        let opened = opened_after_clicking("see [the docs](https://example.org/x) now", 1.0);
        assert!(opened.is_empty(), "{opened:?}");
    }

    #[test]
    fn a_link_to_a_file_is_not_opened_whatever_is_clicked() {
        let opened = opened_after_clicking("[secrets](file:///etc/shadow)", 1.0);
        assert!(opened.is_empty(), "{opened:?}");
    }

    fn bars_beside(source: &str) -> (egui::Rect, Vec<egui::Rect>) {
        let ctx = egui::Context::default();
        let mut whole = egui::Rect::NOTHING;
        let output = ctx.run_ui(frame(Vec::new()), |ui| {
            let before = ui.cursor().min;
            written(ui, source, "bars", Lang::English);
            whole = egui::Rect::from_min_max(before, ui.min_rect().max);
        });
        let mut bars = Vec::new();
        for clipped in output.shapes {
            if let egui::epaint::Shape::Rect(rect) = clipped.shape
                && (rect.rect.width() - 2.0).abs() < 0.01
            {
                bars.push(rect.rect);
            }
        }
        (whole, bars)
    }

    #[test]
    fn a_quote_of_several_paragraphs_has_a_rule_as_tall_as_all_of_it() {
        let (whole, bars) =
            bars_beside("> first paragraph\n>\n> second paragraph\n>\n> third paragraph");
        assert_eq!(bars.len(), 1, "{bars:?}");
        assert!(
            bars[0].height() >= whole.height() - 1.0,
            "the rule is {} high beside {} of quote",
            bars[0].height(),
            whole.height()
        );
    }

    #[test]
    fn a_one_line_quote_has_a_one_line_rule() {
        let (whole, bars) = bars_beside("> short");
        assert_eq!(bars.len(), 1);
        assert!((bars[0].height() - whole.height()).abs() < 2.0);
    }
}
