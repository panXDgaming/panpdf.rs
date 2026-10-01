use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdf_bytes::{ByteStore, SourceId};
use pdf_edit::{BlockRange, BlockReading, ClusterRef, Command, SourceAnchor};
use pdf_paint::Point;
use pdf_session::{PageView, Session};

pub const MOST_CHARACTERS: usize = 60_000;

pub const MOST_PIXELS: f64 = 2_400.0;

pub const MOST_FILE_BYTES: u64 = 512 * 1024 * 1024;

pub type Refused = String;

struct Open {
    path: PathBuf,
    original: Arc<[u8]>,
    session: Session,
    revision: u64,
    arranged: u64,
    named: BTreeMap<(usize, usize), Named>,
    listed: BTreeMap<(usize, usize), Listed>,
    saved: Option<u64>,
    restrictions_set_aside: bool,
}

fn unsaved(open: &Open) -> bool {
    match open.saved {
        Some(saved) => saved != open.revision,
        None => open.revision != 0 && !open.session.source().same_bytes_as(&open.original),
    }
}

pub fn read_a_file(path: &Path) -> Result<Vec<u8>, Refused> {
    let facts = std::fs::metadata(path)
        .map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    if !facts.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if facts.len() > MOST_FILE_BYTES {
        return Err(format!(
            "{} is too large to open: the most is {} MB",
            path.display(),
            MOST_FILE_BYTES / (1024 * 1024)
        ));
    }
    std::fs::read(path).map_err(|error| format!("{} cannot be read: {error}", path.display()))
}

#[derive(Clone, Debug)]
struct Listed {
    arranged: u64,
    anchor: String,
}

#[derive(Clone, Debug)]
struct Named {
    revision: u64,
    arranged: u64,
    text: String,
    area: [f64; 4],
}

#[derive(Clone, Debug)]
pub struct Block {
    pub page: usize,
    pub index: usize,
    pub text: String,
    pub area: [f64; 4],
    pub size: f64,
    pub fixed: Option<&'static str>,
}

impl Block {
    #[must_use]
    pub fn name(&self) -> String {
        format!("p{}-b{}", self.page + 1, self.index + 1)
    }
}

pub(crate) struct Parts {
    pub(crate) rows: Vec<Vec<ClusterRef>>,
    pub(crate) frame: (f64, f64),
    pub(crate) reading: BlockReading,
}

#[derive(Clone, Debug)]
pub struct Summary {
    pub handle: String,
    pub pages: usize,
    pub title: String,
    pub protected: bool,
    pub restricted: bool,
}

pub struct Desk {
    open: BTreeMap<String, Open>,
    opened: u64,
    fonts: Option<Arc<dyn pdf_content::FontProvider>>,
}

impl Default for Desk {
    fn default() -> Self {
        Self::with_fonts(pdf_cli::font_provider())
    }
}

impl Desk {
    #[must_use]
    pub fn with_fonts(fonts: Option<Arc<dyn pdf_content::FontProvider>>) -> Self {
        Self {
            open: BTreeMap::new(),
            opened: 0,
            fonts,
        }
    }

    pub fn open(
        &mut self,
        path: &Path,
        password: &str,
        set_aside_restrictions: bool,
    ) -> Result<Summary, Refused> {
        let bytes = read_a_file(path)?;
        let original: Arc<[u8]> = Arc::from(bytes);
        let source = ByteStore::new(SourceId::new(0), Arc::clone(&original));
        if pdf_edit::info::lock(&source, password.as_bytes()) == pdf_edit::info::Lock::Refused {
            return Err(if password.is_empty() {
                "this document is protected by a password: ask the person for it and open it again with `password`".to_owned()
            } else {
                "that password does not open this document".to_owned()
            });
        }
        let mut session = Session::with_fonts(source, password.as_bytes(), self.fonts.clone());
        let pages = session
            .page_count()
            .map_err(|error| format!("this document's pages cannot be read: {error}"))?;
        if pages == 0 {
            return Err("this document has no pages".to_owned());
        }
        let restricted = session.restricts_editing().unwrap_or(true);
        if restricted && set_aside_restrictions {
            session.set_aside_restrictions();
        }
        let facts = pdf_edit::info::document_facts(session.source(), password.as_bytes()).ok();
        self.opened += 1;
        let handle = format!("doc-{}", self.opened);
        self.open.insert(
            handle.clone(),
            Open {
                path: path.to_owned(),
                original,
                session,
                revision: 0,
                arranged: 0,
                named: BTreeMap::new(),
                listed: BTreeMap::new(),
                saved: None,
                restrictions_set_aside: restricted && set_aside_restrictions,
            },
        );
        Ok(Summary {
            handle,
            pages,
            title: facts
                .as_ref()
                .map(|facts| facts.info.title.clone())
                .unwrap_or_default(),
            protected: facts.is_some_and(|facts| facts.protection.is_some()),
            restricted,
        })
    }

    pub fn close(&mut self, handle: &str) -> Result<bool, Refused> {
        let open = self.open.remove(handle).ok_or_else(|| unknown(handle))?;
        Ok(unsaved(&open))
    }

    #[must_use]
    pub fn handles(&self) -> Vec<(String, PathBuf, usize, bool)> {
        self.open
            .iter()
            .map(|(handle, open)| {
                (
                    handle.clone(),
                    open.path.clone(),
                    open.session.page_count().unwrap_or(0),
                    unsaved(open),
                )
            })
            .collect()
    }

    fn get(&mut self, handle: &str) -> Result<&mut Open, Refused> {
        self.open.get_mut(handle).ok_or_else(|| unknown(handle))
    }

    pub fn page_count(&mut self, handle: &str) -> Result<usize, Refused> {
        let open = self.get(handle)?;
        open.session
            .page_count()
            .map_err(|error| format!("the pages cannot be counted: {error}"))
    }

    pub fn page_sizes(&mut self, handle: &str) -> Result<Vec<[f64; 2]>, Refused> {
        let open = self.get(handle)?;
        let geometries = open
            .session
            .page_geometries()
            .map_err(|error| format!("the pages cannot be measured: {error}"))?;
        Ok(shown_sizes(&geometries))
    }

    pub fn source(&mut self, handle: &str) -> Result<(ByteStore, Vec<u8>), Refused> {
        let open = self.get(handle)?;
        Ok((
            open.session.source().clone(),
            open.session.credential().to_vec(),
        ))
    }

    pub fn blocks(&mut self, handle: &str, page: usize) -> Result<Vec<Block>, Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, page)?;
        let blocks: Vec<Block> = (0..view.index.blocks.len())
            .filter_map(|index| read(&view, page, index).map(|(block, _)| block))
            .collect();
        for block in &blocks {
            open.named.insert(
                (page, block.index),
                Named {
                    revision: open.revision,
                    arranged: open.arranged,
                    text: block.text.clone(),
                    area: block.area,
                },
            );
        }
        Ok(blocks)
    }

    fn resolve(open: &mut Open, name: &str) -> Result<(usize, usize, Arc<PageView>), Refused> {
        let (page, index) = parse_name(name)?;
        let named = open.named.get(&(page, index)).cloned().ok_or_else(|| {
            format!("{name} has not been read yet: call read_text or find_text for that page first")
        })?;
        if named.arranged != open.arranged {
            return Err(format!(
                "{name} was read before the pages were moved about, and its page number \
                 no longer means the page it meant: read_text again and use the name it \
                 gives now"
            ));
        }
        let view = view_of(&mut open.session, page)?;
        if named.revision == open.revision {
            return Ok((page, index, view));
        }
        let same: Vec<usize> = (0..view.index.blocks.len())
            .filter(|at| {
                read(&view, page, *at).is_some_and(|(block, _)| {
                    block.text == named.text && overlaps(block.area, named.area)
                })
            })
            .collect();
        match same[..] {
            [only] => Ok((page, only, view)),
            [] => Err(format!(
                "{name} is no longer on the page as it was read: read_text page {} again",
                page + 1
            )),
            _ => Err(format!(
                "{name} cannot be told apart from another block since the page changed: \
                 read_text page {} again",
                page + 1
            )),
        }
    }

    pub fn rewrite(
        &mut self,
        handle: &str,
        name: &str,
        find: Option<&str>,
        text: &str,
    ) -> Result<Option<Block>, Refused> {
        let open = self.get(handle)?;
        let (page, index, view) = Self::resolve(open, name)?;
        let (block, parts) =
            read(&view, page, index).ok_or_else(|| format!("{name} cannot be read as text"))?;
        if let Some(reason) = block.fixed {
            return Err(format!("{name} cannot be rewritten: {reason}"));
        }
        let deleting = find.is_none() && text.is_empty();
        let Parts {
            rows,
            frame,
            reading,
        } = parts;
        let range = match find {
            None => crate::finding::whole(&reading),
            Some(find) => {
                range_within(&reading, find).map_err(|problem| format!("{name}: {problem}"))?
            }
        };
        let text = text.replace("\r\n", "\n");
        let command = Command::RewriteBlock {
            page_index: page,
            rows,
            frame,
            edges: (0, 0),
            breaks: None,
            frame_declared: false,
            paragraph: pdf_edit::ParagraphLayout::default(),
            range,
            text,
        };
        apply(open, &command)?;
        if deleting {
            return Ok(None);
        }
        Self::read_back(open, page, block.area)
    }

    fn read_back(open: &mut Open, page: usize, area: [f64; 4]) -> Result<Option<Block>, Refused> {
        let view = view_of(&mut open.session, page)?;
        let now = (0..view.index.blocks.len())
            .filter_map(|at| read(&view, page, at).map(|(block, _)| block))
            .filter(|now| overlaps(now.area, area))
            .max_by(|one, other| overlap(one.area, area).total_cmp(&overlap(other.area, area)));
        if let Some(now) = &now {
            open.named.insert(
                (page, now.index),
                Named {
                    revision: open.revision,
                    arranged: open.arranged,
                    text: now.text.clone(),
                    area: now.area,
                },
            );
        }
        Ok(now)
    }

    pub fn style(
        &mut self,
        handle: &str,
        name: &str,
        find: Option<&str>,
        look: &crate::styling::Look,
    ) -> Result<Block, Refused> {
        let open = self.get(handle)?;
        let (page, index, view) = Self::resolve(open, name)?;
        let (command, block) = crate::styling::style_command(&view, page, index, find, look)?;
        apply(open, &command)?;
        Ok(Self::read_back(open, page, block.area)?.unwrap_or(block))
    }

    pub fn place_text(
        &mut self,
        handle: &str,
        page: usize,
        area: [f64; 4],
        text: &str,
        style: (&str, f64, bool, bool, Option<[f64; 3]>),
    ) -> Result<(), Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, page)?;
        let frame = to_text_frame(&view, area)?;
        let (family, size, bold, italic, fill) = style;
        let command = Command::PlaceNewText {
            page_index: page,
            frame,
            text: text.replace("\r\n", "\n"),
            family: family.to_owned(),
            size,
            bold,
            italic,
            fill,
            paragraph: pdf_edit::ParagraphLayout::default(),
        };
        apply(open, &command)
    }

    pub fn replace_everywhere(
        &mut self,
        handle: &str,
        (search, with): (&crate::finding::Search, &str),
        (first, last): (usize, usize),
    ) -> Result<crate::finding::Replaced, Refused> {
        let open = self.get(handle)?;
        let mut works = Vec::new();
        let mut blocks = 0;
        for page in first..=last {
            if blocks >= crate::finding::MOST_BLOCKS {
                break;
            }
            let view = view_of(&mut open.session, page)?;
            let session = &mut open.session;
            let work =
                crate::finding::replacements_on(&view, page, (search, with), &mut |command| {
                    session
                        .plan(command)
                        .map(drop)
                        .map_err(|error| error.to_string())
                });
            blocks += work.commands.len();
            works.push(work);
        }
        let replaced = crate::finding::Replaced::of(works);
        if !replaced.commands.is_empty() {
            open.session
                .apply_each(&replaced.commands)
                .map_err(|error| format!("nothing was replaced: {error}"))?;
            open.revision += 1;
            note_any_page_move(open);
        }
        Ok(replaced)
    }

    pub fn mark_text(
        &mut self,
        handle: &str,
        (search, marking): (&crate::finding::Search, crate::marking::Marking),
        (first, last): (usize, usize),
    ) -> Result<crate::finding::Replaced, Refused> {
        let open = self.get(handle)?;
        let mut works = Vec::new();
        let mut marks = 0;
        for page in first..=last {
            if marks >= crate::marking::MOST_MARKS {
                break;
            }
            let view = view_of(&mut open.session, page)?;
            let overlay = pdf_cli::page_overlay_view(&view, 1.0)
                .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
            let work = crate::marking::marks_on(&view, page, &overlay.clusters, (search, marking));
            marks += work.found;
            works.push(work);
        }
        let marked = crate::finding::Replaced::of(works);
        if !marked.commands.is_empty() {
            open.session
                .apply_each(&marked.commands)
                .map_err(|error| format!("nothing was marked: {error}"))?;
            open.revision += 1;
            note_any_page_move(open);
        }
        Ok(marked)
    }

    pub fn stamp(
        &mut self,
        handle: &str,
        asked: &crate::stamping::Asked,
    ) -> Result<String, Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| format!("the pages cannot be counted: {error}"))?;
        let prepared = asked.prepare(count)?;
        let name = open
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let around = (count, name.as_str(), &crate::stamping::today());
        let commands = prepared.commands((around.0, around.1, around.2));
        open.session.apply_each(&commands).map_err(|error| {
            error
                .to_string()
                .trim_start_matches("cannot type here: ")
                .to_owned()
        })?;
        open.revision += 1;
        note_any_page_move(open);
        Ok(prepared.said((around.0, around.1, around.2)))
    }

    pub fn bookmarks(
        &mut self,
        handle: &str,
        action: &crate::outlining::Action,
    ) -> Result<String, Refused> {
        use crate::outlining::{Action, Place, change_for, commands_for_headings};
        let fonts = self.fonts.clone();
        let open = self.get(handle)?;
        let outline_of = |open: &Open| {
            pdf_edit::outline::read_outline(open.session.source(), open.session.credential())
                .map_err(|error| format!("the bookmarks cannot be read: {error}"))
        };
        let before = outline_of(open)?;
        let said = match action {
            Action::List => return Ok(crate::outlining::listing(&before)),
            Action::FromHeadings { replace } => {
                if !replace && !before.is_empty() {
                    return Err(format!(
                        "the document already has {} bookmark{}: pass replace: true to take them \
                         out and make new ones from the headings, or add the missing ones one by one",
                        before.len(),
                        if before.len() == 1 { "" } else { "s" }
                    ));
                }
                let count = open
                    .session
                    .page_count()
                    .map_err(|error| error.to_string())?;
                let mut pages = Vec::with_capacity(count);
                for page in 0..count {
                    let view = view_of(&mut open.session, page)?;
                    let blocks = (0..view.index.blocks.len())
                        .filter_map(|index| read_block(&view, page, index))
                        .collect();
                    pages.push((page, blocks));
                }
                let headings = crate::outlining::headings_in(&pages);
                if headings.is_empty() {
                    return Err(
                        "no heading was found: the text is all of one size, or none is \
                                set larger than the rest"
                            .to_owned(),
                    );
                }
                let first = crate::outlining::first_new_object(
                    open.session.source(),
                    open.session.credential(),
                    fonts,
                    headings[0].page,
                )?;
                let removing: &[pdf_edit::outline::Bookmark] = if *replace { &before } else { &[] };
                let commands = commands_for_headings(&headings, first, removing);
                open.session
                    .apply_each(&commands)
                    .map_err(|error| format!("no bookmark was made: {error}"))?;
                open.revision += 1;
                crate::outlining::said_headings(
                    &headings,
                    removing.iter().filter(|book| book.depth == 0).count(),
                )
            }
            other => {
                let block = match other {
                    Action::Add {
                        place: Place::Block(name),
                        ..
                    } => {
                        let (page, index, view) = Self::resolve(open, name)?;
                        let (block, _) = read(&view, page, index)
                            .ok_or_else(|| format!("{name} cannot be read as text"))?;
                        Some((page, block.text))
                    }
                    _ => None,
                };
                let pages = open
                    .session
                    .page_count()
                    .map_err(|error| error.to_string())?;
                let change = change_for(&before, other, (block.as_ref(), pages))?;
                let page_index = match &change {
                    pdf_edit::outline::Change::Add { page, .. }
                    | pdf_edit::outline::Change::Retarget { page, .. } => *page,
                    _ => 0,
                };
                let said = crate::outlining::said_change(&change, &before);
                apply(open, &Command::ChangeOutline { page_index, change })?;
                said
            }
        };
        let after = outline_of(open)?;
        Ok(format!("{said}\n{}", crate::outlining::listing(&after)))
    }

    pub fn bottom_of_everything(
        &mut self,
        handle: &str,
        page: usize,
    ) -> Result<Option<f64>, Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, page)?;
        let overlay = pdf_cli::page_overlay_view(&view, 1.0)
            .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
        Ok((0..view.index.blocks.len())
            .filter_map(|index| read_block(&view, page, index))
            .map(|block| block.area[3])
            .chain(overlay.objects.iter().map(|object| object.box_pixels[3]))
            .max_by(f64::total_cmp))
    }

    pub fn place_picture(
        &mut self,
        handle: &str,
        asked: &crate::pictures::Asked,
        file: Arc<[u8]>,
    ) -> Result<[f64; 4], Refused> {
        let open = self.get(handle)?;
        let view = view_of(&mut open.session, asked.page)?;
        let page = page_size_of(&view)?;
        let area = crate::pictures::fit(asked, crate::pictures::size_of(&file)?, page)?;
        let placement = crate::pictures::placement(&view, area)?;
        apply(
            open,
            &Command::PlaceNewImage {
                page_index: asked.page,
                placement,
                file,
            },
        )?;
        open.listed.retain(|(page, _), _| *page != asked.page);
        Ok(area)
    }

    pub fn objects(
        &mut self,
        handle: &str,
        action: &crate::objects::Action,
    ) -> Result<String, Refused> {
        use crate::objects::{Action, Name, listing, object_name, parse_name, the_object};
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        if let Action::List { page } = action {
            let view = view_of(&mut open.session, *page)?;
            let overlay = pdf_cli::page_overlay_view(&view, 1.0)
                .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
            for (index, object) in overlay.objects.iter().enumerate() {
                open.listed.insert(
                    (*page, index),
                    Listed {
                        arranged: open.arranged,
                        anchor: object.anchor.clone(),
                    },
                );
            }
            let blocks: Vec<Block> = (0..view.index.blocks.len())
                .filter_map(|index| read_block(&view, *page, index))
                .collect();
            return Ok(listing(*page, &overlay.objects, &blocks));
        }
        let (Action::Move { object, .. }
        | Action::Resize { object, .. }
        | Action::Delete { object }) = action
        else {
            return Err("that is not a change to an object".to_owned());
        };
        let name = parse_name(object)?;
        if name.page() >= count {
            return Err(no_page(name.page(), count));
        }
        let (command, said) = match name {
            Name::Object { page, index } => {
                let listed = open.listed.get(&(page, index)).cloned();
                if listed
                    .as_ref()
                    .is_some_and(|listed| listed.arranged != open.arranged)
                {
                    return Err(format!(
                        "{object} was listed before the pages were moved about, and its page \
                         number no longer means the page it meant: call list_objects again"
                    ));
                }
                let view = view_of(&mut open.session, page)?;
                let overlay = pdf_cli::page_overlay_view(&view, 1.0)
                    .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
                let (_, found) = the_object(
                    &overlay.objects,
                    index,
                    listed.as_ref().map(|listed| listed.anchor.as_str()),
                    object,
                )?;
                crate::objects::command_on_object(&view, page, found, action)?
            }
            Name::Block { .. } => {
                let (page, index, view) = Self::resolve(open, object)?;
                crate::objects::command_on_block(&view, page, index, action)?
            }
        };
        apply(open, &command)?;
        let page = name.page();
        open.listed.retain(|(held, _), _| *held != page);
        let named = match name {
            Name::Object { index, .. } => object_name(page, index),
            Name::Block { .. } => object.clone(),
        };
        Ok(format!(
            "{named}: {said} The page changed, so list_objects (and read_text) again before naming anything on it."
        ))
    }

    pub fn look_closer(
        &mut self,
        handle: &str,
        page: usize,
        region: [f64; 4],
        dpi: f64,
    ) -> Result<(Vec<u8>, u32, u32), Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        if page >= count {
            return Err(no_page(page, count));
        }
        let view = open
            .session
            .page_for_display(page)
            .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
        region_picture_of(&view, region, dpi)
    }

    #[must_use]
    pub fn fonts(&self) -> Option<Arc<dyn pdf_content::FontProvider>> {
        self.fonts.clone()
    }

    pub fn command(&mut self, handle: &str, command: &Command) -> Result<(), Refused> {
        let open = self.get(handle)?;
        apply(open, command)
    }

    pub fn commands(&mut self, handle: &str, commands: &[Command]) -> Result<(), Refused> {
        let open = self.get(handle)?;
        open.session
            .apply_each(commands)
            .map_err(|error| error.to_string())?;
        open.revision += 1;
        note_any_page_move(open);
        Ok(())
    }

    pub fn page_geometries(
        &mut self,
        handle: &str,
    ) -> Result<Vec<pdf_content::PageGeometry>, Refused> {
        self.get(handle)?
            .session
            .page_geometries()
            .map_err(|error| format!("the pages cannot be measured: {error}"))
    }

    pub fn walk(&mut self, handle: &str, back: bool) -> Result<bool, Refused> {
        let open = self.get(handle)?;
        let walked = if back {
            open.session.undo()
        } else {
            open.session.redo()
        }
        .map_err(|error| error.to_string())?;
        if walked {
            open.revision += 1;
            note_any_page_move(open);
        }
        Ok(walked)
    }

    pub fn picture(
        &mut self,
        handle: &str,
        page: usize,
        dpi: f64,
    ) -> Result<(Vec<u8>, u32, u32), Refused> {
        let open = self.get(handle)?;
        let count = open
            .session
            .page_count()
            .map_err(|error| error.to_string())?;
        if page >= count {
            return Err(no_page(page, count));
        }
        let view = open
            .session
            .page_for_display(page)
            .map_err(|error| format!("page {} cannot be read: {error}", page + 1))?;
        picture_of(&view, dpi)
    }

    pub fn save(
        &mut self,
        handle: &str,
        destination: &Path,
        replace: bool,
    ) -> Result<u64, Refused> {
        let open = self.get(handle)?;
        let exists = destination.exists();
        if exists && !replace {
            return Err(format!(
                "{} already exists: save under another name, or pass replace: true once the person agrees",
                destination.display()
            ));
        }
        let same_file = exists
            && std::fs::canonicalize(destination).ok() == std::fs::canonicalize(&open.path).ok();
        if same_file {
            let on_disk = std::fs::read(destination).map_err(|error| error.to_string())?;
            if on_disk.as_slice() != &*open.original {
                return Err(format!(
                    "{} changed on disk since it was opened: save under another name",
                    destination.display()
                ));
            }
        } else if exists && !destination.is_file() {
            return Err(format!("{} is not a file", destination.display()));
        }
        let source = open.session.source().clone();
        let target = if exists {
            std::fs::canonicalize(destination).unwrap_or_else(|_| destination.to_owned())
        } else {
            destination.to_owned()
        };
        let directory = target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = target
            .file_name()
            .ok_or_else(|| "a file name is needed".to_owned())?;
        let mut temporary_name = std::ffi::OsString::from(".");
        temporary_name.push(name);
        temporary_name.push(format!(".panpdf-{}.tmp", std::process::id()));
        let temporary = directory.join(temporary_name);
        let written = (|| {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            for run in source.runs() {
                file.write_all(run)?;
            }
            file.sync_all()?;
            if exists && let Ok(held) = std::fs::metadata(&target) {
                std::fs::set_permissions(&temporary, held.permissions())?;
            }
            std::fs::rename(&temporary, &target)
        })();
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "{} could not be written: {error}",
                destination.display()
            ));
        }
        if same_file {
            open.original = Arc::from(source.to_vec());
        }
        open.saved = Some(open.revision);
        Ok(source.len() as u64)
    }

    pub fn restrictions_set_aside(&mut self, handle: &str) -> Result<bool, Refused> {
        Ok(self.get(handle)?.restrictions_set_aside)
    }
}

fn unknown(handle: &str) -> Refused {
    format!(
        "no document is open as {handle}: open_document first, or list_documents to see what is open"
    )
}

fn no_page(page: usize, count: usize) -> Refused {
    format!("there is no page {}: the document has {count}", page + 1)
}

pub fn picture_of(view: &PageView, dpi: f64) -> Result<(Vec<u8>, u32, u32), Refused> {
    let device = pdf_render::DeviceTransform::for_page(
        &view.program.geometry,
        1.0,
        pdf_render::RenderLimits::default(),
    )
    .map_err(|_| "this page has no size".to_owned())?;
    let longest = f64::from(device.width.max(device.height)).max(1.0);
    let scale = (dpi / 72.0).min(MOST_PIXELS / longest).max(0.05);
    let (canvas, _) = pdf_cli::render_page_view(view, scale)
        .map_err(|error| format!("it cannot be drawn: {error}"))?;
    let png = pdf_edit::png::write((canvas.width, canvas.height), &canvas.to_rgb8(), None)
        .map_err(str::to_owned)?;
    Ok((png, canvas.width, canvas.height))
}

pub fn page_size_of(view: &PageView) -> Result<[f64; 2], Refused> {
    let device = device(view)?;
    Ok([f64::from(device.width), f64::from(device.height)])
}

pub fn region_picture_of(
    view: &PageView,
    region: [f64; 4],
    dpi: f64,
) -> Result<(Vec<u8>, u32, u32), Refused> {
    let device = device(view)?;
    let page = [f64::from(device.width), f64::from(device.height)];
    let kept = [
        region[0].clamp(0.0, page[0]),
        region[1].clamp(0.0, page[1]),
        region[2].clamp(0.0, page[0]),
        region[3].clamp(0.0, page[1]),
    ];
    let (wide, high) = (kept[2] - kept[0], kept[3] - kept[1]);
    if wide < 1.0 || high < 1.0 {
        return Err(format!(
            "the region [{:.0}, {:.0}, {:.0}, {:.0}] has nothing of the page in it: the page is \
             {:.0} x {:.0} pt, and a region is [left, top, right, bottom] from the top-left",
            region[0], region[1], region[2], region[3], page[0], page[1]
        ));
    }
    let scale = (dpi / 72.0).min(MOST_PIXELS / wide.max(high)).max(0.05);
    let user = to_user(view, kept)?;
    let (canvas, _) = pdf_cli::render_region_view(view, scale, user)
        .map_err(|error| format!("it cannot be drawn at that resolution: {error}"))?
        .ok_or_else(|| "that region touches no pixel of this page".to_owned())?;
    let png = pdf_edit::png::write((canvas.width, canvas.height), &canvas.to_rgb8(), None)
        .map_err(str::to_owned)?;
    Ok((png, canvas.width, canvas.height))
}

#[must_use]
pub fn text_of_page(view: &PageView, page: usize, most: usize) -> String {
    let mut text = String::new();
    for index in 0..view.index.blocks.len() {
        if text.chars().count() >= most {
            break;
        }
        let Some((block, _)) = read(view, page, index) else {
            continue;
        };
        if !text.is_empty() {
            text.push('\n');
        }
        let room = most.saturating_sub(text.chars().count());
        text.extend(block.text.chars().take(room));
    }
    text
}

pub(crate) fn view_of(session: &mut Session, page: usize) -> Result<Arc<PageView>, Refused> {
    let count = session.page_count().map_err(|error| error.to_string())?;
    if page >= count {
        return Err(no_page(page, count));
    }
    session
        .page(page)
        .map_err(|error| format!("page {} cannot be read: {error}", page + 1))
}

fn apply(open: &mut Open, command: &Command) -> Result<(), Refused> {
    let plan = open
        .session
        .plan(command)
        .map_err(|error| error.to_string())?;
    open.session
        .apply(plan)
        .map_err(|error| error.to_string())?;
    open.revision += 1;
    note_any_page_move(open);
    Ok(())
}

fn note_any_page_move(open: &mut Open) {
    if open.session.last_pages().is_some() {
        open.arranged += 1;
    }
}

fn parse_name(name: &str) -> Result<(usize, usize), Refused> {
    let bad = || format!("{name:?} is not a block name: they look like p3-b12");
    let rest = name.trim().strip_prefix('p').ok_or_else(bad)?;
    let (page, block) = rest.split_once("-b").ok_or_else(bad)?;
    let page: usize = page.parse().map_err(|_| bad())?;
    let block: usize = block.parse().map_err(|_| bad())?;
    if page == 0 || block == 0 {
        return Err(bad());
    }
    Ok((page - 1, block - 1))
}

pub(crate) fn device(view: &PageView) -> Result<pdf_render::DeviceTransform, Refused> {
    pdf_render::DeviceTransform::for_page(
        &view.program.geometry,
        1.0,
        pdf_render::RenderLimits::default(),
    )
    .map_err(|_| "this page has no size".to_owned())
}

pub(crate) fn to_shown(
    device: &pdf_render::DeviceTransform,
    [x0, y0, x1, y1]: [f64; 4],
) -> [f64; 4] {
    let corners = [(x0, y0), (x0, y1), (x1, y0), (x1, y1)]
        .map(|(x, y)| device.matrix.transform(Point { x, y }));
    let left = corners
        .iter()
        .map(|point| point.x)
        .fold(f64::INFINITY, f64::min);
    let right = corners
        .iter()
        .map(|point| point.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let top = corners
        .iter()
        .map(|point| point.y)
        .fold(f64::INFINITY, f64::min);
    let bottom = corners
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);
    [left, top, right, bottom].map(|value| (value * 100.0).round() / 100.0)
}

pub fn to_user(view: &PageView, area: [f64; 4]) -> Result<[f64; 4], Refused> {
    to_user_with(&device(view)?.matrix, area)
}

pub fn to_text_frame(view: &PageView, area: [f64; 4]) -> Result<[f64; 4], Refused> {
    to_text_frame_with(&device(view)?.matrix, area)
}

pub fn to_text_frame_with(shown: &pdf_paint::Matrix, area: [f64; 4]) -> Result<[f64; 4], Refused> {
    if shown.b.abs() > 1e-9 || shown.c.abs() > 1e-9 {
        return Err(
            "this page is shown turned, and new text on a turned page is not laid out \
                    yet: turn the page back before writing on it"
                .to_owned(),
        );
    }
    to_user_with(shown, area)
}

pub fn to_user_with(
    shown: &pdf_paint::Matrix,
    [left, top, right, bottom]: [f64; 4],
) -> Result<[f64; 4], Refused> {
    let inverse = shown
        .inverse()
        .ok_or_else(|| "this page has no size".to_owned())?;
    let one = inverse.transform(Point { x: left, y: top });
    let other = inverse.transform(Point {
        x: right,
        y: bottom,
    });
    Ok([
        one.x.min(other.x),
        one.y.min(other.y),
        one.x.max(other.x),
        one.y.max(other.y),
    ])
}

pub fn steps_in_user_space_with(
    shown: &pdf_paint::Matrix,
    steps: &[pdf_edit::PenStep],
) -> Result<Vec<pdf_edit::PenStep>, Refused> {
    let inverse = shown
        .inverse()
        .ok_or_else(|| "this page has no size".to_owned())?;
    let thousandths = |value: f64| (value * 1000.0).round() / 1000.0;
    let point = |(x, y): (f64, f64)| {
        let at = inverse.transform(Point { x, y });
        (thousandths(at.x), thousandths(at.y))
    };
    Ok(steps
        .iter()
        .map(|step| match *step {
            pdf_edit::PenStep::Move(at) => pdf_edit::PenStep::Move(point(at)),
            pdf_edit::PenStep::Line(at) => pdf_edit::PenStep::Line(point(at)),
            pdf_edit::PenStep::Curve(one, other, end) => {
                pdf_edit::PenStep::Curve(point(one), point(other), point(end))
            }
        })
        .collect())
}

#[must_use]
pub fn overlap(one: [f64; 4], other: [f64; 4]) -> f64 {
    let width = one[2].min(other[2]) - one[0].max(other[0]);
    let height = one[3].min(other[3]) - one[1].max(other[1]);
    if width <= 0.0 || height <= 0.0 {
        0.0
    } else {
        width * height
    }
}

#[must_use]
pub fn overlaps(one: [f64; 4], other: [f64; 4]) -> bool {
    overlap(one, other) > 0.0
}

#[must_use]
pub fn read_block(view: &PageView, page: usize, index: usize) -> Option<Block> {
    read(view, page, index).map(|(block, _)| block)
}

#[must_use]
pub fn shown_sizes(geometries: &[pdf_content::PageGeometry]) -> Vec<[f64; 2]> {
    geometries
        .iter()
        .map(|geometry| {
            pdf_render::DeviceTransform::for_page(
                geometry,
                1.0,
                pdf_render::RenderLimits::default(),
            )
            .map_or([0.0, 0.0], |device| {
                [f64::from(device.width), f64::from(device.height)]
            })
        })
        .collect()
}

pub(crate) fn read(view: &PageView, page: usize, index: usize) -> Option<(Block, Parts)> {
    let semantic = view.index.blocks.get(index)?;
    let rows: Vec<Vec<ClusterRef>> = semantic
        .lines
        .iter()
        .map(|line| {
            view.index.lines[*line]
                .clusters
                .iter()
                .map(|cluster| {
                    let cluster = &view.index.clusters[*cluster];
                    ClusterRef {
                        anchor: SourceAnchor::of(&view.graph.atoms[cluster.atom].id),
                        glyphs: cluster.glyphs.clone(),
                    }
                })
                .collect()
        })
        .collect();
    let laid = semantic.layout.or(semantic.bounds)?;
    let device = device(view).ok()?;
    let upright = device.matrix.b.abs() < 1e-9 && device.matrix.c.abs() < 1e-9;
    let frame = (laid[0], laid[2]);
    let reading =
        pdf_edit::read_block(&view.program, &view.graph, &rows, frame, (0, 0), None).ok()?;
    let end = reading
        .lines
        .len()
        .checked_sub(1)
        .map(|last| (last, reading.lines[last].clusters.len()))?;
    let text = reading.text_between((0, 0), end)?;
    if text.trim().is_empty() {
        return None;
    }
    let fixed = if !upright {
        Some("the page is shown turned, and text on a turned page is not laid out again yet")
    } else if reading.turn.abs() > 1e-9 {
        Some("the text is set at an angle, and turned text is not laid out again yet")
    } else {
        None
    };
    let size = reading.lines.first().map_or(0.0, |line| line.em);
    Some((
        Block {
            page,
            index,
            text,
            area: to_shown(&device, laid),
            size: (size * 10.0).round() / 10.0,
            fixed,
        },
        Parts {
            rows,
            frame,
            reading,
        },
    ))
}

pub fn range_within(reading: &BlockReading, find: &str) -> Result<BlockRange, String> {
    if find.is_empty() {
        return Err("`find` is empty".to_owned());
    }
    let matches = crate::finding::matches_in(reading, &crate::finding::Search::exactly(find));
    match matches.found[..] {
        [ref only] if matches.inside_a_character == 0 => Ok(only.range()),
        [] if matches.inside_a_character == 0 => Err(format!("{find:?} is not in the block")),
        [] => Err(format!(
            "{find:?} starts or ends inside one character of the block: include the whole character"
        )),
        _ => Err(format!(
            "{find:?} is in the block {} times: give more of the text around it so it is found once",
            matches.found.len() + matches.inside_a_character
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests;
