use crate::catalogue::tool::Tool;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placed {
    pub folder: Option<String>,
    pub names: Vec<String>,
}

#[must_use]
pub fn stem_of(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ if base.is_empty() => "result".to_owned(),
        _ => base.to_owned(),
    }
}

#[must_use]
pub fn named_after(tool: Tool, made: &str, inputs: &[&str]) -> String {
    if tool == Tool::Compare
        && let Some((_, extension)) = made.rsplit_once('.')
        && let Some(first) = inputs.first()
    {
        return format!("{}-comparison.{extension}", stem_of(first));
    }
    made.to_owned()
}

#[must_use]
pub fn placed(made: &[String], inputs: &[&str]) -> Placed {
    let alone = made.len() == 1 && made.iter().all(|name| !name.contains('/'));
    if alone {
        return Placed {
            folder: None,
            names: made.to_vec(),
        };
    }
    let first = inputs
        .first()
        .map_or_else(|| "result".to_owned(), |name| stem_of(name));
    let folder = match inputs.len() {
        0 | 1 => first,
        many => format!("{first}-and-{}-more", many - 1),
    };
    Placed {
        folder: Some(folder),
        names: made.to_vec(),
    }
}

#[must_use]
pub fn unused(name: &str, taken: &dyn Fn(&str) -> bool, with_ending: bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ending) = match name.rsplit_once('.') {
        Some((stem, ending)) if with_ending && !stem.is_empty() => (stem, format!(".{ending}")),
        _ => (name, String::new()),
    };
    (2_u32..=u32::MAX)
        .map(|number| format!("{stem}-{number}{ending}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}
