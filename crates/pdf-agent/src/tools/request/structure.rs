use crate::fielding::{Asked as FieldAsked, Order, kind_of};
use crate::linking::{Action, Goes, Place};
use crate::shaping::{Asked as ShapeAsked, DEFAULT_WIDTH, MOST_WIDTH, Shape};

use crate::json::Json;
use crate::tools::Args;

use super::editing::{optional_colour, optional_number};
use super::{Request, optional_page};

fn corner(args: &Args, key: &str) -> Result<f64, String> {
    args.number(key)
        .filter(|value| value.is_finite())
        .ok_or_else(|| format!("`{key}` is needed, in points from the top-left corner of the page"))
}

fn the_box(args: &Args) -> Result<[f64; 4], String> {
    Ok([
        corner(args, "left")?,
        corner(args, "top")?,
        corner(args, "right")?,
        corner(args, "bottom")?,
    ])
}

fn block_page(name: &str) -> Result<usize, String> {
    let bad = || format!("{name:?} is not a block name: they look like p3-b12");
    let rest = name.trim().strip_prefix('p').ok_or_else(bad)?;
    let (page, block) = rest.split_once("-b").ok_or_else(bad)?;
    let (page, _): (usize, usize) = (
        page.parse().map_err(|_| bad())?,
        block.parse().map_err(|_| bad())?,
    );
    page.checked_sub(1).ok_or_else(bad)
}

pub(super) fn links(args: &Args) -> Result<Request, String> {
    let action = args
        .required("action")
        .map_err(|_| "`action` is needed: list, add or remove".to_owned())?;
    let action = match action {
        "list" => Action::List {
            page: args.page("page")?,
        },
        "add" => add_link(args)?,
        "remove" => {
            let link = args.required("link")?.trim().to_owned();
            crate::linking::parse_name(&link)?;
            Action::Remove { link }
        }
        _ => return Err("`action` is list, add or remove".to_owned()),
    };
    Ok(Request::Links(action))
}

fn add_link(args: &Args) -> Result<Action, String> {
    let goes = match (args.text("url"), optional_page(args, "to_page")?) {
        (Some(_), Some(_)) => return Err("give `url` or `to_page`, not both".to_owned()),
        (Some(url), None) => {
            let url = url.trim();
            if !crate::linking::is_an_address(url) {
                return Err(format!(
                    "{url:?} is not an address a link can open: it starts with http://, \
                     https:// or mailto: and has no spaces"
                ));
            }
            Goes::Address(url.to_owned())
        }
        (None, Some(page)) => Goes::Page(page),
        (None, None) => {
            return Err("`url` or `to_page` is needed: where the link goes".to_owned());
        }
    };
    let listed = ["left", "top", "right", "bottom"]
        .iter()
        .filter(|key| args.has(key))
        .count();
    let given = optional_page(args, "page")?;
    let (page, place) = match (args.text("block"), listed) {
        (Some(_), 1..) => return Err("give `block` or the box, not both".to_owned()),
        (Some(name), 0) => {
            let from_block = block_page(name)?;
            if given.is_some_and(|page| page != from_block) {
                return Err(format!(
                    "{name} is on page {}, not page {}",
                    from_block + 1,
                    given.map_or(0, |page| page + 1)
                ));
            }
            (from_block, Place::Block(name.trim().to_owned()))
        }
        (None, 0) => {
            return Err(
                "`block` or a box (`left`, `top`, `right`, `bottom`) is needed: what the link \
                 goes over"
                    .to_owned(),
            );
        }
        (None, _) => {
            let page = given.ok_or("`page` is needed with a box, as a page number from 1")?;
            (page, Place::Area(the_box(args)?))
        }
    };
    Ok(Action::Add { page, place, goes })
}

pub(super) fn draw_shape(args: &Args) -> Result<Request, String> {
    let shape = args
        .required("shape")
        .map_err(|_| "`shape` is needed: rectangle, ellipse, line or arrow".to_owned())
        .and_then(|word| {
            Shape::of(word).ok_or_else(|| "`shape` is rectangle, ellipse, line or arrow".to_owned())
        })?;
    let [left, top, right, bottom] = the_box(args)?;
    let asked = ShapeAsked {
        page: args.page("page")?,
        shape,
        from: (left, top),
        to: (right, bottom),
        colour: optional_colour(args, "color")?.unwrap_or([0.0; 3]),
        width: optional_number(args, "width", (0.1, MOST_WIDTH), " points")?
            .unwrap_or(DEFAULT_WIDTH),
        fill: optional_colour(args, "fill")?,
    };
    asked.check()?;
    Ok(Request::DrawShape(asked))
}

pub(super) fn add_field(args: &Args) -> Result<Request, String> {
    let kind = args
        .required("kind")
        .map_err(|_| {
            "`kind` is needed: text, paragraph, checkbox, radio, dropdown, list, date, signature or button"
                .to_owned()
        })
        .and_then(|word| {
            kind_of(word).ok_or_else(|| {
                "`kind` is text, paragraph, checkbox, radio, dropdown, list, date, signature or button"
                    .to_owned()
            })
        })?;
    let (left, top) = (corner(args, "left")?, corner(args, "top")?);
    let size = |key: &str| {
        args.number(key)
            .filter(|value| value.is_finite() && *value > 0.0)
            .ok_or_else(|| format!("`{key}` is needed, in points, above 0"))
    };
    let area = [left, top, left + size("width")?, top + size("height")?];
    let name = match args.text("name") {
        None => None,
        Some(name) if name.trim().is_empty() => return Err("`name` is empty".to_owned()),
        Some(name) => Some(name.trim().to_owned()),
    };
    let options = match args.0.get("options") {
        None | Some(Json::Null) => Vec::new(),
        Some(Json::List(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()
            .ok_or("`options` is a list of text")?,
        Some(_) => return Err("`options` is a list of text".to_owned()),
    };
    if options.len() > crate::fielding::MOST_OPTIONS {
        return Err(format!(
            "`options` holds {} choices: the most one field offers is {}",
            options.len(),
            crate::fielding::MOST_OPTIONS
        ));
    }
    let asked = FieldAsked {
        page: args.page("page")?,
        kind,
        area,
        name,
        options,
    };
    asked.check()?;
    Ok(Request::AddField(asked))
}

pub(super) fn set_tab_order(args: &Args) -> Result<Request, String> {
    let order = args
        .required("order")
        .map_err(|_| "`order` is needed: rows, columns or structure".to_owned())
        .and_then(|word| {
            Order::of(word).ok_or_else(|| "`order` is rows, columns or structure".to_owned())
        })?;
    Ok(Request::SetTabOrder {
        page: args.page("page")?,
        order,
    })
}
