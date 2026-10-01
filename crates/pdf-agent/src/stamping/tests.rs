use pdf_edit::stamp::{Edge, Only, Side, Spot};

use crate::desk::tests::with_paragraphs;

use super::{Asked, Kind, spot_as_str, spot_of, today, written_day};

#[test]
fn a_day_is_written_as_day_month_year_and_counts_leap_days() {
    assert_eq!(written_day(0), "01/01/1970");
    assert_eq!(written_day(1_000_000_000), "09/09/2001");
    assert_eq!(written_day(951_782_400), "29/02/2000");
    assert_eq!(written_day(1_709_164_800), "29/02/2024");
    assert_eq!(written_day(1_709_251_200), "01/03/2024");
    assert_eq!(today().len(), 10, "{}", today());
}

#[test]
fn every_place_on_the_page_has_a_name_that_reads_back_as_itself() {
    let spots = [
        Spot::Middle,
        Spot::Along {
            edge: Edge::Header,
            side: Side::Left,
        },
        Spot::Along {
            edge: Edge::Header,
            side: Side::Centre,
        },
        Spot::Along {
            edge: Edge::Header,
            side: Side::Right,
        },
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Left,
        },
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Centre,
        },
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Right,
        },
    ];
    for spot in spots {
        assert_eq!(spot_of(spot_as_str(spot)), Some(spot), "{spot:?}");
    }
    assert_eq!(
        spot_of("footer-center"),
        Some(Spot::Along {
            edge: Edge::Footer,
            side: Side::Centre
        }),
        "the American spelling and a hyphen are understood"
    );
    assert_eq!(spot_of("under the table"), None);
    assert_eq!(spot_of("footer_up"), None);
}

#[test]
fn each_kind_of_stamp_starts_from_what_the_stamp_panel_starts_from() {
    let prepared = |kind: Kind| Asked::new(kind).prepare(3).expect("prepared");
    let numbers = prepared(Kind::PageNumbers);
    assert_eq!(numbers.stamp.wording, "{page}");
    assert_eq!(
        numbers.stamp.spot,
        Spot::Along {
            edge: Edge::Footer,
            side: Side::Centre
        }
    );
    assert!((numbers.stamp.size - 10.0).abs() < 1e-9);
    assert_eq!(numbers.pages, vec![0, 1, 2]);
    let header = prepared(Kind::HeaderFooter);
    assert_eq!(header.stamp.wording, "{file}");
    assert!((header.stamp.size - 9.0).abs() < 1e-9);
    let mark = prepared(Kind::Watermark);
    assert_eq!(mark.stamp.wording, "DRAFT");
    assert_eq!(mark.stamp.spot, Spot::Middle);
    assert!(mark.stamp.bold);
    assert!((mark.stamp.opacity - 0.5).abs() < 1e-9);
    assert!(
        !numbers.stamp.bold,
        "negative control: numbers are not bold"
    );
}

#[test]
fn the_first_page_stamped_shows_its_own_number_unless_a_start_is_given() {
    let mut asked = Asked::new(Kind::PageNumbers);
    asked.pages = "3-5".to_owned();
    let prepared = asked.prepare(6).expect("prepared");
    assert_eq!(prepared.pages, vec![2, 3, 4]);
    assert_eq!(prepared.start, 3);
    assert_eq!(prepared.facts(3, (6, "a.pdf", "01/01/2025")).number, 4);
    asked.start = Some(1);
    let prepared = asked.prepare(6).expect("prepared");
    assert_eq!(prepared.facts(4, (6, "a.pdf", "01/01/2025")).number, 3);
    asked.only = Only::Odd;
    asked.pages = String::new();
    let prepared = asked.prepare(6).expect("prepared");
    assert_eq!(prepared.pages, vec![0, 2, 4]);
}

#[test]
fn a_page_range_that_names_no_page_is_refused_in_words() {
    let mut asked = Asked::new(Kind::PageNumbers);
    asked.pages = "9".to_owned();
    let why = asked.prepare(3).expect_err("past the end");
    assert!(why.contains("no page of that number"), "{why}");
    asked.pages = "abc".to_owned();
    let why = asked.prepare(3).expect_err("not numbers");
    assert!(why.contains("numbers"), "{why}");
}

fn three_pages(name: &str) -> (crate::desk::Desk, String) {
    let (mut desk, handle) = with_paragraphs(name, &["Body text."]);
    for _ in 0..2 {
        desk.command(
            &handle,
            &pdf_edit::Command::AddBlankPage {
                beside: 0,
                before: false,
                size: [595.0, 842.0],
            },
        )
        .expect("a page is added");
    }
    (desk, handle)
}

#[test]
fn page_numbers_are_stamped_on_every_page_as_one_step_and_read_back() {
    let (mut desk, handle) = three_pages("numbers");
    let mut asked = Asked::new(Kind::PageNumbers);
    asked.wording = Some("Page {page} of {pages}".to_owned());
    asked.family = Some("DejaVu Sans".to_owned());
    let said = desk.stamp(&handle, &asked).expect("stamped");
    assert!(said.contains("Stamped 3 pages"), "{said}");
    assert!(said.contains("Page 1 of 3"), "{said}");
    for page in 0..3 {
        let blocks = desk.blocks(&handle, page).expect("reads");
        let wanted = format!("Page {} of 3", page + 1);
        assert!(
            blocks.iter().any(|block| block.text == wanted),
            "page {page} should read {wanted:?}: {blocks:?}"
        );
    }
    let footer = desk.blocks(&handle, 2).expect("reads");
    let line = footer
        .iter()
        .find(|block| block.text.starts_with("Page"))
        .expect("the number");
    assert!(
        line.area[1] > 842.0 - 60.0,
        "in the footer: {:?}",
        line.area
    );
    assert!(
        desk.walk(&handle, true)
            .expect("one undo takes back all three")
    );
    for page in 0..3 {
        let blocks = desk.blocks(&handle, page).expect("reads");
        assert!(
            blocks.iter().all(|block| !block.text.starts_with("Page")),
            "page {page}: {blocks:?}"
        );
    }
}

#[test]
fn a_watermark_goes_in_the_middle_of_the_pages_asked_for_only() {
    let (mut desk, handle) = three_pages("watermark");
    let mut asked = Asked::new(Kind::Watermark);
    asked.pages = "2".to_owned();
    asked.family = Some("DejaVu Sans".to_owned());
    desk.stamp(&handle, &asked).expect("stamped");
    let middle = desk.blocks(&handle, 1).expect("reads");
    let mark = middle
        .iter()
        .find(|block| block.text == "DRAFT")
        .expect("the watermark");
    assert!(
        f64::midpoint(mark.area[1], mark.area[3]) > 842.0 / 2.0 - 60.0
            && f64::midpoint(mark.area[1], mark.area[3]) < 842.0 / 2.0 + 60.0,
        "{:?}",
        mark.area
    );
    for page in [0, 2] {
        assert!(
            desk.blocks(&handle, page)
                .expect("reads")
                .iter()
                .all(|block| block.text != "DRAFT"),
            "page {page} was left alone"
        );
    }
}
