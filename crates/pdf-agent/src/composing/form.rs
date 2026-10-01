use std::collections::BTreeSet;

use pdf_edit::new_field::NewFieldKind;

use super::chart::mix;
use super::{Colour, Composer, Fitted, Style};
use crate::fielding;
use crate::json::Json;

const MOST_FIELDS: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Field {
    label: String,
    name: String,
    kind: NewFieldKind,
    required: bool,
    half: bool,
    lines: Option<usize>,
    options: Vec<String>,
}

fn kind_of(word: &str) -> Result<NewFieldKind, String> {
    match fielding::kind_of(&word.to_ascii_lowercase()) {
        Some(NewFieldKind::Button) | None => Err(format!(
            "\"{word}\" is not a kind of form field: text, paragraph, checkbox, radio, dropdown, list, date or signature"
        )),
        Some(kind) => Ok(kind),
    }
}

fn read_field(at: usize, json: &Json) -> Result<Field, String> {
    let which = at + 1;
    let text = |key: &str| {
        json.get(key)
            .and_then(Json::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let name = text("name");
    if name.is_empty() {
        return Err(format!("field {which} has no \"name\""));
    }
    let kind = match json.get("kind").or_else(|| json.get("type")) {
        None => NewFieldKind::Text,
        Some(Json::Text(word)) => kind_of(word.trim())?,
        Some(_) => return Err(format!("the \"kind\" of field {which} is not text")),
    };
    let label = text("label");
    if label.is_empty() && kind == NewFieldKind::Checkbox {
        return Err(format!("the checkbox {which} has no \"label\""));
    }
    let flag = |key: &str| json.get(key).and_then(Json::as_bool).unwrap_or(false);
    let lines = match json.get("lines") {
        None => None,
        Some(value) => Some(
            value
                .as_count()
                .filter(|lines| (1..=40).contains(lines))
                .ok_or_else(|| {
                    format!("\"lines\" of field {which} is a whole number from 1 to 40")
                })?,
        ),
    };
    let mut options = Vec::new();
    if let Some(list) = json.get("options") {
        let list = list
            .as_list()
            .ok_or_else(|| format!("\"options\" of field {which} is a list of text"))?;
        for option in list {
            match option.as_str().map(str::trim) {
                Some(option) if !option.is_empty() => options.push(option.to_owned()),
                _ => return Err(format!("an option of field {which} is not text")),
            }
        }
    }
    let wants = matches!(
        kind,
        NewFieldKind::Radio | NewFieldKind::Dropdown | NewFieldKind::ListBox
    );
    if wants && options.is_empty() {
        return Err(format!(
            "the {} \"{name}\" needs \"options\", the choices it offers",
            fielding::kind_name(kind)
        ));
    }
    if !wants && !options.is_empty() {
        return Err(format!(
            "the {} \"{name}\" takes no \"options\"",
            fielding::kind_name(kind)
        ));
    }
    Ok(Field {
        label,
        name,
        kind,
        required: flag("required"),
        half: flag("half"),
        lines,
        options,
    })
}

pub(crate) fn read(spec: &str) -> Result<Vec<Field>, String> {
    let json = Json::parse(spec.trim())
        .map_err(|why| format!("the form is not JSON: {} at {}", why.reason, why.at))?;
    let list = json
        .get("fields")
        .and_then(Json::as_list)
        .ok_or("the form has no \"fields\" list")?;
    if list.is_empty() {
        return Err("the form has no fields".to_owned());
    }
    if list.len() > MOST_FIELDS {
        return Err(format!(
            "a form block holds at most {MOST_FIELDS} fields: split it"
        ));
    }
    let mut seen = BTreeSet::new();
    let mut fields = Vec::with_capacity(list.len());
    for (at, json) in list.iter().enumerate() {
        let field = read_field(at, json)?;
        if !seen.insert(field.name.clone()) {
            return Err(format!(
                "two fields are named \"{}\": every field has its own name",
                field.name
            ));
        }
        fields.push(field);
    }
    Ok(fields)
}

enum Piece {
    Text {
        at: (f64, f64),
        width: f64,
        fitted: Fitted,
    },
    Widget {
        area: [f64; 4],
        kind: NewFieldKind,
        name: String,
        options: Vec<String>,
    },
}

struct Row {
    pieces: Vec<Piece>,
    height: f64,
}

struct Grid {
    body: f64,
    family: String,
    muted: Colour,
}

impl Grid {
    fn small(&self) -> f64 {
        (self.body * 0.85).max(6.5)
    }

    fn style(&self, size: f64) -> Style {
        Style {
            family: self.family.clone(),
            size,
            bold: false,
            italic: false,
            colour: Some(self.muted),
        }
    }

    fn gap(&self) -> f64 {
        self.body * 1.2
    }

    fn box_height(&self, field: &Field) -> f64 {
        let line = self.body * 1.4;
        #[expect(clippy::cast_precision_loss, reason = "a count of lines, at most 40")]
        match field.kind {
            NewFieldKind::Paragraph => field.lines.unwrap_or(4) as f64 * line + 8.0,
            NewFieldKind::ListBox => {
                field.lines.unwrap_or(field.options.len().clamp(2, 5)) as f64 * line + 6.0
            }
            NewFieldKind::Signature => self.body * 4.5,
            _ => (self.body * 2.0).max(20.0),
        }
    }
}

fn caption(field: &Field) -> String {
    if field.required {
        format!("{} *", field.label)
    } else {
        field.label.clone()
    }
}

fn lay_field(
    composer: &Composer<'_>,
    grid: &Grid,
    field: &Field,
    (x, width): (f64, f64),
) -> Result<Row, String> {
    let mut pieces = Vec::new();
    let small = grid.small();
    match field.kind {
        NewFieldKind::Checkbox => {
            let side = grid.body * 1.2;
            let beside = side + grid.body * 0.7;
            let fitted = composer.fit(
                &caption(field),
                &grid.style(grid.body * 0.95),
                width - beside,
            )?;
            let height = fitted.room.height.max(side);
            pieces.push(Piece::Widget {
                area: [x, 0.0, x + side, side],
                kind: field.kind,
                name: field.name.clone(),
                options: Vec::new(),
            });
            pieces.push(Piece::Text {
                at: (x + beside, (height - fitted.room.height) / 2.0),
                width: width - beside,
                fitted,
            });
            Ok(Row { pieces, height })
        }
        NewFieldKind::Radio => {
            let mut down = 0.0;
            if !field.label.is_empty() {
                let fitted = composer.fit(&caption(field), &grid.style(small), width)?;
                down = fitted.room.height + 4.0;
                pieces.push(Piece::Text {
                    at: (x, 0.0),
                    width,
                    fitted,
                });
            }
            let side = grid.body * 1.1;
            let line = side + 6.0;
            let (mut across, mut row_top) = (0.0, down);
            for option in &field.options {
                let fitted = composer.fit(option, &grid.style(grid.body * 0.95), width)?;
                let need = side + 6.0 + fitted.room.widest;
                if across > 0.0 && across + need > width {
                    across = 0.0;
                    row_top += line;
                }
                let edge = x + across;
                pieces.push(Piece::Widget {
                    area: [edge, row_top, edge + side, row_top + side],
                    kind: field.kind,
                    name: field.name.clone(),
                    options: Vec::new(),
                });
                let lift = (side - fitted.room.height) / 2.0;
                pieces.push(Piece::Text {
                    at: (edge + side + 6.0, row_top + lift),
                    width: fitted.room.widest,
                    fitted,
                });
                across += need + grid.body * 1.6;
            }
            Ok(Row {
                pieces,
                height: row_top + side,
            })
        }
        _ => {
            let mut down = 0.0;
            if !field.label.is_empty() {
                let fitted = composer.fit(&caption(field), &grid.style(small), width)?;
                down = fitted.room.height + 3.0;
                pieces.push(Piece::Text {
                    at: (x, 0.0),
                    width,
                    fitted,
                });
            }
            let high = grid.box_height(field);
            pieces.push(Piece::Widget {
                area: [x, down, x + width, down + high],
                kind: field.kind,
                name: field.name.clone(),
                options: field.options.clone(),
            });
            Ok(Row {
                pieces,
                height: down + high,
            })
        }
    }
}

fn lay_out(
    composer: &Composer<'_>,
    fields: &[Field],
    indent: f64,
) -> Result<(Vec<Row>, f64), String> {
    let theme = composer.theme();
    let background = theme.paper.unwrap_or([1.0, 1.0, 1.0]);
    let grid = Grid {
        body: composer.setting.body,
        family: composer.setting.family.to_owned(),
        muted: mix(theme.ink, background, 0.35),
    };
    let left = composer.left() + indent;
    let wide = composer.right() - left;
    let between = grid.body * 1.3;
    let half = (wide - between) / 2.0;
    let mut rows = Vec::new();
    let mut at = 0;
    while at < fields.len() {
        let field = &fields[at];
        let next = fields.get(at + 1).filter(|next| field.half && next.half);
        let mut row = match next {
            Some(next) => {
                let mut one = lay_field(composer, &grid, field, (left, half))?;
                let other = lay_field(composer, &grid, next, (left + half + between, half))?;
                one.pieces.extend(other.pieces);
                one.height = one.height.max(other.height);
                one
            }
            None if field.half => lay_field(composer, &grid, field, (left, half))?,
            None => lay_field(composer, &grid, field, (left, wide))?,
        };
        row.height = row.height.max(1.0);
        rows.push(row);
        at += if next.is_some() { 2 } else { 1 };
    }
    Ok((rows, grid.gap()))
}

fn total(rows: &[Row], gap: f64) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    rows.iter().map(|row| row.height + gap).sum::<f64>() - gap
}

pub(crate) fn keep_height(composer: &Composer<'_>, spec: &str, indent: f64) -> f64 {
    let Ok(fields) = read(spec) else {
        return 0.0;
    };
    let Ok((rows, gap)) = lay_out(composer, &fields, indent) else {
        return 0.0;
    };
    if total(&rows, gap) <= composer.usable() / 2.0 {
        total(&rows, gap)
    } else {
        rows.first().map_or(0.0, |row| row.height)
    }
}

pub(crate) fn set_out(
    composer: &mut Composer<'_>,
    spec: &str,
    space_before: f64,
    indent: f64,
) -> Result<(), String> {
    let fields = read(spec)?;
    for field in &fields {
        if !composer.names.insert(field.name.clone()) {
            return Err(format!(
                "the field name \"{}\" is used twice in this document: every field has its own name",
                field.name
            ));
        }
    }
    let (rows, gap) = lay_out(composer, &fields, indent)?;
    let whole = total(&rows, gap);
    let together = whole <= composer.usable() / 2.0;
    let mut top = composer.place(
        space_before,
        if together {
            whole
        } else {
            rows.first().map_or(0.0, |row| row.height)
        },
    );
    for (at, row) in rows.into_iter().enumerate() {
        if at > 0 {
            top = if together {
                composer.top + gap
            } else {
                composer.place(gap, row.height)
            };
        }
        for piece in row.pieces {
            match piece {
                Piece::Text {
                    at: (x, down),
                    width,
                    fitted,
                } => composer.text((x, top + down), width, fitted),
                Piece::Widget {
                    area: [left, up, right, down],
                    kind,
                    name,
                    options,
                } => composer.field([left, top + up, right, top + down], kind, name, options)?,
            }
        }
        composer.top = top + row.height;
    }
    Ok(())
}
