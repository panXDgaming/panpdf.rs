use std::fmt::Write as _;

use super::theme::{self, Theme};
use super::{Composed, Mark, Measure, Room, Setting, Sheet, Style, compose};
use crate::markup::laying_out::parts;

struct Even;

impl Measure for Even {
    fn room(&self, text: &str, style: &Style, width: f64) -> Result<Room, String> {
        if text.chars().any(|c| u32::from(c) > 0xFFFF) {
            return Err("no face on this machine draws what was typed".to_owned());
        }
        let per = style.size * 0.5;
        let mut lines = 0_u32;
        let mut widest: f64 = 0.0;
        for paragraph in text.split('\n') {
            let wide = f64::from(u32::try_from(paragraph.chars().count()).unwrap_or(0)) * per;
            let count = (wide / width).ceil().max(1.0);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a small count"
            )]
            let count = count as u32;
            lines += count;
            widest = widest.max(wide.min(width));
        }
        Ok(Room {
            height: f64::from(lines) * style.size * 1.2,
            widest,
            lines: lines as usize,
        })
    }
}

const A4: Sheet = Sheet {
    wide: 595.0,
    high: 842.0,
    margin: 50.0,
};

fn composed(markdown: &str, theme: &Theme) -> Composed {
    let setting = Setting {
        sheet: A4,
        from_page: 0,
        start: None,
        family: "Noto Sans",
        theme,
        body: 10.0,
    };
    compose(&parts(markdown, 10.0), &setting, &Even).expect("it composes")
}

fn same(one: &[f64; 3], other: &[f64; 3]) -> bool {
    one.map(f64::to_bits) == other.map(f64::to_bits)
}

fn texts(composed: &Composed) -> Vec<(&str, [f64; 4], &Style, usize)> {
    composed
        .marks
        .iter()
        .filter_map(|mark| match mark {
            Mark::Text {
                page,
                area,
                text,
                style,
            } => Some((text.as_str(), *area, style, *page)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_theme_dresses_the_page() {
    let classic = theme::named("classic").expect("classic");
    let out = composed("# Title\n\n## Part\n\n- item\n\nBody.\n", classic);
    let first_text = out
        .marks
        .iter()
        .position(|mark| matches!(mark, Mark::Text { .. }))
        .expect("text");
    let bands: Vec<&Mark> = out.marks[..first_text].iter().collect();
    assert_eq!(
        bands.len(),
        2,
        "shadow and band before the title's words: {bands:#?}"
    );
    assert!(
        matches!(bands[1], Mark::Shape { fill: Some(fill), .. } if same(fill, &classic.accent))
    );
    let words = texts(&out);
    let (title, area, style, _) = words[0];
    assert_eq!(title, "Title");
    assert_eq!(style.colour, Some(classic.on_accent));
    assert!((area[0] - (50.0 + 18.0 * 0.6)).abs() < 1e-9, "{area:?}");
    let (part, _, style, _) = words[1];
    assert_eq!(part, "Part");
    assert_eq!(style.colour, Some(classic.accent));
    let (item, _, _, _) = words[2];
    assert_eq!(item, "item");
    let dot = out.marks.iter().any(|mark| {
        matches!(mark, Mark::Shape { fill: Some(fill), steps, .. } if same(fill, &classic.accent) && steps.len() == 5)
    });
    assert!(dot, "a round bullet in the accent");
    assert_eq!(words[3].2.colour, Some(classic.ink));

    let plain = theme::named("plain").expect("plain");
    let bare = composed("# Title\n\n## Part\n\n- item\n", plain);
    for mark in &bare.marks {
        if let Mark::Shape {
            fill: Some(fill), ..
        } = mark
        {
            assert!(
                same(fill, &plain.ink) || same(fill, &plain.line),
                "{fill:?}"
            );
        }
    }
}

#[test]
fn a_table_is_dressed_and_carried_over() {
    let classic = theme::named("classic").expect("classic");
    let mut markdown = "| Name | Score |\n|---|---|\n".to_owned();
    for at in 0..40 {
        let _ = writeln!(markdown, "| Row {at} | {at} |");
    }
    let out = composed(&markdown, classic);
    let first_text = out
        .marks
        .iter()
        .position(|mark| matches!(mark, Mark::Text { .. }))
        .expect("text");
    let before: Vec<&Mark> = out.marks[..first_text].iter().collect();
    assert!(
        before.iter().any(
            |mark| matches!(mark, Mark::Shape { fill: Some(fill), .. } if same(fill, &classic.accent))
        ),
        "the head's band goes first"
    );
    assert!(
        before.iter().any(
            |mark| matches!(mark, Mark::Shape { fill: Some(fill), .. } if same(fill, &classic.stripe))
        ),
        "the stripes go first"
    );
    assert!(
        out.marks
            .iter()
            .any(|mark| matches!(mark, Mark::NewPage { after: 0 }))
    );
    let heads: Vec<usize> = texts(&out)
        .into_iter()
        .filter(|(text, _, _, _)| *text == "Name")
        .map(|(_, _, _, page)| page)
        .collect();
    assert_eq!(heads, vec![0, 1]);
    assert_eq!(out.pages, 2);
}

#[test]
fn a_long_document_goes_on_to_new_pages() {
    let classic = theme::named("slate").expect("slate");
    let markdown = "A paragraph of words.\n\n".repeat(80);
    let out = composed(&markdown, classic);
    assert!(out.pages >= 2);
    for (_, area, _, _) in texts(&out) {
        assert!(area[3] <= 842.0 - 50.0 + 1e-9, "{area:?}");
    }
}

#[test]
fn a_dark_theme_paints_the_paper() {
    let midnight = theme::named("midnight").expect("midnight");
    let out = composed(&"Words.\n\n".repeat(120), midnight);
    let Mark::Shape { fill, .. } = &out.marks[0] else {
        panic!("the paper first");
    };
    assert_eq!(*fill, midnight.paper);
    let papers = out
        .marks
        .iter()
        .filter(|mark| matches!(mark, Mark::Shape { fill, .. } if *fill == midnight.paper))
        .count();
    assert_eq!(papers, out.pages);
}

#[test]
fn pictographs_are_left_out() {
    let classic = theme::named("classic").expect("classic");
    let out = composed("Well done! \u{1F31F}\n", classic);
    assert_eq!(texts(&out)[0].0, "Well done! ");
    assert_eq!(out.left_out, "\u{1F31F}", "and the model is told what went");
}

#[test]
fn charts_are_drawn_or_refused_whole() {
    let classic = theme::named("classic").expect("classic");
    let out = composed(
        "```chart\n{\"type\":\"bar\",\"title\":\"Sales\",\"x\":[\"Q1\",\"Q2\"],\"series\":[{\"name\":\"A\",\"values\":[3,5]},{\"name\":\"B\",\"values\":[4,2]}],\"style\":\"3d\"}\n```\n",
        classic,
    );
    let fills: Vec<[f64; 3]> = out
        .marks
        .iter()
        .filter_map(|mark| match mark {
            Mark::Shape {
                fill: Some(fill), ..
            } => Some(*fill),
            _ => None,
        })
        .collect();
    assert!(fills.contains(&classic.palette[0]) && fills.contains(&classic.palette[1]));
    assert!(
        fills.contains(&theme::darker(classic.palette[0], 0.28)),
        "a shaded side"
    );
    assert!(texts(&out).iter().any(|(text, _, _, _)| *text == "Sales"));

    let setting = Setting {
        sheet: A4,
        from_page: 0,
        start: None,
        family: "Noto Sans",
        theme: classic,
        body: 10.0,
    };
    let refused = compose(
        &parts("Before.\n\n```chart\n{\"type\":\"radar\"}\n```\n", 10.0),
        &setting,
        &Even,
    );
    assert!(refused.unwrap_err().contains("radar"));
}

fn field_marks(out: &Composed) -> Vec<&crate::fielding::Asked> {
    out.marks
        .iter()
        .filter_map(|mark| match mark {
            Mark::Field(asked) => Some(asked),
            _ => None,
        })
        .collect()
}

fn form(fields: &str) -> String {
    format!("```form\n{{\"fields\":[{fields}]}}\n```\n")
}

fn refused_form(markdown: &str) -> String {
    let setting = Setting {
        sheet: A4,
        from_page: 0,
        start: None,
        family: "Noto Sans",
        theme: theme::named("classic").expect("classic"),
        body: 10.0,
    };
    compose(&parts(markdown, 10.0), &setting, &Even).expect_err("the form is refused")
}

#[test]
fn a_form_block_becomes_labelled_fields_with_stars_on_the_required_ones() {
    let classic = theme::named("classic").expect("classic");
    let out = composed(
        &form(
            r#"{"label":"Full name","name":"full_name","required":true},
            {"label":"Role","name":"role","kind":"dropdown","options":["A","B"]},
            {"label":"Agree","name":"agree","kind":"checkbox"},
            {"label":"Size","name":"size","kind":"radio","options":["S","M","L"]}"#,
        ),
        classic,
    );
    let fields = field_marks(&out);
    let names: Vec<_> = fields
        .iter()
        .map(|asked| asked.name.as_deref().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        ["full_name", "role", "agree", "size", "size", "size"]
    );
    let words: Vec<_> = texts(&out).into_iter().map(|(text, ..)| text).collect();
    assert!(words.contains(&"Full name *"), "{words:?}");
    assert!(words.contains(&"Role"), "{words:?}");
    assert!(words.contains(&"Agree"), "{words:?}");
    assert_eq!(fields[1].options, ["A", "B"]);
}

#[test]
fn a_form_with_a_repeated_name_a_bad_kind_or_bad_json_is_refused_whole() {
    let twice = refused_form(&form(
        r#"{"label":"A","name":"same"},{"label":"B","name":"same"}"#,
    ));
    assert!(twice.contains("same"), "{twice}");
    let across = refused_form(&format!(
        "{}\n{}",
        form(r#"{"label":"A","name":"same"}"#),
        form(r#"{"label":"B","name":"same"}"#)
    ));
    assert!(across.contains("same"), "{across}");
    let kind = refused_form(&form(r#"{"label":"A","name":"a","kind":"slider"}"#));
    assert!(kind.contains("slider"), "{kind}");
    let json = refused_form("```form\n{\"fields\":\n```\n");
    assert!(json.contains("not JSON"), "{json}");
    let dropdown = refused_form(&form(r#"{"label":"A","name":"a","kind":"dropdown"}"#));
    assert!(dropdown.contains("options"), "{dropdown}");
}

#[test]
fn two_half_fields_share_a_row_and_a_full_one_takes_the_width() {
    let classic = theme::named("classic").expect("classic");
    let out = composed(
        &form(
            r#"{"label":"A","name":"a","half":true},{"label":"B","name":"b","half":true},
            {"label":"C","name":"c"}"#,
        ),
        classic,
    );
    let fields = field_marks(&out);
    assert_eq!(fields.len(), 3);
    let [a, b, c] = [fields[0].area, fields[1].area, fields[2].area];
    assert!((a[1] - b[1]).abs() < 1e-6, "one row: {a:?} {b:?}");
    assert!(b[0] > a[2], "side by side");
    assert!(c[1] > a[3], "the next row is below");
    assert!((c[0] - 50.0).abs() < 1e-6 && (c[2] - 545.0).abs() < 1e-6);
}

#[test]
fn a_row_of_a_long_form_is_never_split_across_two_pages() {
    let classic = theme::named("classic").expect("classic");
    let many: Vec<String> = (0..40)
        .map(|at| format!(r#"{{"label":"Field {at}","name":"f{at}","half":true}}"#))
        .collect();
    let out = composed(&form(&many.join(",")), classic);
    let fields = field_marks(&out);
    assert_eq!(fields.len(), 40);
    assert!(fields.iter().any(|asked| asked.page == 1), "a second page");
    for pair in fields.chunks(2) {
        assert_eq!(pair[0].page, pair[1].page, "a row stays on one page");
        assert!((pair[0].area[1] - pair[1].area[1]).abs() < 1e-6);
    }
    let labels = texts(&out);
    for asked in &fields {
        let label = labels.iter().find(|(_, area, _, page)| {
            *page == asked.page
                && (area[0] - asked.area[0]).abs() < 1e-6
                && area[3] <= asked.area[1] + 1e-6
                && asked.area[1] - area[3] < 10.0
        });
        assert!(label.is_some(), "a label sits above {asked:?} on its page");
    }
}

#[test]
fn an_equation_is_stacked() {
    let classic = theme::named("classic").expect("classic");
    let out = composed("$$\\frac{a+b}{2}$$\n", classic);
    let words = texts(&out);
    assert_eq!(words.len(), 2, "{words:?}");
    assert!(
        words[0].1[1] < words[1].1[1],
        "the numerator above the denominator"
    );
    let bars = out
        .marks
        .iter()
        .filter(|mark| {
            matches!(
                mark,
                Mark::Shape {
                    stroke: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(bars, 1);
}

#[test]
fn a_heading_keeps_with_what_it_heads() {
    let classic = theme::named("classic").expect("classic");
    let filler = "Words.\n\n".repeat(42);
    let chart = format!(
        "{filler}## Results\n\n```chart\n{{\"type\":\"bar\",\"x\":[\"a\"],\"series\":[{{\"values\":[1]}}],\"height\":200}}\n```\n"
    );
    let page_of = |out: &Composed, wanted: &str| {
        texts(out)
            .into_iter()
            .filter(|(text, _, _, _)| *text == wanted)
            .map(|(_, _, _, page)| page)
            .next_back()
            .expect("found")
    };
    let out = composed(&chart, classic);
    assert_eq!(page_of(&out, "Words."), 0, "the filler fits page 0");
    assert_eq!(
        page_of(&out, "Results"),
        1,
        "the heading moved to the chart's page"
    );
    let control = composed(&format!("{filler}## Results\n\nMore.\n"), classic);
    assert_eq!(page_of(&control, "Results"), 0);
}

fn the_pages_of_a_blank_document() -> Vec<pdf_content::PageGeometry> {
    let bytes = pdf_session::blank_document([595.0, 842.0]).expect("a blank page");
    let source = pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(0), bytes);
    pdf_session::Session::new(source, b"")
        .page_geometries()
        .expect("the page is measured")
}

#[test]
fn a_composed_document_becomes_commands_that_place_its_new_pages_as_they_will_be_numbered() {
    use pdf_edit::Command;

    let slate = theme::named("slate").expect("slate");
    let out = composed(&"A paragraph of words.\n\n".repeat(80), slate);
    let pages = the_pages_of_a_blank_document();
    let commands = super::placing::as_commands(&out.marks, &|page| pages.get(page).copied())
        .expect("every mark has a page to go on");
    assert_eq!(commands.len(), out.marks.len(), "one command to a mark");

    let added: Vec<usize> = commands
        .iter()
        .filter_map(|command| match command {
            Command::AddBlankPage { beside, .. } => Some(*beside),
            _ => None,
        })
        .collect();
    assert_eq!(added.len(), out.pages - 1);
    assert_eq!(
        added,
        (0..out.pages - 1).collect::<Vec<_>>(),
        "one after the other"
    );

    let first = out
        .marks
        .iter()
        .zip(&commands)
        .find_map(|(mark, command)| match (mark, command) {
            (
                Mark::Text { area, page: 0, .. },
                Command::PlaceNewText {
                    frame,
                    page_index: 0,
                    ..
                },
            ) => Some((*area, *frame)),
            _ => None,
        })
        .expect("a piece of text on the first page");
    let (area, frame) = first;
    assert!((frame[0] - area[0]).abs() < 1e-9 && (frame[2] - area[2]).abs() < 1e-9);
    assert!(
        (frame[1] - (842.0 - area[3])).abs() < 1e-9 && (frame[3] - (842.0 - area[1])).abs() < 1e-9,
        "the top of the page shown is the top of the page, y counted upward: {area:?} {frame:?}"
    );

    let on_a_new_page = out
        .marks
        .iter()
        .zip(&commands)
        .find_map(|(mark, command)| match (mark, command) {
            (
                Mark::Text { area, page, .. },
                Command::PlaceNewText {
                    frame, page_index, ..
                },
            ) if *page > 0 && page == page_index => Some((*area, *frame)),
            _ => None,
        })
        .expect("a piece of text on a page that is new");
    assert!(
        (on_a_new_page.1[1] - (842.0 - on_a_new_page.0[3])).abs() < 1e-9,
        "a page that is not there yet is placed as a blank page of the same size"
    );
}

#[test]
fn a_mark_on_a_page_that_is_not_there_is_refused_in_words() {
    let marks = vec![Mark::Shape {
        page: 4,
        steps: vec![
            super::PenStep::Move((1.0, 1.0)),
            super::PenStep::Line((2.0, 2.0)),
        ],
        stroke: Some(([0.0, 0.0, 0.0], 1.0)),
        fill: None,
    }];
    let pages = the_pages_of_a_blank_document();
    let refused = super::placing::as_commands(&marks, &|page| pages.get(page).copied())
        .expect_err("page 5 does not exist");
    assert!(refused.contains("page 5"), "{refused}");
}

fn composed_from(markdown: &str, theme: &Theme, start: Option<f64>) -> Composed {
    let setting = Setting {
        sheet: A4,
        from_page: 0,
        start,
        family: "Noto Sans",
        theme,
        body: 10.0,
    };
    compose(&parts(markdown, 10.0), &setting, &Even).expect("it composes")
}

fn words_of(out: &Composed) -> String {
    texts(out)
        .iter()
        .map(|(text, ..)| *text)
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_dark_page_written_under_what_is_there_is_dark_only_below_it() {
    let midnight = theme::named("midnight").expect("midnight");
    let under = composed_from("Words.\n", midnight, Some(300.0));
    let Mark::Shape { fill, steps, .. } = &under.marks[0] else {
        panic!("the paper first");
    };
    assert_eq!(
        *fill, midnight.paper,
        "the light ink has a dark ground to be read on"
    );
    let tops: Vec<f64> = steps
        .iter()
        .map(|step| match step {
            super::PenStep::Move(at) | super::PenStep::Line(at) => at.1,
            super::PenStep::Curve(_, _, end) => end.1,
        })
        .collect();
    let least = tops.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(
        (least - 300.0).abs() < 1e-9,
        "the page above 300 is left as it was: {tops:?}"
    );
    let whole = composed_from("Words.\n", midnight, None);
    let Mark::Shape { steps, .. } = &whole.marks[0] else {
        panic!("the paper first");
    };
    let top = steps
        .iter()
        .map(|step| match step {
            super::PenStep::Move(at) | super::PenStep::Line(at) => at.1,
            super::PenStep::Curve(_, _, end) => end.1,
        })
        .fold(f64::INFINITY, f64::min);
    assert!(
        top.abs() < 1e-9,
        "negative control: starting at the top covers the whole page"
    );
    let classic = theme::named("classic").expect("classic");
    assert!(
        composed_from("Words.\n", classic, Some(300.0))
            .marks
            .iter()
            .all(|mark| !matches!(mark, Mark::Shape { fill, .. } if *fill == classic.paper)),
        "a theme with no paper paints none"
    );
}

#[test]
fn a_paragraph_taller_than_a_page_is_carried_over_the_pages_and_loses_no_word() {
    let plain = theme::named("plain").expect("plain");
    let words: Vec<String> = (0..2_400).map(|at| format!("word{at}")).collect();
    let paragraph = words.join(" ");
    let out = composed(&paragraph, plain);
    assert!(out.pages >= 2, "{} pages", out.pages);
    for (_, area, _, _) in texts(&out) {
        assert!(
            area[3] <= 842.0 - 50.0 + 1e-6,
            "runs off the page: {area:?}"
        );
    }
    assert_eq!(
        words_of(&out),
        paragraph,
        "every word is there, once, in order"
    );
    assert!(out.pieces >= 2, "the one paragraph became several frames");
}

#[test]
fn a_bullet_taller_than_a_page_has_its_dot_once_and_a_paragraph_that_fits_is_not_split() {
    let classic = theme::named("classic").expect("classic");
    let words: Vec<String> = (0..2_400).map(|at| format!("w{at}")).collect();
    let big = composed(&format!("- {}\n", words.join(" ")), classic);
    let dots = big
        .marks
        .iter()
        .filter(|mark| matches!(mark, Mark::Shape { steps, .. } if steps.len() == 5))
        .count();
    assert_eq!(dots, 1, "one bullet for one item");
    let small = composed("A short paragraph that fits where it is.\n", classic);
    assert_eq!(
        texts(&small).len(),
        1,
        "negative control: a paragraph that fits is one frame"
    );
}

#[test]
fn a_code_block_taller_than_a_page_is_boxed_page_by_page_and_keeps_every_line() {
    let classic = theme::named("classic").expect("classic");
    let lines: Vec<String> = (0..180).map(|at| format!("line_{at}();")).collect();
    let markdown = format!("```\n{}\n```\n", lines.join("\n"));
    let out = composed(&markdown, classic);
    assert!(out.pages >= 2, "{} pages", out.pages);
    for (_, area, _, _) in texts(&out) {
        assert!(
            area[3] <= 842.0 - 50.0 + 1e-6,
            "runs off the page: {area:?}"
        );
    }
    let kept: Vec<String> = texts(&out)
        .iter()
        .flat_map(|(text, ..)| text.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect();
    assert_eq!(kept, lines, "every line, once, in order");
    let boxes = out
        .marks
        .iter()
        .filter(|mark| {
            matches!(
                mark,
                Mark::Shape {
                    stroke: Some(_),
                    ..
                }
            )
        })
        .count();
    assert_eq!(boxes, out.pages, "a box on each page the block reaches");
}

#[test]
fn a_quote_taller_than_a_page_is_set_as_ordinary_paragraphs_rather_than_run_off_the_page() {
    let classic = theme::named("classic").expect("classic");
    let words: Vec<String> = (0..2_400).map(|at| format!("q{at}")).collect();
    let out = composed(&format!("> {}\n", words.join(" ")), classic);
    for (_, area, _, _) in texts(&out) {
        assert!(
            area[3] <= 842.0 - 50.0 + 1e-6,
            "runs off the page: {area:?}"
        );
    }
    assert_eq!(words_of(&out), words.join(" "));
}

struct ThaiFace;

impl Measure for ThaiFace {
    fn room(&self, text: &str, style: &Style, width: f64) -> Result<Room, String> {
        if let Some(letter) = text
            .chars()
            .find(|letter| !(letter.is_ascii() || ('\u{0E00}'..='\u{0E7F}').contains(letter)))
        {
            return Err(format!(
                "the face chosen does not draw what was typed ({letter:?})"
            ));
        }
        Even.room(text, style, width)
    }
}

fn composed_in_thai(markdown: &str) -> Result<Composed, String> {
    let plain = theme::named("plain").expect("plain");
    let setting = Setting {
        sheet: A4,
        from_page: 0,
        start: None,
        family: "Noto Sans Thai",
        theme: plain,
        body: 10.0,
    };
    compose(&parts(markdown, 10.0), &setting, &ThaiFace)
}

#[test]
fn a_thai_paragraph_with_symbols_its_face_lacks_is_written_with_stand_ins_not_refused() {
    let out = composed_in_thai("ความเร็ว v\u{00B2} และ \u{03B1} \u{2192} 5 \u{2264} 9\n")
        .expect("the symbols are mended, not refused");
    let words = texts(&out);
    assert_eq!(words.len(), 1);
    assert_eq!(
        words[0].0, "ความเร็ว v^2 และ alpha -> 5 <= 9",
        "each symbol is said in letters the face has"
    );
    assert!(
        out.left_out.is_empty(),
        "nothing was lost: {:?}",
        out.left_out
    );
}

#[test]
fn a_symbol_with_no_stand_in_is_left_out_and_the_model_is_told() {
    let out = composed_in_thai("ราคา \u{20AC} 5 บาท\n").expect("the euro sign is dropped");
    assert_eq!(texts(&out)[0].0, "ราคา  5 บาท");
    assert_eq!(out.left_out, "\u{20AC}");
}

#[test]
fn a_paragraph_that_would_be_nothing_without_its_symbols_is_still_refused() {
    let why = composed_in_thai("\u{20AC}\u{20AC}\n").expect_err("nothing is left to write");
    assert!(why.contains("does not draw what was typed"), "{why}");
    assert!(
        composed_in_thai("plain words\n").is_ok(),
        "negative control: a paragraph the face draws whole"
    );
}

#[test]
fn a_cell_of_a_table_gets_the_same_mending_as_a_paragraph() {
    let out = composed_in_thai("| a | b |\n|---|---|\n| ความเร็ว | \u{03B1} \u{2192} 1 |\n")
        .expect("a table cell is mended too");
    let said: Vec<&str> = texts(&out).iter().map(|(text, ..)| *text).collect();
    assert!(said.contains(&"alpha -> 1"), "{said:?}");
}
