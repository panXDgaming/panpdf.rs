use std::path::PathBuf;
use std::sync::Arc;

use crate::desk::tests::{view_of_page, with_paragraphs};

use super::{Asked, Source, fit, placement, read_file, size_of};

pub(crate) fn red_square(across: u32, down: u32) -> Arc<[u8]> {
    let rgb: Vec<u8> = std::iter::repeat_n([220_u8, 20, 20], (across * down) as usize)
        .flatten()
        .collect();
    Arc::from(pdf_edit::png::write((across, down), &rgb, None).expect("a picture"))
}

fn assert_near(got: [f64; 4], wanted: [f64; 4], why: &str) {
    assert!(
        got.iter()
            .zip(&wanted)
            .all(|(one, other)| (one - other).abs() < 1e-6),
        "{why}: {got:?} is not {wanted:?}"
    );
}

fn asked(width: Option<f64>, height: Option<f64>) -> Asked {
    Asked {
        page: 0,
        left: 100.0,
        top: 100.0,
        width,
        height,
        source: Source::File(PathBuf::from("a.png")),
    }
}

#[test]
fn a_picture_read_from_its_bytes_has_its_own_size() {
    let picture = red_square(40, 20);
    assert_eq!(size_of(&picture).expect("a size"), (40.0, 20.0));
    let why = size_of(b"not a picture at all").expect_err("not a picture");
    assert!(why.contains("not a picture"), "{why}");
}

#[test]
fn a_width_or_a_height_alone_keeps_the_pictures_shape() {
    let page = [595.0, 842.0];
    let wide = fit(&asked(Some(200.0), None), (40.0, 20.0), page).expect("fits");
    assert_near(wide, [100.0, 100.0, 300.0, 200.0], "as wide as asked");
    let tall = fit(&asked(None, Some(50.0)), (40.0, 20.0), page).expect("fits");
    assert_near(tall, [100.0, 100.0, 200.0, 150.0], "as tall as asked");
}

#[test]
fn both_a_width_and_a_height_make_a_box_the_picture_fits_inside() {
    let page = [595.0, 842.0];
    let inside = fit(&asked(Some(100.0), Some(100.0)), (40.0, 20.0), page).expect("fits");
    assert_near(
        inside,
        [100.0, 100.0, 200.0, 150.0],
        "as wide as the box, and no taller than its shape",
    );
    let narrow = fit(&asked(Some(400.0), Some(50.0)), (40.0, 20.0), page).expect("fits");
    assert_near(narrow, [100.0, 100.0, 200.0, 150.0], "as tall as the box");
}

#[test]
fn a_picture_with_no_size_asked_is_its_own_size_but_never_wider_than_200_points() {
    let page = [595.0, 842.0];
    let small = fit(&asked(None, None), (40.0, 20.0), page).expect("fits");
    assert_near(
        small,
        [100.0, 100.0, 130.0, 115.0],
        "40 pixels are 30 points",
    );
    let big = fit(&asked(None, None), (2000.0, 1000.0), page).expect("fits");
    assert_near(
        big,
        [100.0, 100.0, 300.0, 200.0],
        "never wider than 200 points",
    );
    let tall = fit(&asked(None, None), (100.0, 4000.0), page).expect("fits");
    assert!(tall[3] - tall[1] <= 842.0 * 0.8 + 1e-9, "{tall:?}");
}

#[test]
fn a_picture_that_runs_off_the_page_is_refused_with_both_sizes() {
    let why = fit(&asked(Some(600.0), None), (40.0, 20.0), [595.0, 842.0]).expect_err("too wide");
    assert!(why.contains("runs off the page, 595 x 842 pt"), "{why}");
    let mut off = asked(Some(10.0), None);
    off.left = -40.0;
    assert!(fit(&off, (40.0, 20.0), [595.0, 842.0]).is_err());
    let none = fit(&asked(Some(0.0), None), (40.0, 20.0), [595.0, 842.0]).expect_err("no size");
    assert!(none.contains("no size"), "{none}");
}

#[test]
fn a_placement_carries_the_unit_square_to_the_box_as_shown() {
    let (mut desk, handle) = with_paragraphs("placement", &["x"]);
    let view = view_of_page(&mut desk, &handle, 0);
    let matrix = placement(&view, [100.0, 100.0, 300.0, 200.0]).expect("a placement");
    let corner = |x: f64, y: f64| matrix.transform(pdf_paint::Point { x, y });
    let (low_left, high_right) = (corner(0.0, 0.0), corner(1.0, 1.0));
    assert!((low_left.x - 100.0).abs() < 1e-6 && (low_left.y - (842.0 - 200.0)).abs() < 1e-6);
    assert!((high_right.x - 300.0).abs() < 1e-6 && (high_right.y - (842.0 - 100.0)).abs() < 1e-6);
}

#[test]
fn a_file_that_is_not_there_or_is_a_folder_says_so() {
    let missing = read_file(std::path::Path::new("/nowhere/at/all.png")).expect_err("missing");
    assert!(missing.contains("cannot be read"), "{missing}");
    let folder = read_file(&std::env::temp_dir()).expect_err("a folder");
    assert!(folder.contains("is not a file"), "{folder}");
}
