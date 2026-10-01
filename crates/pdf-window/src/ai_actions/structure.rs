use std::sync::Arc;

use pdf_agent::connect::ToolCall;
use pdf_agent::desk::{self, engine_words};
use pdf_agent::fielding::{self, Order};
use pdf_agent::linking::{self, Action, Goes, Place};
use pdf_agent::shaping;
use pdf_agent::tools::request::Request;
use pdf_app::wording::Done;
use pdf_edit::Command;
use pdf_edit::link::Listed;

use super::editing::{failed, no_page, said};
use super::{Performed, parse_block_name};
use crate::window_state::Window;

impl Window {
    pub(super) fn perform_structure(
        &mut self,
        call: &ToolCall,
        request: &Request,
        pages: usize,
    ) -> Performed {
        match request {
            Request::Links(action) => self.links(call, request.clone(), action, pages),
            Request::DrawShape(asked) => self.draw_shape(call, request.clone(), asked, pages),
            Request::AddField(asked) => self.add_field(call, request.clone(), asked, pages),
            Request::SetTabOrder { page, order } => {
                self.tab_order(call, request.clone(), (*page, *order), pages)
            }
            _ => failed(
                call,
                "that is not one of the tools for links, shapes and fields",
            ),
        }
    }

    fn device_of_page(&self, page: usize) -> Result<pdf_render::DeviceTransform, String> {
        let geometry = self
            .editor
            .geometry(page)
            .ok_or_else(|| format!("page {} has no size this can write on", page + 1))?;
        desk::device_of(geometry)
    }

    fn links(
        &mut self,
        call: &ToolCall,
        request: Request,
        action: &Action,
        pages: usize,
    ) -> Performed {
        let page = match action {
            Action::List { page } | Action::Add { page, .. } => *page,
            Action::Remove { link } => match linking::parse_name(link) {
                Ok((page, _)) => page,
                Err(why) => return failed(call, why),
            },
        };
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        match action {
            Action::List { .. } => self.list_links(call, page),
            Action::Add { place, goes, .. } => {
                self.add_a_link(call, request, page, (place, goes), pages)
            }
            Action::Remove { link } => self.remove_a_link(call, request, page, link),
        }
    }

    fn the_links_of(&mut self, call: &ToolCall, page: usize) -> Result<Vec<Listed>, Performed> {
        let Some(source) = self.editor.source().cloned() else {
            return Err(self.no_source(call));
        };
        pdf_edit::link::links_of(&source, self.editor.credential(), page).map_err(|error| {
            failed(
                call,
                format!("the links cannot be read: {}", engine_words(&error)),
            )
        })
    }

    fn list_links(&mut self, call: &ToolCall, page: usize) -> Performed {
        let device = match self.device_of_page(page) {
            Ok(device) => device,
            Err(why) => return failed(call, why),
        };
        let found = match self.the_links_of(call, page) {
            Ok(found) => found,
            Err(performed) => return performed,
        };
        let listing = linking::listing(page, &found, &device);
        self.ai
            .tools
            .listed_links
            .retain(|(held, _), _| *held != page);
        for (index, anchor) in listing.anchors.into_iter().enumerate() {
            self.ai.tools.listed_links.insert((page, index), anchor);
        }
        said(call, listing.text)
    }

    fn add_a_link(
        &mut self,
        call: &ToolCall,
        request: Request,
        page: usize,
        (place, goes): (&Place, &Goes),
        pages: usize,
    ) -> Performed {
        let area = match place {
            Place::Area(area) => *area,
            Place::Block(name) => {
                if let Ok(key) = parse_block_name(name)
                    && key.0 < pages
                    && self.editor.leaf(key.0).is_none()
                {
                    return Performed::NeedPages(vec![key.0]);
                }
                let (block_page, index) = match self.block_named(name) {
                    Ok(found) => found,
                    Err(why) => return failed(call, why),
                };
                if block_page != page {
                    return failed(call, format!("{name} is not on page {}", page + 1));
                }
                match self.ai.tools.named.get(&(block_page, index)) {
                    Some(named) => named.area,
                    None => return failed(call, format!("{name} has not been read yet")),
                }
            }
        };
        let device = match self.device_of_page(page) {
            Ok(device) => device,
            Err(why) => return failed(call, why),
        };
        let command = match linking::add_command(page, &device.matrix, (area, goes), pages) {
            Ok(command) => command,
            Err(why) => return failed(call, why),
        };
        let text = linking::said_added(page, area, goes);
        let job = self.editor.begin_command(command, Done::AddedLink);
        let sent = self.send_for(call, request, job, None);
        self.ai
            .tools
            .listed_links
            .retain(|(held, _), _| *held != page);
        self.sent_changing(vec![page], Some(text));
        sent
    }

    fn remove_a_link(
        &mut self,
        call: &ToolCall,
        request: Request,
        page: usize,
        link: &str,
    ) -> Performed {
        let Ok((_, index)) = linking::parse_name(link) else {
            return failed(call, format!("{link} is not a link name"));
        };
        let found = match self.the_links_of(call, page) {
            Ok(found) => found,
            Err(performed) => return performed,
        };
        let recorded = self.ai.tools.listed_links.get(&(page, index)).cloned();
        let reference = match linking::the_link(&found, index, recorded.as_deref(), link) {
            Ok(reference) => reference,
            Err(why) => return failed(call, why),
        };
        let job = self
            .editor
            .begin_command(linking::remove_command(page, reference), Done::RemovedLink);
        let sent = self.send_for(call, request, job, None);
        self.ai
            .tools
            .listed_links
            .retain(|(held, _), _| *held != page);
        self.sent_changing(vec![page], Some(linking::said_removed(link)));
        sent
    }

    fn draw_shape(
        &mut self,
        call: &ToolCall,
        request: Request,
        asked: &shaping::Asked,
        pages: usize,
    ) -> Performed {
        if asked.page >= pages {
            return failed(call, no_page(asked.page, pages));
        }
        let device = match self.device_of_page(asked.page) {
            Ok(device) => device,
            Err(why) => return failed(call, why),
        };
        let size = [f64::from(device.width), f64::from(device.height)];
        let command = match shaping::command(asked, &device.matrix, size) {
            Ok(command) => command,
            Err(why) => return failed(call, why),
        };
        let job = self.editor.begin_command(command, Done::DrewShape);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![asked.page], Some(shaping::said(asked)));
        sent
    }

    fn add_field(
        &mut self,
        call: &ToolCall,
        request: Request,
        asked: &fielding::Asked,
        pages: usize,
    ) -> Performed {
        if asked.page >= pages {
            return failed(call, no_page(asked.page, pages));
        }
        let device = match self.device_of_page(asked.page) {
            Ok(device) => device,
            Err(why) => return failed(call, why),
        };
        let command = match fielding::add_command(asked, &device.matrix) {
            Ok(command) => command,
            Err(why) => return failed(call, why),
        };
        let job = self.editor.begin_command(command, Done::AddedField);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![asked.page], Some(fielding::said_added(asked)));
        sent
    }

    fn tab_order(
        &mut self,
        call: &ToolCall,
        request: Request,
        (page, order): (usize, Order),
        pages: usize,
    ) -> Performed {
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        let fields = Arc::clone(&self.editor.fields_in_document());
        let (command, text): (Command, String) = match fielding::tab_command(page, &fields, order) {
            Ok(made) => made,
            Err(why) => return failed(call, why),
        };
        let job = self.editor.begin_command(command, Done::OrderedFields);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![page], Some(text));
        sent
    }
}

#[cfg(test)]
mod tests;
