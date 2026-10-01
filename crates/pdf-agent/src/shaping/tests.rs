use pdf_edit::{Command, PenStep};

use super::{Asked, DEFAULT_WIDTH, Shape, command, said};
use crate::desk::device_of;
use crate::desk::tests::with_paragraphs;
use crate::json::Json;
use crate::tools::request::{Request, parse};

fn read(arguments: &str) -> Result<Asked, String> {
    match parse(
        "draw_shape",
        &Json::parse(arguments).expect("the arguments in this test are JSON"),
    )? {
        Request::DrawShape(asked) => Ok(asked),
        other => panic!("draw_shape read as {other:?}"),
    }
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the numbers are read straight from the text, not computed"
)]
fn a_shape_is_read_with_its_corners_its_colour_and_its_thickness() {
    let asked = read(
        r##"{"document":"doc-1","page":2,"shape":"rectangle","left":100,"top":120,"right":300,"bottom":220,"color":"#ff0000","width":4,"fill":"#00ff00"}"##,
    )
    .expect("reads");
    assert_eq!(asked.page, 1);
    assert_eq!(asked.shape, Shape::Rectangle);
    assert_eq!((asked.from, asked.to), ((100.0, 120.0), (300.0, 220.0)));
    assert_eq!(asked.colour, [1.0, 0.0, 0.0]);
    assert!((asked.width - 4.0).abs() < 1e-9);
    assert_eq!(asked.fill, Some([0.0, 1.0, 0.0]));
    let plain = read(
        r#"{"document":"doc-1","page":1,"shape":"arrow","left":0,"top":0,"right":50,"bottom":50}"#,
    )
    .expect("reads");
    assert_eq!(plain.colour, [0.0; 3]);
    assert!((plain.width - DEFAULT_WIDTH).abs() < 1e-9);
    assert_eq!(plain.fill, None);
    for word in ["rectangle", "ellipse", "line", "arrow"] {
        assert_eq!(Shape::of(word).map(Shape::as_str), Some(word));
    }
    assert_eq!(Shape::of("oval"), Some(Shape::Ellipse));
}

#[test]
fn a_shape_with_no_size_or_a_wrong_word_is_refused() {
    for (bad, said) in [
        (
            r#"{"document":"doc-1","page":1,"shape":"star","left":0,"top":0,"right":9,"bottom":9}"#,
            "rectangle, ellipse, line or arrow",
        ),
        (
            r#"{"document":"doc-1","page":1,"left":0,"top":0,"right":9,"bottom":9}"#,
            "`shape` is needed",
        ),
        (
            r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":9}"#,
            "`bottom` is needed",
        ),
        (
            r#"{"document":"doc-1","page":1,"shape":"rectangle","left":5,"top":5,"right":5,"bottom":90}"#,
            "less than 1 point",
        ),
        (
            r#"{"document":"doc-1","page":1,"shape":"line","left":5,"top":5,"right":5,"bottom":5}"#,
            "less than 1 point",
        ),
        (
            r#"{"document":"doc-1","shape":"line","left":0,"top":0,"right":9,"bottom":9}"#,
            "`page` is needed",
        ),
        (
            r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":9,"bottom":9,"width":500}"#,
            "`width` is a number from 0.1 to 100",
        ),
        (
            r#"{"document":"doc-1","page":1,"shape":"line","left":0,"top":0,"right":9,"bottom":9,"color":"red"}"#,
            "is not a colour",
        ),
    ] {
        let why = read(bad).expect_err(bad);
        assert!(why.contains(said), "{bad}: {why}");
    }
    assert!(
        read(
            r#"{"document":"doc-1","page":1,"shape":"line","left":9,"top":9,"right":0,"bottom":0}"#
        )
        .is_ok(),
        "negative control: a line may run up and to the left"
    );
}

#[test]
fn the_outline_of_each_shape_is_where_its_corners_are_and_an_arrow_has_a_head() {
    let at = |shape, from, to| Asked {
        page: 0,
        shape,
        from,
        to,
        colour: [0.0; 3],
        width: 2.0,
        fill: None,
    };
    let rectangle = at(Shape::Rectangle, (50.0, 80.0), (10.0, 20.0)).steps();
    assert_eq!(rectangle.len(), 4, "the engine closes it");
    assert_eq!(
        rectangle[0],
        PenStep::Move((10.0, 20.0)),
        "corners from either direction"
    );
    assert_eq!(rectangle[2], PenStep::Line((50.0, 80.0)));
    let ellipse = at(Shape::Ellipse, (0.0, 0.0), (100.0, 40.0)).steps();
    assert_eq!(ellipse.len(), 5, "a start and four curves");
    assert_eq!(ellipse[0], PenStep::Move((100.0, 20.0)));
    let line = at(Shape::Line, (1.0, 2.0), (30.0, 40.0)).steps();
    assert_eq!(
        line,
        [PenStep::Move((1.0, 2.0)), PenStep::Line((30.0, 40.0))]
    );
    let arrow = at(Shape::Arrow, (0.0, 0.0), (100.0, 0.0)).steps();
    assert_eq!(arrow.len(), 6, "the shaft and two sides of the head");
    assert_eq!(
        arrow[2],
        PenStep::Move((100.0, 0.0)),
        "the head is at the end"
    );
    let PenStep::Line((x, y)) = arrow[3] else {
        panic!("a side of the head");
    };
    assert!(
        x < 100.0 && y.abs() > 0.0,
        "swept back from the tip: {x},{y}"
    );
    let short = at(Shape::Arrow, (0.0, 0.0), (4.0, 0.0)).steps();
    let PenStep::Line((back, _)) = short[3] else {
        panic!("a side of the head");
    };
    assert!(
        back >= 2.0 - 1e-9,
        "the head of a short arrow stays on it: {back}"
    );
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "the numbers are read straight from the text, not computed"
)]
fn a_shape_is_drawn_in_user_space_with_a_line_as_thick_as_asked_and_off_page_refused() {
    let (mut desk, handle) = with_paragraphs("shaping-command", &["x"]);
    let geometry = desk.page_geometries(&handle).expect("measured")[0];
    let device = device_of(&geometry).expect("a device");
    let asked = read(
        r##"{"document":"doc-1","page":1,"shape":"rectangle","left":100,"top":100,"right":200,"bottom":150,"width":3,"fill":"#0000ff"}"##,
    )
    .expect("reads");
    let page = [595.0, 842.0];
    let Command::DrawPath {
        steps,
        closed,
        stroke,
        fill,
        ..
    } = command(&asked, &device.matrix, page).expect("a command")
    else {
        panic!("a DrawPath");
    };
    assert!(closed && fill == Some([0.0, 0.0, 1.0]));
    assert_eq!(
        steps[0],
        PenStep::Move((100.0, 742.0)),
        "the top is 100 points down: 842 - 100"
    );
    let stroke = stroke.expect("a stroke");
    assert!((stroke.width - 3.0).abs() < 1e-9 && stroke.colour == [0.0; 3]);
    let open = Asked {
        shape: Shape::Line,
        fill: Some([1.0; 3]),
        ..asked.clone()
    };
    let Command::DrawPath { fill, closed, .. } =
        command(&open, &device.matrix, page).expect("a line")
    else {
        panic!("a DrawPath");
    };
    assert!(!closed && fill.is_none(), "a line is not filled");
    let off = Asked {
        from: (700.0, 900.0),
        to: (800.0, 950.0),
        ..asked.clone()
    };
    assert!(
        command(&off, &device.matrix, page)
            .expect_err("refused")
            .contains("lies off the page")
    );
    assert_eq!(
        said(&asked),
        "Drew a rectangle on page 1 over [100, 100, 200, 150], filled, as one step undo takes \
         back. objects with action list names it."
    );
    assert!(said(&open).starts_with("Drew a line on page 1"));
    assert!(
        said(&Asked {
            shape: Shape::Arrow,
            ..open
        })
        .starts_with("Drew an arrow")
    );
}

#[test]
fn a_drawn_shape_is_listed_as_a_drawing_and_is_one_undo_step() {
    let (mut desk, handle) = with_paragraphs("shaping-desk", &["Words."]);
    let said = desk
        .draw_shape(
            &handle,
            &read(
                r#"{"document":"doc-1","page":1,"shape":"ellipse","left":200,"top":300,"right":400,"bottom":400}"#,
            )
            .expect("reads"),
        )
        .expect("drawn");
    assert!(
        said.starts_with("Drew an ellipse on page 1 over [200, 300, 400, 400]"),
        "{said}"
    );
    let listed = desk
        .objects(&handle, &crate::objects::Action::List { page: 0 })
        .expect("listed");
    assert!(
        listed.contains("p1-o1 drawing [200, 300, 400, 400]"),
        "{listed}"
    );
    assert!(desk.walk(&handle, true).expect("undone"));
    let after = desk
        .objects(&handle, &crate::objects::Action::List { page: 0 })
        .expect("listed");
    assert!(
        !after.contains("p1-o1"),
        "one undo takes the whole shape away: {after}"
    );
    let off = desk
        .draw_shape(
            &handle,
            &read(
                r#"{"document":"doc-1","page":1,"shape":"line","left":1000,"top":1000,"right":1100,"bottom":1100}"#,
            )
            .expect("reads"),
        )
        .expect_err("refused");
    assert!(off.contains("lies off the page"), "{off}");
    let far = desk
        .draw_shape(
            &handle,
            &Asked {
                page: 4,
                ..read(
                    r#"{"document":"doc-1","page":1,"shape":"line","left":1,"top":1,"right":9,"bottom":9}"#,
                )
                .expect("reads")
            },
        )
        .expect_err("refused");
    assert!(far.contains("there is no page 5"), "{far}");
}
