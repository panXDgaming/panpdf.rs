use crate::finding::Search;
use crate::json::Json;
use crate::marking::{How, Marking};
use crate::objects::{self, Action as ObjectAction};
use crate::outlining::{Action as BookmarkAction, Place, Step};
use crate::pictures::{Asked as PictureAsked, Source};
use crate::stamping::{Asked as StampAsked, Kind, only_of, spot_of};
use crate::styling::{Align, LEAST_SPACING, Look, MOST_SPACING};
use crate::tools::{Args, colour, expand};

use super::{Request, optional_page};

pub const MOST_DPI: f64 = 600.0;

const LEAST_DPI: f64 = 20.0;

const DEFAULT_DPI: f64 = 200.0;

pub(super) fn optional_flag(args: &Args, key: &str) -> Result<Option<bool>, String> {
    match args.0.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(on)) => Ok(Some(*on)),
        Some(_) => Err(format!("`{key}` is true or false")),
    }
}

pub(super) fn optional_number(
    args: &Args,
    key: &str,
    (least, most): (f64, f64),
    unit: &str,
) -> Result<Option<f64>, String> {
    if !args.has(key) {
        return Ok(None);
    }
    args.number(key)
        .filter(|value| (least..=most).contains(value))
        .map(Some)
        .ok_or_else(|| format!("`{key}` is a number from {least} to {most}{unit}"))
}

pub(super) fn optional_count(args: &Args, key: &str) -> Result<Option<usize>, String> {
    if !args.has(key) {
        return Ok(None);
    }
    args.0
        .get(key)
        .and_then(Json::as_count)
        .filter(|number| *number >= 1)
        .map(Some)
        .ok_or_else(|| format!("`{key}` is a whole number from 1"))
}

pub(super) fn optional_colour(args: &Args, key: &str) -> Result<Option<[f64; 3]>, String> {
    if !args.has(key) {
        return Ok(None);
    }
    args.text(key)
        .ok_or_else(|| format!("`{key}` is a colour like #1a2b3c"))
        .and_then(colour)
        .map(Some)
}

fn search(args: &Args, key: &str) -> Result<Search, String> {
    let wanted = args.required(key)?;
    if wanted.is_empty() {
        return Err(format!("`{key}` is empty"));
    }
    Ok(Search {
        wanted: wanted.to_owned(),
        match_case: args.flag("match_case"),
        whole_words: args.flag("whole_words"),
    })
}

fn span(args: &Args) -> Result<(Option<usize>, Option<usize>), String> {
    let first = optional_page(args, "first_page")?;
    let last = optional_page(args, "last_page")?;
    if let (Some(first), Some(last)) = (first, last)
        && last < first
    {
        return Err("`last_page` comes before `first_page`".to_owned());
    }
    Ok((first, last))
}

pub(super) fn find_and_replace(args: &Args) -> Result<Request, String> {
    let search = search(args, "find")?;
    let with = args.required("replace_with")?.to_owned();
    if with == search.wanted {
        return Err(
            "`replace_with` is the same words as `find`: there is nothing to change".to_owned(),
        );
    }
    let (first, last) = span(args)?;
    Ok(Request::FindAndReplace {
        search,
        with,
        first,
        last,
    })
}

pub(super) fn style_text(args: &Args) -> Result<Request, String> {
    let block = args.required("block")?.to_owned();
    let find = match args.text("find") {
        Some("") => return Err("`find` is empty".to_owned()),
        other => other.map(str::to_owned),
    };
    let align = match args.text("align") {
        None => None,
        Some(word) => Some(Align::of(word).ok_or("`align` is left, center, right or justify")?),
    };
    let look = Look {
        bold: optional_flag(args, "bold")?,
        italic: optional_flag(args, "italic")?,
        underline: optional_flag(args, "underline")?,
        size: optional_number(args, "size", (1.0, 1000.0), " points")?,
        fill: optional_colour(args, "color")?,
        family: args.text("font").map(str::to_owned),
        line_spacing: optional_number(
            args,
            "line_spacing",
            (LEAST_SPACING, MOST_SPACING),
            ", as a multiple of the text size",
        )?,
        align,
    };
    if look.asks_for_nothing() {
        return Err(
            "nothing to change: pass bold, italic, underline, size, color, font, line_spacing or align"
                .to_owned(),
        );
    }
    Ok(Request::StyleText { block, find, look })
}

pub(super) fn mark_text(args: &Args) -> Result<Request, String> {
    let search = search(args, "text")?;
    let how = match args.text("how") {
        None => How::Highlight,
        Some(word) => How::of(word).ok_or("`how` is highlight, underline or strike_through")?,
    };
    let (first, last) = span(args)?;
    Ok(Request::MarkText {
        search,
        marking: Marking::new(how, optional_colour(args, "color")?),
        first,
        last,
    })
}

pub(super) fn add_stamp(args: &Args) -> Result<Request, String> {
    let kind = args.required("kind").and_then(|word| {
        Kind::of(word)
            .ok_or_else(|| "`kind` is page_numbers, header_footer or watermark".to_owned())
    })?;
    let mut asked = StampAsked::new(kind);
    if let Some(text) = args.text("text") {
        if text.trim().is_empty() {
            return Err("`text` is empty: there is nothing to put on the pages".to_owned());
        }
        asked.wording = Some(text.to_owned());
    }
    if let Some(word) = args.text("position") {
        asked.spot = Some(spot_of(word).ok_or(
            "`position` is header_left, header_centre, header_right, footer_left, footer_centre, \
             footer_right or middle",
        )?);
    }
    args.text("pages")
        .unwrap_or_default()
        .clone_into(&mut asked.pages);
    if let Some(word) = args.text("only") {
        asked.only = only_of(word).ok_or("`only` is every, odd or even")?;
    }
    if args.has("start_number") {
        let start = args
            .number("start_number")
            .filter(|number| number.fract() == 0.0 && number.abs() < 1e9)
            .ok_or("`start_number` is a whole number")?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a whole number below a billion, checked above"
        )]
        let start = start as i64;
        asked.start = Some(start);
    }
    asked.family = args.text("font").map(str::to_owned);
    asked.size = optional_number(args, "size", (1.0, 500.0), " points")?;
    asked.bold = optional_flag(args, "bold")?;
    asked.italic = optional_flag(args, "italic")?.unwrap_or(false);
    asked.fill = optional_colour(args, "color")?;
    asked.opacity =
        optional_number(args, "opacity", (1.0, 100.0), " percent")?.map(|percent| percent / 100.0);
    asked.margin = optional_number(args, "margin", (0.0, 300.0), " points")?;
    Ok(Request::AddStamp(asked))
}

fn bookmark_number(args: &Args) -> Result<usize, String> {
    optional_count(args, "bookmark")?
        .ok_or_else(|| "`bookmark` is needed: its number in the list".to_owned())
}

fn page_of(args: &Args, key: &str) -> Result<usize, String> {
    optional_page(args, key)?.ok_or_else(|| format!("`{key}` is needed, as a page number from 1"))
}

pub(super) fn bookmarks(args: &Args) -> Result<Request, String> {
    let action = args.required("action").map_err(|_| {
        "`action` is needed: list, add, rename, retarget, move, delete or from_headings".to_owned()
    })?;
    let action = match action {
        "list" => BookmarkAction::List,
        "add" => {
            let place = match (optional_page(args, "page")?, args.text("block")) {
                (Some(_), Some(_)) => return Err("give `page` or `block`, not both".to_owned()),
                (Some(page), None) => Place::Page(page),
                (None, Some(block)) => Place::Block(block.to_owned()),
                (None, None) => {
                    return Err("`page` or `block` is needed: what the bookmark goes to".to_owned());
                }
            };
            let title = args.text("title").map(str::to_owned);
            if title
                .as_deref()
                .is_some_and(|title| title.trim().is_empty())
                || (title.is_none() && matches!(place, Place::Page(_)))
            {
                return Err("`title` is needed: say what to call the bookmark".to_owned());
            }
            BookmarkAction::Add {
                title,
                place,
                after: optional_count(args, "after")?,
                inside: optional_count(args, "inside")?,
            }
        }
        "rename" => {
            let title = args.required("title")?.trim().to_owned();
            if title.is_empty() {
                return Err("`title` is empty".to_owned());
            }
            BookmarkAction::Rename {
                bookmark: bookmark_number(args)?,
                title,
            }
        }
        "retarget" => BookmarkAction::Retarget {
            bookmark: bookmark_number(args)?,
            page: page_of(args, "page")?,
        },
        "move" => BookmarkAction::Move {
            bookmark: bookmark_number(args)?,
            step: args
                .required("direction")
                .map_err(|_| "`direction` is needed: up, down, in or out".to_owned())
                .and_then(|word| {
                    Step::of(word).ok_or_else(|| "`direction` is up, down, in or out".to_owned())
                })?,
        },
        "delete" => BookmarkAction::Delete {
            bookmark: bookmark_number(args)?,
        },
        "from_headings" => BookmarkAction::FromHeadings {
            replace: args.flag("replace"),
        },
        _ => {
            return Err(
                "`action` is list, add, rename, retarget, move, delete or from_headings".to_owned(),
            );
        }
    };
    Ok(Request::Bookmarks(action))
}

pub(super) fn place_picture(args: &Args) -> Result<Request, String> {
    let source = match (optional_count(args, "attachment")?, args.text("path")) {
        (Some(_), Some(_)) => return Err("give `attachment` or `path`, not both".to_owned()),
        (Some(number), None) => Source::Attachment(Some(number)),
        (None, Some(path)) if path.trim().is_empty() => return Err("`path` is empty".to_owned()),
        (None, Some(path)) => Source::File(expand(path)),
        (None, None) => Source::Attachment(None),
    };
    let size = |key: &str| optional_number(args, key, (1.0, 20_000.0), " points");
    Ok(Request::PlacePicture(PictureAsked {
        page: args.page("page")?,
        left: args.number("left").ok_or("`left` is needed, in points")?,
        top: args.number("top").ok_or("`top` is needed, in points")?,
        width: size("width")?,
        height: size("height")?,
        source,
    }))
}

pub(super) fn objects(args: &Args) -> Result<Request, String> {
    let action = args
        .required("action")
        .map_err(|_| "`action` is needed: list, move, resize or delete".to_owned())?;
    let object = || -> Result<String, String> {
        let name = args.required("object")?.trim().to_owned();
        objects::parse_name(&name)?;
        Ok(name)
    };
    let size = |key: &str| optional_number(args, key, (1.0, 20_000.0), " points");
    let position = |key: &str| optional_number(args, key, (-20_000.0, 20_000.0), " points");
    let action = match action {
        "list" => ObjectAction::List {
            page: args.page("page")?,
        },
        "move" => ObjectAction::Move {
            object: object()?,
            left: position("left")?,
            top: position("top")?,
        },
        "resize" => ObjectAction::Resize {
            object: object()?,
            width: size("width")?,
            height: size("height")?,
        },
        "delete" => ObjectAction::Delete { object: object()? },
        _ => return Err("`action` is list, move, resize or delete".to_owned()),
    };
    Ok(Request::Objects(action))
}

pub(super) fn look_closer(args: &Args) -> Result<Request, String> {
    let number = |key: &str| {
        let from = if key == "left" || key == "right" {
            "left"
        } else {
            "top"
        };
        args.number(key)
            .ok_or_else(|| format!("`{key}` is needed, in points from the {from} edge of the page"))
    };
    let (left, top, right, bottom) = (
        number("left")?,
        number("top")?,
        number("right")?,
        number("bottom")?,
    );
    if !(right > left && bottom > top) {
        return Err(
            "the region is empty: `right` is more than `left` and `bottom` more than `top`"
                .to_owned(),
        );
    }
    let dpi = if args.has("dpi") {
        args.number("dpi")
            .filter(|dpi| (LEAST_DPI..=MOST_DPI).contains(dpi))
            .ok_or_else(|| format!("`dpi` is a number from {LEAST_DPI} to {MOST_DPI}"))?
    } else {
        DEFAULT_DPI
    };
    Ok(Request::LookCloser {
        page: args.page("page")?,
        region: [left, top, right, bottom],
        dpi,
    })
}
