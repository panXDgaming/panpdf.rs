use eframe::egui;

use pdf_app::wording::{Command, Message};

use crate::window_state::{Tool, Window};

pub(crate) const PICTURE_DPI: f64 = 300.0;

const MOST_PICTURE_PIXELS: f64 = 6000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PictureMenu {
    pub(crate) at: egui::Pos2,
    pub(crate) page: usize,
    pub(crate) object: usize,
}

pub(crate) fn picture_png(
    view: &pdf_session::PageView,
    box_pixels: [f64; 4],
) -> Result<Vec<u8>, String> {
    let (width, height) = (box_pixels[2] - box_pixels[0], box_pixels[3] - box_pixels[1]);
    if width <= 0.0 || height <= 0.0 {
        return Err("the picture has no size on the page".to_owned());
    }
    let wanted = PICTURE_DPI / 72.0 / pdf_app::document::OVERLAY_SCALE;
    let scale = wanted.min(MOST_PICTURE_PIXELS / width.max(height));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let region = [
        (box_pixels[0] * scale).round().max(0.0) as u32,
        (box_pixels[1] * scale).round().max(0.0) as u32,
        (box_pixels[2] * scale).round().max(0.0) as u32,
        (box_pixels[3] * scale).round().max(0.0) as u32,
    ];
    let options = pdf_render::RenderOptions {
        scale: scale * pdf_app::document::OVERLAY_SCALE,
        ..pdf_render::RenderOptions::default()
    };
    let (canvas, _) =
        pdf_render::render_region_layers(&view.layers(), &view.program.geometry, options, region)
            .map_err(|error| error.to_string())?;
    let metre = pdf_edit::png::per_metre(scale * pdf_app::document::OVERLAY_SCALE * 72.0);
    pdf_edit::png::write(
        (canvas.width, canvas.height),
        &canvas.to_rgb8(),
        Some((metre, metre)),
    )
    .map_err(str::to_owned)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Reading {
    pub(crate) page: usize,
    pub(crate) anchor: usize,
    pub(crate) caret: usize,
}

impl Window {
    pub(crate) fn edit_when_asked(&mut self) {
        if self.viewing && (self.tool != Tool::Select || self.stamp_draft.is_some()) {
            self.viewing = false;
            self.reading = None;
        }
    }

    pub(crate) fn view_pointer(&mut self, ctx: &egui::Context, response: &egui::Response) {
        self.view_cursor(ctx, response);
        if response.secondary_clicked()
            && let Some(at) = response.interact_pointer_pos()
            && let Some((page, point)) = self.page_point(at)
        {
            self.picture_menu =
                self.picture_at(page, point)
                    .map(|object| PictureMenu { at, page, object });
            return;
        }
        if response.clicked() {
            self.picture_menu = None;
        }
        if response.clicked()
            && !self.editor.is_busy()
            && let Some(at) = response.interact_pointer_pos()
            && let Some((page, point)) = self.page_point(at)
            && let Some(link) = self.editor.link_at(page, point)
        {
            self.follow(&link);
            return;
        }
        if self.clicked_a_field(ctx, response) {
            return;
        }
        if response.drag_started() {
            let from = ctx
                .input(|input| input.pointer.press_origin())
                .or_else(|| response.interact_pointer_pos());
            self.reading = from.and_then(|at| {
                let (page, point) = self.page_point(at)?;
                let stop = pdf_app::view::caret_at(&self.overlay(page)?.carets, point)?;
                Some(Reading {
                    page,
                    anchor: stop,
                    caret: stop,
                })
            });
        } else if response.dragged()
            && let Some(reading) = self.reading
            && let Some(at) = response.interact_pointer_pos()
            && let Some(laid) = self.laid.iter().find(|laid| laid.page == reading.page)
            && let Some(point) = laid.placed.point_in_page((at.x, at.y))
            && let Some(stop) = self
                .overlay(reading.page)
                .and_then(|overlay| pdf_app::view::caret_at(&overlay.carets, point))
        {
            self.reading = Some(Reading {
                caret: stop,
                ..reading
            });
        } else if response.clicked() {
            self.reading = None;
        }
    }

    fn view_cursor(&mut self, ctx: &egui::Context, response: &egui::Response) {
        let Some((page, point)) = response.hover_pos().and_then(|at| self.page_point(at)) else {
            return;
        };
        if self.editor.link_at(page, point).is_some() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            return;
        }
        let over = |quad: &[f64; 4]| {
            point.0 >= quad[0] && point.0 <= quad[2] && point.1 >= quad[1] && point.1 <= quad[3]
        };
        if self
            .overlay(page)
            .is_some_and(|overlay| overlay.blocks.iter().any(|block| over(&block.box_pixels)))
        {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }

    pub(crate) fn reading_spans(&self, page: usize) -> Vec<(usize, usize, usize)> {
        let (Some(reading), Some(overlay)) = (self.reading, self.overlay(page)) else {
            return Vec::new();
        };
        if reading.page != page {
            return Vec::new();
        }
        let rows = pdf_app::view::page_rows(&overlay.carets);
        pdf_app::view::selection_rows(&overlay.carets, &rows, reading.anchor, reading.caret)
    }

    pub(crate) fn copy_reading(&mut self, ctx: &egui::Context) -> bool {
        let Some(reading) = self.reading else {
            return false;
        };
        let spans = self.reading_spans(reading.page);
        let Some(overlay) = self.overlay(reading.page) else {
            return false;
        };
        let text = pdf_app::view::text_of_spans(&overlay.clusters, &spans);
        if text.is_empty() {
            return false;
        }
        ctx.copy_text(text);
        true
    }

    pub(crate) fn read_the_whole_page(&mut self) {
        let page = self.focus;
        let Some(overlay) = self.overlay(page) else {
            return;
        };
        if overlay.carets.is_empty() {
            return;
        }
        self.reading = Some(Reading {
            page,
            anchor: 0,
            caret: overlay.carets.len() - 1,
        });
    }

    fn picture_at(&self, page: usize, point: (f64, f64)) -> Option<usize> {
        self.overlay(page)?.objects.iter().rposition(|object| {
            matches!(object.kind, pdf_semantics::ObjectKind::Image)
                && point.0 >= object.box_pixels[0]
                && point.0 <= object.box_pixels[2]
                && point.1 >= object.box_pixels[1]
                && point.1 <= object.box_pixels[3]
        })
    }

    pub(crate) fn picture_menu(&mut self, ctx: &egui::Context) {
        let Some(menu) = self.picture_menu else {
            return;
        };
        let label = Message::Command(Command::SavePicture).say(self.lang);
        let mut save = false;
        let area = egui::Area::new(egui::Id::new("picture-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu.at)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    save = ui.button(label).clicked();
                });
            });
        let pressed_elsewhere = ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !area.response.rect.contains(at))
        });
        if save {
            self.picture_menu = None;
            self.save_picture(menu.page, menu.object);
        } else if pressed_elsewhere || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.picture_menu = None;
        }
    }

    fn save_picture(&mut self, page: usize, object: usize) {
        let Some(leaf) = self.editor.leaf(page).cloned() else {
            return;
        };
        let Some(found) = leaf.overlay.objects.get(object) else {
            return;
        };
        match picture_png(&leaf.view, found.box_pixels) {
            Ok(png) => {
                #[cfg(target_arch = "wasm32")]
                drop(png);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let folder = self.opened.parent().map(std::path::Path::to_path_buf);
                    self.chooser = Some(crate::chooser::Chooser::saving_as(
                        folder.as_deref(),
                        pdf_app::files::named_for_pages(&self.opened, &[page])
                            .trim_end_matches(".pdf"),
                        1,
                        "png",
                    ));
                    self.choosing_for = crate::page_actions::Choosing::PictureOut(png);
                }
            }
            Err(why) => self.editor.say(Message::Refused(why.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::picture_png;

    fn page_with_a_picture() -> pdf_bytes::ByteStore {
        let content = b"q 72 0 0 72 100 100 cm /Im1 Do Q";
        let samples = [255u8, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0];
        let mut pdf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        let mut object = |pdf: &mut Vec<u8>, body: &[u8]| {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n", offsets.len()).as_bytes());
            pdf.extend_from_slice(body);
            pdf.extend_from_slice(b"\nendobj\n");
        };
        object(&mut pdf, b"<< /Type /Catalog /Pages 2 0 R >>");
        object(&mut pdf, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        object(&mut pdf, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Contents 4 0 R /Resources << /XObject << /Im1 5 0 R >> >> >>");
        let mut body = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
        body.extend_from_slice(content);
        body.extend_from_slice(b"\nendstream");
        object(&mut pdf, &body);
        let mut body = format!("<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Length {} >>\nstream\n", samples.len()).into_bytes();
        body.extend_from_slice(&samples);
        body.extend_from_slice(b"\nendstream");
        object(&mut pdf, &body);
        let start = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).as_bytes(),
        );
        for offset in &offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
                offsets.len() + 1
            )
            .as_bytes(),
        );
        pdf_bytes::ByteStore::new(pdf_bytes::SourceId::new(95), Arc::<[u8]>::from(pdf))
    }

    #[test]
    fn a_picture_is_saved_as_the_page_shows_it_at_300_dpi() {
        let source = page_with_a_picture();
        let view = pdf_session::interpret_page_for_display(&source, 0, b"", None, None).unwrap();
        let png = picture_png(&view, [100.0, 128.0, 172.0, 200.0]).unwrap();
        let read = pdf_edit::image_file::ImageFile::read(&png).unwrap();
        let (across, down) = read.dpi().unwrap();
        assert!((across - 300.0).abs() < 1.0 && (down - 300.0).abs() < 1.0);
        let picture = read.thumbnail(1000).unwrap();
        assert_eq!((picture.width, picture.height), (300, 300));
        let middle = ((150 * 300 + 150) * 4) as usize;
        assert_eq!(&picture.rgba[middle..middle + 3], &[255, 0, 0]);
        assert!(picture_png(&view, [10.0, 10.0, 10.0, 20.0]).is_err());
    }
}
