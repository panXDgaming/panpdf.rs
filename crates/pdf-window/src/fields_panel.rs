use eframe::egui;

use pdf_app::wording::Message;
use pdf_edit::form::{FieldKind, FormField};
use pdf_edit::tab_order::{TabOrder, in_reading_order};
use pdf_syntax::Reference;

use crate::dialog;
use crate::format::quiet_icon_button;
use crate::icons::Icon;
use crate::window_state::{ChosenFields, Tool, Window};

const PANEL_WIDTH: f32 = 230.0;

const fn kind_name(kind: FieldKind) -> Message {
    match kind {
        FieldKind::Text => Message::FieldText,
        FieldKind::Checkbox => Message::FieldCheckbox,
        FieldKind::Radio => Message::FieldRadio,
        FieldKind::Combo => Message::FieldDropdown,
        FieldKind::List => Message::FieldListBox,
        FieldKind::Push => Message::FieldButton,
        FieldKind::Signature => Message::FieldSignature,
    }
}

impl Window {
    pub(crate) fn fields_panel(&mut self, ui: &mut egui::Ui) {
        if self.tool != Tool::Form {
            return;
        }
        let lang = self.lang;
        let fields = self.editor.fields_in_document();
        let mut go_to = None;
        let mut order = None;
        let mut move_by = None;
        egui::Panel::right("fields panel")
            .resizable(false)
            .exact_size(PANEL_WIDTH)
            .show(ui, |ui| {
                ui.add_space(6.0);
                dialog::panel_caption(ui, &Message::FieldsPanel.say(lang));
                ui.add_space(4.0);
                if fields.is_empty() {
                    dialog::small(ui, &Message::NoFieldsYet.say(lang));
                    return;
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut page = usize::MAX;
                    for (at, (on, field)) in fields.iter().enumerate() {
                        let first = at == 0 || fields[at - 1].0 != *on;
                        let last = fields.get(at + 1).is_none_or(|(next, _)| next != on);
                        if *on != page {
                            page = *on;
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    Message::PageOf {
                                        page: page + 1,
                                        count: self.editor.page_count(),
                                    }
                                    .say(lang),
                                );
                                if ui
                                    .small_button(Message::OrderByRow.say(lang))
                                    .on_hover_text(Message::OrderByRowSaid.say(lang))
                                    .clicked()
                                {
                                    order = Some((page, false));
                                }
                                if ui
                                    .small_button(Message::OrderByColumn.say(lang))
                                    .on_hover_text(Message::OrderByColumnSaid.say(lang))
                                    .clicked()
                                {
                                    order = Some((page, true));
                                }
                            });
                            ui.separator();
                        }
                        let chosen = self.chosen_fields.as_ref().is_some_and(|held| {
                            held.page == page && held.widgets.contains(&field.widget)
                        });
                        ui.horizontal(|ui| {
                            let label = format!(
                                "{}. {}  ·  {}",
                                at + 1,
                                field.name,
                                kind_name(field.kind).say(lang)
                            );
                            if ui.selectable_label(chosen, label).clicked() {
                                go_to = Some((page, field.widget));
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let later = Message::FieldLater.say(lang);
                                    ui.add_enabled_ui(!last, |ui| {
                                        if quiet_icon_button(ui, Icon::Expand, &later).clicked() {
                                            move_by = Some((page, field.widget, 1_isize));
                                        }
                                    });
                                    let earlier = Message::FieldEarlier.say(lang);
                                    ui.add_enabled_ui(!first, |ui| {
                                        if quiet_icon_button(ui, Icon::Up, &earlier).clicked() {
                                            move_by = Some((page, field.widget, -1));
                                        }
                                    });
                                },
                            );
                        });
                    }
                });
            });
        if let Some((page, widget)) = go_to {
            self.goto(page);
            self.chosen_fields = Some(ChosenFields {
                page,
                widgets: vec![widget],
            });
        }
        if let Some((page, columns)) = order {
            self.order_the_fields(page, columns);
        }
        if let Some((page, widget, by)) = move_by {
            self.move_in_the_tab_order(page, widget, by);
        }
    }

    fn tabbed(&mut self, page: usize) -> Vec<(Reference, [f64; 4])> {
        self.editor
            .fields_in_document()
            .iter()
            .filter(|(on, _)| *on == page)
            .map(|(_, field): &(usize, FormField)| (field.widget, field.rect))
            .collect()
    }

    fn order_the_fields(&mut self, page: usize, columns: bool) {
        let fields = self.tabbed(page);
        let widgets = in_reading_order(&fields, columns);
        self.write_the_order(page, widgets, TabOrder::AsListed);
    }

    fn move_in_the_tab_order(&mut self, page: usize, widget: Reference, by: isize) {
        let mut widgets: Vec<Reference> = self
            .tabbed(page)
            .into_iter()
            .map(|(widget, _)| widget)
            .collect();
        if !moved_in_the_order(&mut widgets, widget, by) {
            return;
        }
        self.write_the_order(page, widgets, TabOrder::AsListed);
    }

    fn write_the_order(&mut self, page: usize, widgets: Vec<Reference>, order: TabOrder) {
        if widgets.is_empty() {
            return;
        }
        let job = self.editor.begin_set_tab_order(page, widgets, order);
        if job.is_none() {
            self.editor.say(Message::AnotherEditIsRunning);
            return;
        }
        self.send(job);
    }
}

fn moved_in_the_order(widgets: &mut [Reference], widget: Reference, by: isize) -> bool {
    let Some(at) = widgets.iter().position(|held| *held == widget) else {
        return false;
    };
    let to = at.saturating_add_signed(by);
    if to >= widgets.len() || to == at {
        return false;
    }
    widgets.swap(at, to);
    true
}

#[cfg(test)]
mod tests {
    use pdf_syntax::Reference;

    use super::moved_in_the_order;

    fn three() -> Vec<Reference> {
        (1..=3).map(|number| Reference::new(number, 0)).collect()
    }

    #[test]
    fn the_first_field_cannot_go_earlier_and_the_last_cannot_go_later() {
        let mut widgets = three();
        assert!(
            !moved_in_the_order(&mut widgets, Reference::new(1, 0), -1),
            "a move that changes nothing is not an edit"
        );
        assert!(!moved_in_the_order(&mut widgets, Reference::new(3, 0), 1));
        assert_eq!(widgets, three());
    }

    #[test]
    fn a_field_in_the_middle_swaps_with_its_neighbour() {
        let mut widgets = three();
        assert!(moved_in_the_order(&mut widgets, Reference::new(2, 0), 1));
        assert_eq!(
            widgets,
            [
                Reference::new(1, 0),
                Reference::new(3, 0),
                Reference::new(2, 0)
            ]
        );
        assert!(!moved_in_the_order(&mut widgets, Reference::new(9, 0), 1));
    }
}
