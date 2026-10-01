use std::io;
use std::path::{Component, Path, PathBuf};

use crate::catalogue::Tool;
use crate::catalogue::placing::{named_after, placed, unused};
use crate::run::Outcome;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Saved {
    pub folder: PathBuf,
    pub files: Vec<PathBuf>,
    pub tucked_in: bool,
}

pub type Writer<'a> = &'a mut dyn FnMut(&Path, &[u8]) -> io::Result<()>;

#[must_use]
pub fn is_inside(name: &str) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && !name.contains('\\')
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub fn save_beside(
    tool: Option<Tool>,
    outcome: &Outcome,
    inputs: &[&str],
    (base, fallback): (&Path, Option<&Path>),
    write: Writer<'_>,
) -> Result<Saved, String> {
    let renamed: Vec<String> = outcome
        .files
        .iter()
        .map(|(name, _)| tool.map_or_else(|| name.clone(), |tool| named_after(tool, name, inputs)))
        .collect();
    if let Some(bad) = renamed.iter().find(|name| !is_inside(name)) {
        return Err(format!("{bad} is not a name this can write"));
    }
    let layout = placed(&renamed, inputs);
    let mut tried = vec![base.to_path_buf()];
    if let Some(fallback) = fallback
        && fallback != base
    {
        tried.push(fallback.to_path_buf());
    }
    let mut last = String::new();
    for folder in tried {
        match write_all(
            &folder,
            layout.folder.as_deref(),
            (&renamed, outcome),
            &mut *write,
        ) {
            Ok(saved) => return Ok(saved),
            Err(why) => last = why,
        }
    }
    Err(last)
}

fn write_all(
    folder: &Path,
    tucked_in: Option<&str>,
    (names, outcome): (&[String], &Outcome),
    write: Writer<'_>,
) -> Result<Saved, String> {
    let there = |name: &str| std::fs::symlink_metadata(folder.join(name)).is_ok();
    let target = match tucked_in {
        Some(name) => {
            let made = folder.join(unused(name, &there, false));
            std::fs::create_dir(&made).map_err(|error| format!("{}: {error}", made.display()))?;
            made
        }
        None => folder.to_path_buf(),
    };
    let mut files = Vec::with_capacity(names.len());
    for (name, (_, bytes)) in names.iter().zip(&outcome.files) {
        let inside = |wanted: &str| std::fs::symlink_metadata(target.join(wanted)).is_ok();
        let wanted = if tucked_in.is_some() {
            name.clone()
        } else {
            unused(name, &inside, true)
        };
        let path = target.join(&wanted);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("{}: {error}", parent.display()))?;
        }
        write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        files.push(path);
    }
    Ok(Saved {
        folder: target,
        files,
        tucked_in: tucked_in.is_some(),
    })
}

pub fn write_new_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = std::fs::remove_file(path);
    }
    written
}
