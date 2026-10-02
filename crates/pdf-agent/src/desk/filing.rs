use std::path::Path;
use std::sync::Arc;

use pdf_convert::run::Input;

use crate::converting::{self, Asked, Source};
use crate::taking::{self, Pictures, Split};

use super::{Desk, Open, Refused, page_png, read_a_file};

fn folder_of(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn made(
    open: &Open,
    outcome: &pdf_convert::run::Outcome,
) -> Result<pdf_convert::run::Saved, Refused> {
    if outcome.made_nothing() {
        return Err("nothing was made".to_owned());
    }
    let name = converting::name_of(&open.path);
    converting::put_beside(None, outcome, &[name.as_str()], folder_of(&open.path))
}

impl Desk {
    pub fn convert(&mut self, handle: &str, asked: &Asked) -> Result<String, Refused> {
        let sources = asked.sources()?;
        let fonts = self.fonts.clone();
        let open = self.get(handle)?;
        let credential = open.session.credential().to_vec();
        let document = converting::name_of(&open.path);
        let mut values = asked.values_with_pictures()?;
        let mut inputs = Vec::with_capacity(sources.len());
        let mut base = None;
        for (at, source) in sources.iter().enumerate() {
            match source {
                Source::Document => {
                    values.give_the_credential(asked.tool, at, &credential);
                    inputs.push(Input::new(document.clone(), open.session.source().to_vec()));
                    base.get_or_insert_with(|| folder_of(&open.path).to_path_buf());
                }
                Source::File(path) => {
                    inputs.push(Input::new(converting::name_of(path), read_a_file(path)?));
                    base.get_or_insert_with(|| folder_of(path).to_path_buf());
                }
            }
        }
        let names: Vec<String> = inputs.iter().map(|input| input.name.clone()).collect();
        let outcome = converting::run_to_the_end(asked.tool, inputs, values, fonts)
            .map_err(|failure| converting::why(&failure))?;
        if outcome.made_nothing() {
            return Err(format!(
                "The converter made no file: {}",
                if outcome.notes.is_empty() {
                    "it gave no reason".to_owned()
                } else {
                    outcome.notes.join("; ")
                }
            ));
        }
        let inputs_named: Vec<&str> = names.iter().map(String::as_str).collect();
        let saved = converting::put_beside(
            Some(asked.tool),
            &outcome,
            &inputs_named,
            &base.unwrap_or_else(|| Path::new(".").to_path_buf()),
        )?;
        let mut said = converting::said_made(&saved, &outcome);
        if asked.open_result {
            said.push_str(" It was not opened: there is no window here to open it in.");
        }
        Ok(said)
    }

    pub fn extract(&mut self, handle: &str, spec: &str) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        let pages = taking::pages_named(spec, count)?;
        let credential = open.session.credential().to_vec();
        let bytes: Arc<[u8]> = Arc::from(open.session.source().to_vec());
        let outcome = taking::extract_outcome(&open.path, &bytes, &credential, &pages)?;
        let saved = made(open, &outcome)?;
        Ok(taking::said_extracted(
            &pages,
            &converting::said_made(&saved, &outcome),
        ))
    }

    pub fn split(&mut self, handle: &str, split: &Split) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        let groups = split.groups(count)?;
        let credential = open.session.credential().to_vec();
        let bytes: Arc<[u8]> = Arc::from(open.session.source().to_vec());
        let outcome = taking::split_outcome(&open.path, (&bytes, &credential), &groups)?;
        let saved = made(open, &outcome)?;
        Ok(taking::said_split(
            &groups,
            &converting::said_made(&saved, &outcome),
        ))
    }

    pub fn page_pictures(&mut self, handle: &str, asked: &Pictures) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        let pages = taking::pages_named(&asked.pages, count)?;
        if pages.len() > taking::MOST_FILES {
            return Err(format!(
                "that is {} pictures: the most one call writes is {}",
                pages.len(),
                taking::MOST_FILES
            ));
        }
        let names = taking::picture_names(&open.path, &pages);
        let mut files = Vec::with_capacity(pages.len());
        for (page, name) in pages.iter().zip(names) {
            let view = open
                .session
                .page_for_display(*page)
                .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
            let picture = page_png(&view, asked.dpi)
                .map_err(|why| format!("page {} cannot be drawn: {why}", page + 1))?;
            files.push((name, picture));
        }
        let outcome = pdf_convert::run::Outcome {
            files,
            notes: Vec::new(),
        };
        let saved = made(open, &outcome)?;
        Ok(taking::said_pictures(
            &pages,
            asked.dpi,
            &converting::said_made(&saved, &outcome),
        ))
    }
}
