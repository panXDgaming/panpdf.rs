use crate::desk::Desk;
use crate::finding::{said_marked, said_replaced};
use crate::json::Json;
use crate::tools::request::{Request, parse_arguments};
use crate::tools::{Answer, Args, page_span};

fn counted(per_page: &[(usize, usize)]) -> Json {
    Json::List(
        per_page
            .iter()
            .map(|(page, count)| {
                Json::object([
                    ("page", Json::count(page + 1)),
                    ("count", Json::count(*count)),
                ])
            })
            .collect(),
    )
}

fn not_this(name: &str) -> String {
    format!("{name} was not read as the tool it names")
}

pub(super) fn find_and_replace(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::FindAndReplace {
        search,
        with,
        first,
        last,
    } = parse_arguments("find_and_replace", args)?
    else {
        return Err(not_this("find_and_replace"));
    };
    let pages = desk.page_count(handle)?;
    let span = page_span(first, last, pages)?;
    let replaced = desk.replace_everywhere(handle, (&search, &with), span)?;
    Ok(Answer::of(
        said_replaced((&search, &with), &replaced, (span, pages)),
        Json::object([
            ("replaced", Json::count(replaced.total())),
            ("pages", counted(&replaced.per_page)),
        ]),
    ))
}

pub(super) fn style_text(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::StyleText { block, find, look } = parse_arguments("style_text", args)? else {
        return Err(not_this("style_text"));
    };
    let now = desk.style(handle, &block, find.as_deref(), &look)?;
    Ok(Answer::of(
        format!(
            "Styled {}{}: {}.",
            now.name(),
            find.as_ref().map_or_else(String::new, |find| format!(
                " (the words \u{201c}{find}\u{201d})"
            )),
            look.words()
        ),
        Json::object([("block", Json::text(now.name()))]),
    ))
}

pub(super) fn mark_text(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::MarkText {
        search,
        marking,
        first,
        last,
    } = parse_arguments("mark_text", args)?
    else {
        return Err(not_this("mark_text"));
    };
    let pages = desk.page_count(handle)?;
    let span = page_span(first, last, pages)?;
    let marked = desk.mark_text(handle, (&search, marking), span)?;
    Ok(Answer::of(
        said_marked((&search, marking.how.done()), &marked, (span, pages)),
        Json::object([
            ("marked", Json::count(marked.total())),
            ("pages", counted(&marked.per_page)),
        ]),
    ))
}

pub(super) fn add_stamp(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::AddStamp(asked) = parse_arguments("add_stamp", args)? else {
        return Err(not_this("add_stamp"));
    };
    let said = desk.stamp(handle, &asked)?;
    Ok(Answer::of(
        format!("{said}, as one step undo takes back."),
        Json::Null,
    ))
}

pub(super) fn bookmarks(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::Bookmarks(action) = parse_arguments("bookmarks", args)? else {
        return Err(not_this("bookmarks"));
    };
    Ok(Answer::of(desk.bookmarks(handle, &action)?, Json::Null))
}

pub(super) fn place_picture(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    use crate::pictures::Source;
    let handle = args.required("document")?;
    let Request::PlacePicture(asked) = parse_arguments("place_picture", args)? else {
        return Err(not_this("place_picture"));
    };
    let Source::File(path) = &asked.source else {
        return Err(
            "there is no chat here to take a picture from: give `path`, a PNG or JPEG file"
                .to_owned(),
        );
    };
    let file = crate::pictures::read_file(path)?;
    let area = desk.place_picture(handle, &asked, file)?;
    Ok(Answer::of(
        crate::pictures::said(asked.page, area),
        Json::object([
            ("page", Json::count(asked.page + 1)),
            (
                "box",
                Json::List(area.iter().map(|value| Json::Number(*value)).collect()),
            ),
        ]),
    ))
}

pub(super) fn objects(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::Objects(action) = parse_arguments("objects", args)? else {
        return Err(not_this("objects"));
    };
    Ok(Answer::of(desk.objects(handle, &action)?, Json::Null))
}

pub(super) fn look_closer(desk: &mut Desk, args: &Args) -> Result<Answer, String> {
    let handle = args.required("document")?;
    let Request::LookCloser { page, region, dpi } = parse_arguments("look_closer", args)? else {
        return Err(not_this("look_closer"));
    };
    let (png, width, height) = desk.look_closer(handle, page, region, dpi)?;
    Ok(Answer {
        text: crate::pictures::said_close(page, region, (width, height)),
        data: Json::object([
            ("page", Json::count(page + 1)),
            ("width", Json::Number(f64::from(width))),
            ("height", Json::Number(f64::from(height))),
        ]),
        picture: Some(png),
    })
}
