use crate::fielding::{self, Order};
use crate::linking::{self, Action, Place};
use crate::shaping;

use super::{Desk, Refused, apply, device, device_of, engine_words, page_size_of, read, view_of};

impl Desk {
    pub fn links(&mut self, handle: &str, action: &Action) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        let no_page =
            |page: usize| format!("there is no page {}: the document has {count}", page + 1);
        let source = open.session.source().clone();
        let credential = open.session.credential().to_vec();
        let geometries = open
            .session
            .page_geometries()
            .map_err(|error| format!("the pages cannot be measured: {error}"))?;
        match action {
            Action::List { page } => {
                let geometry = geometries.get(*page).ok_or_else(|| no_page(*page))?;
                let found =
                    pdf_edit::link::links_of(&source, &credential, *page).map_err(|error| {
                        format!("the links cannot be read: {}", engine_words(&error))
                    })?;
                let listing = linking::listing(*page, &found, &device_of(geometry)?);
                open.links_listed.retain(|(held, _), _| held != page);
                for (index, anchor) in listing.anchors.into_iter().enumerate() {
                    open.links_listed.insert((*page, index), anchor);
                }
                Ok(listing.text)
            }
            Action::Add { page, place, goes } => {
                let geometry = geometries.get(*page).ok_or_else(|| no_page(*page))?;
                let area = match place {
                    Place::Area(area) => *area,
                    Place::Block(name) => {
                        let (block_page, index, view) = Self::resolve(open, name)?;
                        if block_page != *page {
                            return Err(format!("{name} is not on page {}", page + 1));
                        }
                        read(&view, block_page, index)
                            .ok_or_else(|| format!("{name} cannot be read"))?
                            .0
                            .area
                    }
                };
                let command =
                    linking::add_command(*page, &device_of(geometry)?.matrix, (area, goes), count)?;
                apply(open, &command).map_err(|error| engine_words(&error))?;
                open.links_listed.retain(|(held, _), _| held != page);
                Ok(linking::said_added(*page, area, goes))
            }
            Action::Remove { link } => {
                let (page, index) = linking::parse_name(link)?;
                if page >= count {
                    return Err(no_page(page));
                }
                let found =
                    pdf_edit::link::links_of(&source, &credential, page).map_err(|error| {
                        format!("the links cannot be read: {}", engine_words(&error))
                    })?;
                let recorded = open.links_listed.get(&(page, index)).cloned();
                let reference = linking::the_link(&found, index, recorded.as_deref(), link)?;
                apply(open, &linking::remove_command(page, reference))
                    .map_err(|error| engine_words(&error))?;
                open.links_listed.retain(|(held, _), _| *held != page);
                Ok(linking::said_removed(link))
            }
        }
    }

    pub fn draw_shape(&mut self, handle: &str, asked: &shaping::Asked) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, asked.page)?;
        let command = shaping::command(asked, &device(&view)?.matrix, page_size_of(&view)?)?;
        apply(open, &command).map_err(|error| engine_words(&error))?;
        Ok(shaping::said(asked))
    }

    pub fn add_field(&mut self, handle: &str, asked: &fielding::Asked) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, asked.page)?;
        let command = fielding::add_command(asked, &device(&view)?.matrix)?;
        apply(open, &command).map_err(|error| engine_words(&error))?;
        Ok(fielding::said_added(asked))
    }

    pub fn tab_order(
        &mut self,
        handle: &str,
        page: usize,
        order: Order,
    ) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        if page >= count {
            return Err(format!(
                "there is no page {}: the document has {count}",
                page + 1
            ));
        }
        let fields =
            pdf_edit::form::fields_of_document(open.session.source(), open.session.credential())
                .map_err(|error| {
                    format!("the form's fields cannot be read: {}", engine_words(&error))
                })?;
        let (command, said) = fielding::tab_command(page, &fields, order)?;
        apply(open, &command).map_err(|error| engine_words(&error))?;
        Ok(said)
    }
}
