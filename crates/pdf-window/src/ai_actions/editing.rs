use std::sync::Arc;

use pdf_agent::connect::{AttachmentKind, Picture, ToolCall, ToolResult};
use pdf_agent::desk::{self, Block};
use pdf_agent::finding::{MOST_BLOCKS, PageWork, Replaced, Search, said_marked, said_replaced};
use pdf_agent::marking::{self, MOST_MARKS, Marking};
use pdf_agent::objects::{self, Action as ObjectAction, Name};
use pdf_agent::outlining::{self, Action as BookmarkAction, Place};
use pdf_agent::pictures::{self, Source};
use pdf_agent::stamping;
use pdf_agent::styling::Look;
use pdf_agent::tools::{self, request::Request};
use pdf_app::wording::Done;

use super::{Performed, READ_AHEAD, parse_block_name};
use crate::window_state::Window;

pub(super) struct Sweeping {
    call: String,
    span: (usize, usize),
    next: usize,
    works: Vec<PageWork>,
    taken: usize,
}

pub(super) struct Listed {
    pub(super) arranged: u64,
    pub(super) anchor: String,
}

enum Sweep<'a> {
    Replace(&'a Search, &'a str),
    Mark(&'a Search, Marking),
}

pub(super) fn failed(call: &ToolCall, why: impl Into<String>) -> Performed {
    Performed::Done(ToolResult::failed(&call.id, why))
}

pub(super) fn said(call: &ToolCall, text: impl Into<String>) -> Performed {
    Performed::Done(ToolResult::said(&call.id, text))
}

pub(super) fn no_page(page: usize, pages: usize) -> String {
    format!("there is no page {}: the document has {pages}", page + 1)
}

impl Window {
    pub(super) fn perform_editing(
        &mut self,
        call: &ToolCall,
        request: &Request,
        pages: usize,
    ) -> Performed {
        match request {
            Request::FindAndReplace {
                search,
                with,
                first,
                last,
            } => self.find_and_replace(
                call,
                request.clone(),
                (search, with),
                (*first, *last),
                pages,
            ),
            Request::StyleText { block, find, look } => {
                self.style_text(call, request.clone(), (block, find.as_deref(), look))
            }
            Request::MarkText {
                search,
                marking,
                first,
                last,
            } => self.mark_text(
                call,
                request.clone(),
                (search, *marking),
                (*first, *last),
                pages,
            ),
            Request::AddStamp(asked) => self.add_stamp(call, request.clone(), asked, pages),
            Request::Bookmarks(action) => self.bookmarks(call, request.clone(), action, pages),
            Request::PlacePicture(asked) => self.place_picture(call, request.clone(), asked, pages),
            Request::Objects(action) => self.objects(call, request.clone(), action, pages),
            Request::GoToPage { page } => self.go_to_page(call, *page, pages),
            Request::LookCloser { page, region, dpi } => {
                self.look_closer(call, (*page, *region, *dpi), pages)
            }
            _ => failed(call, "that is not one of the tools for editing pages"),
        }
    }

    fn sweep_the_pages(
        &mut self,
        call: &ToolCall,
        span: (usize, usize),
        kind: &Sweep<'_>,
    ) -> Result<Vec<PageWork>, Performed> {
        let mut sweeping = match self.ai.tools.sweeping.take() {
            Some(held) if held.call == call.id && held.span == span => held,
            _ => Sweeping {
                call: call.id.clone(),
                span,
                next: span.0,
                works: Vec::new(),
                taken: 0,
            },
        };
        let most = match kind {
            Sweep::Replace(..) => MOST_BLOCKS,
            Sweep::Mark(..) => MOST_MARKS,
        };
        while sweeping.next <= span.1 && sweeping.taken < most {
            let page = sweeping.next;
            if let Some(why) = self.failed.get(&page) {
                return Err(failed(
                    call,
                    format!("page {} cannot be read: {why}", page + 1),
                ));
            }
            let Some(leaf) = self.editor.leaf(page).map(Arc::clone) else {
                self.ai.tools.sweeping = Some(sweeping);
                let wanted: Vec<usize> = (page..=span.1.min(page + READ_AHEAD - 1))
                    .filter(|at| self.editor.leaf(*at).is_none() && !self.failed.contains_key(at))
                    .collect();
                return Err(Performed::NeedPages(wanted));
            };
            let work = match kind {
                Sweep::Replace(search, with) => {
                    if self.editor.is_busy() {
                        self.ai.tools.sweeping = Some(sweeping);
                        return Err(Performed::Busy);
                    }
                    pdf_agent::finding::replacements_on(
                        &leaf.view,
                        page,
                        (search, with),
                        &mut |command| {
                            self.editor
                                .try_planning(command)
                                .unwrap_or_else(|| Err("the document is busy".to_owned()))
                        },
                    )
                }
                Sweep::Mark(search, marking) => {
                    marking::marks_on(&leaf.view, page, &leaf.overlay.clusters, (search, *marking))
                }
            };
            sweeping.taken += work.commands.len();
            sweeping.works.push(work);
            sweeping.next += 1;
        }
        Ok(sweeping.works)
    }

    pub(super) fn find_and_replace(
        &mut self,
        call: &ToolCall,
        request: Request,
        (search, with): (&Search, &str),
        (first, last): (Option<usize>, Option<usize>),
        pages: usize,
    ) -> Performed {
        let span = match tools::page_span(first, last, pages) {
            Ok(span) => span,
            Err(why) => return failed(call, why),
        };
        let works = match self.sweep_the_pages(call, span, &Sweep::Replace(search, with)) {
            Ok(works) => works,
            Err(waiting) => return waiting,
        };
        let replaced = Replaced::of(works);
        let text = said_replaced((search, with), &replaced, (span, pages));
        if replaced.commands.is_empty() {
            return said(call, text);
        }
        let changed: Vec<usize> = replaced.per_page.iter().map(|(page, _)| *page).collect();
        let done = Done::ReplacedText {
            count: replaced.total(),
            pages: replaced.per_page.len(),
        };
        let job = self.editor.begin_commands(replaced.commands, done);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(changed, Some(text));
        sent
    }

    pub(super) fn mark_text(
        &mut self,
        call: &ToolCall,
        request: Request,
        (search, marking): (&Search, Marking),
        (first, last): (Option<usize>, Option<usize>),
        pages: usize,
    ) -> Performed {
        let span = match tools::page_span(first, last, pages) {
            Ok(span) => span,
            Err(why) => return failed(call, why),
        };
        let works = match self.sweep_the_pages(call, span, &Sweep::Mark(search, marking)) {
            Ok(works) => works,
            Err(waiting) => return waiting,
        };
        let marked = Replaced::of(works);
        let text = said_marked((search, marking.how.done()), &marked, (span, pages));
        if marked.commands.is_empty() {
            return said(call, text);
        }
        let changed: Vec<usize> = marked.per_page.iter().map(|(page, _)| *page).collect();
        let done = Done::MarkedText {
            count: marked.total(),
        };
        let job = self.editor.begin_commands(marked.commands, done);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(changed, Some(text));
        sent
    }

    pub(super) fn style_text(
        &mut self,
        call: &ToolCall,
        request: Request,
        (name, find, look): (&str, Option<&str>, &Look),
    ) -> Performed {
        let key = match parse_block_name(name) {
            Ok(key) => key,
            Err(why) => return failed(call, why),
        };
        if key.0 < self.editor.page_count() && self.editor.leaf(key.0).is_none() {
            return Performed::NeedPages(vec![key.0]);
        }
        let (page, block) = match self.block_named(name) {
            Ok(found) => found,
            Err(why) => return failed(call, why),
        };
        let Some(reading) = self.editor.block_reading(page, block) else {
            return failed(call, format!("{name} cannot be read as text"));
        };
        let range = match find {
            None => self.editor.whole_block(page, block),
            Some(find) => match desk::range_within(&reading, find) {
                Ok(range) => Some(range),
                Err(problem) => return failed(call, format!("{name}: {problem}")),
            },
        };
        let Some(range) = range else {
            return failed(call, format!("{name} cannot be read as text"));
        };
        let size = reading.lines.first().map_or(12.0, |line| line.em);
        let style = look.text_style(size);
        if let Some(align) = look.align {
            self.editor.set_alignment(page, block, align.alignment());
        }
        let job = self.editor.begin_style(page, block, range, style);
        let text = format!(
            "Styled {name}{}: {}.",
            find.map_or_else(String::new, |find| format!(
                " (the words \u{201c}{find}\u{201d})"
            )),
            look.words()
        );
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![page], Some(text));
        sent
    }

    pub(super) fn add_stamp(
        &mut self,
        call: &ToolCall,
        request: Request,
        asked: &stamping::Asked,
        pages: usize,
    ) -> Performed {
        let prepared = match asked.prepare(pages) {
            Ok(prepared) => prepared,
            Err(why) => {
                return failed(
                    call,
                    why.trim_start_matches("cannot type here: ").to_owned(),
                );
            }
        };
        let name = self
            .opened
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let today = stamping::today();
        let text = format!(
            "{}, as one step undo takes back.",
            prepared.said((pages, &name, &today))
        );
        let changed = prepared.pages.clone();
        let job = self.editor.begin_stamp(
            prepared.pages,
            prepared.stamp,
            (prepared.start, name, today),
        );
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(changed, Some(text));
        sent
    }

    pub(super) fn bookmarks(
        &mut self,
        call: &ToolCall,
        request: Request,
        action: &BookmarkAction,
        pages: usize,
    ) -> Performed {
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        let credential = self.editor.credential().to_vec();
        let before = match pdf_edit::outline::read_outline(&source, &credential) {
            Ok(before) => before,
            Err(error) => {
                return failed(call, format!("the bookmarks cannot be read: {error}"));
            }
        };
        match action {
            BookmarkAction::List => said(call, outlining::listing(&before)),
            BookmarkAction::FromHeadings { replace } => {
                self.contents_from_headings(call, request, *replace, (&before, pages))
            }
            other => {
                let block = match other {
                    BookmarkAction::Add {
                        place: Place::Block(name),
                        ..
                    } => {
                        if let Ok(key) = parse_block_name(name)
                            && key.0 < pages
                            && self.editor.leaf(key.0).is_none()
                        {
                            return Performed::NeedPages(vec![key.0]);
                        }
                        let (page, index) = match self.block_named(name) {
                            Ok(found) => found,
                            Err(why) => return failed(call, why),
                        };
                        let text = self
                            .ai
                            .tools
                            .named
                            .get(&(page, index))
                            .map(|named| named.text.clone())
                            .unwrap_or_default();
                        Some((page, text))
                    }
                    _ => None,
                };
                let change = match outlining::change_for(&before, other, (block.as_ref(), pages)) {
                    Ok(change) => change,
                    Err(why) => return failed(call, why),
                };
                let page_index = match &change {
                    pdf_edit::outline::Change::Add { page, .. }
                    | pdf_edit::outline::Change::Retarget { page, .. } => *page,
                    _ => 0,
                };
                let text = outlining::said_change(&change, &before);
                let job = self.editor.begin_command(
                    pdf_edit::Command::ChangeOutline { page_index, change },
                    Done::ChangedBookmarks,
                );
                let sent = self.send_for(call, request, job, None);
                self.sent_changing(Vec::new(), Some(text));
                sent
            }
        }
    }

    fn contents_from_headings(
        &mut self,
        call: &ToolCall,
        request: Request,
        replace: bool,
        (before, pages): (&[pdf_edit::outline::Bookmark], usize),
    ) -> Performed {
        if !replace && !before.is_empty() {
            return failed(
                call,
                format!(
                    "the document already has {} bookmark{}: pass replace: true to take them out \
                     and make new ones from the headings, or add the missing ones one by one",
                    before.len(),
                    if before.len() == 1 { "" } else { "s" }
                ),
            );
        }
        let gathered = match self.gather_pages(call, (0, pages.saturating_sub(1)), usize::MAX) {
            Ok(gathered) => gathered,
            Err(waiting) => return waiting,
        };
        let headings = outlining::headings_in(&gathered);
        let Some(first_heading) = headings.first() else {
            return failed(
                call,
                "no heading was found: the text is all of one size, or none is set larger than \
                 the rest",
            );
        };
        let Some(source) = self.editor.source().cloned() else {
            return self.no_source(call);
        };
        let first = match outlining::first_new_object(
            &source,
            self.editor.credential(),
            self.editor.fonts(),
            first_heading.page,
        ) {
            Ok(first) => first,
            Err(why) => return failed(call, why),
        };
        let removing: &[pdf_edit::outline::Bookmark] = if replace { before } else { &[] };
        let taken_out = removing.iter().filter(|book| book.depth == 0).count();
        let commands = outlining::commands_for_headings(&headings, first, removing);
        let text = outlining::said_headings(&headings, taken_out);
        let job = self.editor.begin_commands(commands, Done::ChangedBookmarks);
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(Vec::new(), Some(text));
        sent
    }

    pub(super) fn the_picture_to_place(&self, wanted: Option<usize>) -> Result<Arc<[u8]>, String> {
        let pictures: Vec<&pdf_agent::connect::Attachment> = self
            .ai
            .pictures_attached()
            .filter(|attachment| matches!(attachment.kind, AttachmentKind::Image { .. }))
            .collect();
        let found = match wanted {
            None => pictures.last(),
            Some(number) => number
                .checked_sub(1)
                .and_then(|at| pictures.get(at))
                .or(None),
        };
        match (found, wanted) {
            (Some(attachment), _) => Ok(Arc::from(attachment.bytes.clone())),
            (None, None) => Err(
                "no picture is attached to this chat: the person can attach one with the paper \
                 clip, or give you the path of a file"
                    .to_owned(),
            ),
            (None, Some(number)) => Err(format!(
                "there is no attached picture {number}: {} picture{} {} attached to this chat",
                pictures.len(),
                if pictures.len() == 1 { "" } else { "s" },
                if pictures.len() == 1 { "is" } else { "are" }
            )),
        }
    }

    pub(super) fn place_picture(
        &mut self,
        call: &ToolCall,
        request: Request,
        asked: &pictures::Asked,
        pages: usize,
    ) -> Performed {
        if asked.page >= pages {
            return failed(call, no_page(asked.page, pages));
        }
        let file = match &asked.source {
            Source::File(path) => pictures::read_file(path),
            Source::Attachment(number) => self.the_picture_to_place(*number),
        };
        let file = match file {
            Ok(file) => file,
            Err(why) => return failed(call, why),
        };
        if let Some(why) = self.failed.get(&asked.page) {
            return failed(
                call,
                format!("page {} cannot be read: {why}", asked.page + 1),
            );
        }
        let Some(leaf) = self.editor.leaf(asked.page).map(Arc::clone) else {
            return Performed::NeedPages(vec![asked.page]);
        };
        let placed = (|| {
            let page = desk::page_size_of(&leaf.view)?;
            let area = pictures::fit(asked, pictures::size_of(&file)?, page)?;
            let placement = pictures::placement(&leaf.view, area)?;
            Ok::<_, String>((area, placement))
        })();
        let (area, placement) = match placed {
            Ok(placed) => placed,
            Err(why) => return failed(call, why),
        };
        let job = self.editor.begin_command(
            pdf_edit::Command::PlaceNewImage {
                page_index: asked.page,
                placement,
                file,
            },
            Done::AddedPicture,
        );
        let sent = self.send_for(call, request, job, None);
        self.sent_changing(vec![asked.page], Some(pictures::said(asked.page, area)));
        sent
    }

    pub(super) fn objects(
        &mut self,
        call: &ToolCall,
        request: Request,
        action: &ObjectAction,
        pages: usize,
    ) -> Performed {
        if let ObjectAction::List { page } = action {
            return self.list_objects(call, *page, pages);
        }
        let (ObjectAction::Move { object, .. }
        | ObjectAction::Resize { object, .. }
        | ObjectAction::Delete { object }) = action
        else {
            return failed(call, "that is not a change to an object");
        };
        let name = match objects::parse_name(object) {
            Ok(name) => name,
            Err(why) => return failed(call, why),
        };
        let page = name.page();
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        if self.editor.leaf(page).is_none() {
            if let Some(why) = self.failed.get(&page) {
                return failed(call, format!("page {} cannot be read: {why}", page + 1));
            }
            return Performed::NeedPages(vec![page]);
        }
        let Some(leaf) = self.editor.leaf(page).map(Arc::clone) else {
            return Performed::NeedPages(vec![page]);
        };
        let made = match name {
            Name::Object { index, .. } => {
                let listed = self.ai.tools.listed.get(&(page, index));
                if listed.is_some_and(|listed| listed.arranged != self.ai.tools.arranged) {
                    return failed(
                        call,
                        format!(
                            "{object} was listed before the pages were moved about, and its page \
                             number no longer means the page it meant: call objects with action \
                             list again"
                        ),
                    );
                }
                let recorded = listed.map(|listed| listed.anchor.as_str());
                objects::the_object(&leaf.overlay.objects, index, recorded, object).and_then(
                    |(_, found)| objects::command_on_object(&leaf.view, page, found, action),
                )
            }
            Name::Block { .. } => match self.block_named(object) {
                Ok((page, index)) => objects::command_on_block(&leaf.view, page, index, action),
                Err(why) => Err(why),
            },
        };
        let (command, text) = match made {
            Ok(made) => made,
            Err(why) => return failed(call, why),
        };
        let done = match action {
            ObjectAction::Move { .. } => Done::MovedObject,
            ObjectAction::Resize { .. } => Done::ResizedObject,
            _ => Done::DeletedObject,
        };
        let named = match name {
            Name::Object { index, .. } => objects::object_name(page, index),
            Name::Block { .. } => object.clone(),
        };
        let job = self.editor.begin_command(command, done);
        let sent = self.send_for(call, request, job, None);
        self.ai.tools.listed.retain(|(held, _), _| *held != page);
        self.sent_changing(
            vec![page],
            Some(format!(
                "{named}: {text} The page changed, so list the objects (and read_text) again \
                 before naming anything on it."
            )),
        );
        sent
    }

    fn list_objects(&mut self, call: &ToolCall, page: usize, pages: usize) -> Performed {
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        if let Some(why) = self.failed.get(&page) {
            return failed(call, format!("page {} cannot be read: {why}", page + 1));
        }
        let Some(leaf) = self.editor.leaf(page).map(Arc::clone) else {
            return Performed::NeedPages(vec![page]);
        };
        let blocks: Vec<Block> = (0..leaf.view.index.blocks.len())
            .filter_map(|index| desk::read_block(&leaf.view, page, index))
            .collect();
        for block in &blocks {
            self.remember_the_name(block);
        }
        for (index, object) in leaf.overlay.objects.iter().enumerate() {
            self.ai.tools.listed.insert(
                (page, index),
                Listed {
                    arranged: self.ai.tools.arranged,
                    anchor: object.anchor.clone(),
                },
            );
        }
        said(call, objects::listing(page, &leaf.overlay.objects, &blocks))
    }

    pub(super) fn go_to_page(&mut self, call: &ToolCall, page: usize, pages: usize) -> Performed {
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        self.goto(page);
        said(
            call,
            format!(
                "The window now shows page {}. Nothing in the document changed.",
                page + 1
            ),
        )
    }

    pub(super) fn look_closer(
        &mut self,
        call: &ToolCall,
        (page, region, dpi): (usize, [f64; 4], f64),
        pages: usize,
    ) -> Performed {
        if page >= pages {
            return failed(call, no_page(page, pages));
        }
        if let Some(why) = self.failed.get(&page) {
            return failed(call, format!("page {} cannot be read: {why}", page + 1));
        }
        let Some(leaf) = self.editor.leaf(page).map(Arc::clone) else {
            return Performed::NeedPages(vec![page]);
        };
        match desk::region_picture_of(&leaf.view, region, dpi) {
            Ok((png, width, height)) => {
                let mut result = ToolResult::said(
                    &call.id,
                    pictures::said_close(page, region, (width, height)),
                );
                result.picture = Some(Picture {
                    media_type: "image/png".to_owned(),
                    base64: pdf_agent::json::base64(&png),
                });
                Performed::Done(result)
            }
            Err(why) => failed(call, why),
        }
    }
}

#[cfg(test)]
pub(super) mod tests;
