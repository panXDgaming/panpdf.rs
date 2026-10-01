use pdf_edit::Command;
use pdf_edit::form::FormField;
use pdf_edit::new_field::NewFieldKind;
use pdf_edit::tab_order::{TabOrder, in_reading_order};
use pdf_paint::Matrix;
use pdf_syntax::Reference;

use crate::desk;

const LEAST_SIDE: f64 = 6.0;

pub const MOST_OPTIONS: usize = pdf_edit::new_field::MOST_OPTIONS;

#[must_use]
pub fn kind_of(word: &str) -> Option<NewFieldKind> {
    match word.trim() {
        "text" => Some(NewFieldKind::Text),
        "paragraph" | "multiline" => Some(NewFieldKind::Paragraph),
        "checkbox" | "check_box" => Some(NewFieldKind::Checkbox),
        "radio" | "radio_button" => Some(NewFieldKind::Radio),
        "dropdown" | "combo" => Some(NewFieldKind::Dropdown),
        "list" | "list_box" => Some(NewFieldKind::ListBox),
        "date" => Some(NewFieldKind::Date),
        "signature" => Some(NewFieldKind::Signature),
        "button" => Some(NewFieldKind::Button),
        _ => None,
    }
}

#[must_use]
pub const fn kind_name(kind: NewFieldKind) -> &'static str {
    match kind {
        NewFieldKind::Text => "text",
        NewFieldKind::Paragraph => "paragraph",
        NewFieldKind::Checkbox => "checkbox",
        NewFieldKind::Radio => "radio",
        NewFieldKind::Dropdown => "dropdown",
        NewFieldKind::ListBox => "list",
        NewFieldKind::Date => "date",
        NewFieldKind::Signature => "signature",
        NewFieldKind::Button => "button",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub page: usize,
    pub kind: NewFieldKind,
    pub area: [f64; 4],
    pub name: Option<String>,
    pub options: Vec<String>,
}

impl Asked {
    pub fn check(&self) -> Result<(), String> {
        let [left, top, right, bottom] = self.area;
        if !(right - left >= LEAST_SIDE && bottom - top >= LEAST_SIDE) {
            return Err(format!(
                "the box [{left:.0}, {top:.0}, {right:.0}, {bottom:.0}] is too small for a field: \
                 it is at least {LEAST_SIDE:.0} points each way"
            ));
        }
        let wants_options = matches!(self.kind, NewFieldKind::Dropdown | NewFieldKind::ListBox);
        if self.kind == NewFieldKind::Dropdown && self.options.is_empty() {
            return Err("a dropdown needs `options`, the choices it offers".to_owned());
        }
        if !self.options.is_empty() && !wants_options && self.kind != NewFieldKind::Button {
            return Err(format!(
                "a {} field takes no `options`: only dropdowns and lists have choices, and a \
                 button's one option is its caption",
                kind_name(self.kind)
            ));
        }
        if self.kind == NewFieldKind::Button && self.options.len() > 1 {
            return Err("a button has one caption: give one option".to_owned());
        }
        Ok(())
    }
}

pub fn add_command(asked: &Asked, shown: &Matrix) -> Result<Command, String> {
    asked.check()?;
    Ok(Command::AddField {
        page_index: asked.page,
        rect: desk::to_user_with(shown, asked.area)?,
        kind: asked.kind,
        name: asked.name.clone(),
        options: asked.options.clone(),
    })
}

#[must_use]
pub fn said_added(asked: &Asked) -> String {
    let named = asked
        .name
        .as_ref()
        .map_or_else(String::new, |name| format!(" named \u{201c}{name}\u{201d}"));
    format!(
        "Added a {} field{named} on page {} over [{:.0}, {:.0}, {:.0}, {:.0}], as one step undo \
         takes back. document_info lists it, and fill_field can fill it.",
        kind_name(asked.kind),
        asked.page + 1,
        asked.area[0],
        asked.area[1],
        asked.area[2],
        asked.area[3]
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Order {
    Rows,
    Columns,
    Structure,
}

impl Order {
    #[must_use]
    pub fn of(word: &str) -> Option<Self> {
        match word.trim() {
            "rows" => Some(Self::Rows),
            "columns" => Some(Self::Columns),
            "structure" => Some(Self::Structure),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rows => "rows",
            Self::Columns => "columns",
            Self::Structure => "structure",
        }
    }
}

pub fn tab_command(
    page: usize,
    fields: &[(usize, FormField)],
    order: Order,
) -> Result<(Command, String), String> {
    let on_page: Vec<(Reference, [f64; 4])> = fields
        .iter()
        .filter(|(at, _)| *at == page)
        .map(|(_, field)| (field.widget, field.rect))
        .collect();
    if on_page.is_empty() {
        return Err(format!(
            "page {} has no form fields to put in order: document_info lists the fields and their pages",
            page + 1
        ));
    }
    let (widgets, how) = match order {
        Order::Rows => (in_reading_order(&on_page, false), TabOrder::AsListed),
        Order::Columns => (in_reading_order(&on_page, true), TabOrder::AsListed),
        Order::Structure => (
            on_page.iter().map(|(widget, _)| *widget).collect(),
            TabOrder::Structure,
        ),
    };
    let said = format!(
        "Page {} now tabs through its {} field{} in {} order, as one step undo takes back.",
        page + 1,
        widgets.len(),
        if widgets.len() == 1 { "" } else { "s" },
        order.as_str()
    );
    Ok((
        Command::SetTabOrder {
            page_index: page,
            widgets,
            order: how,
        },
        said,
    ))
}

#[cfg(test)]
mod tests;
