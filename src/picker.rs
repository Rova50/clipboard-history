//! The window listing the history, opened with Super+V.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use clipboard_history::{paste, preview};
use gtk::prelude::*;
use gtk::{gdk, glib};

use crate::app::SharedState;

const PREVIEW_CHARS: usize = 120;
const TOOLTIP_CHARS: usize = 2000;
/// Lets the previous window get the focus back before Ctrl+V is simulated.
const PASTE_DELAY: Duration = Duration::from_millis(200);

/// What a key press does in the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Close,
    Next,
    Previous,
    Choose,
    Remove,
}

/// Maps a key press to an action; None lets the search field handle it.
fn action_for(key: gdk::Key, modifiers: gdk::ModifierType, searching: bool) -> Option<Action> {
    let super_held = modifiers.contains(gdk::ModifierType::SUPER_MASK);
    match key {
        gdk::Key::Escape => Some(Action::Close),
        gdk::Key::Down | gdk::Key::Tab => Some(Action::Next),
        gdk::Key::v | gdk::Key::V if super_held => Some(Action::Next),
        gdk::Key::Up | gdk::Key::ISO_Left_Tab => Some(Action::Previous),
        gdk::Key::Return | gdk::Key::KP_Enter => Some(Action::Choose),
        gdk::Key::Delete if !searching => Some(Action::Remove),
        _ => None,
    }
}

/// Opens the picker, or brings it to the front if it is already open.
pub fn show(app: &gtk::Application, state: &SharedState) {
    if let Some(window) = &state.borrow().picker {
        window.present();
        return;
    }
    let picker = Picker::new(app, state);
    picker.connect_signals();
    picker.open();
}

struct Picker {
    state: SharedState,
    window: gtk::ApplicationWindow,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    /// Snapshot of the history, indexed like the list rows.
    entries: RefCell<Vec<String>>,
}

impl Picker {
    fn new(app: &gtk::Application, state: &SharedState) -> Rc<Self> {
        let search = gtk::SearchEntry::builder().placeholder_text("Rechercher…").build();
        let list = build_list();
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let window = build_window(app, &search, &scroller);

        let entries = state.borrow().history.entries().to_vec();
        for (i, text) in entries.iter().enumerate() {
            list.append(&build_row(i, text));
        }
        Rc::new(Self {
            state: state.clone(),
            window,
            search,
            list,
            scroller,
            entries: RefCell::new(entries),
        })
    }

    fn connect_signals(self: &Rc<Self>) {
        self.connect_search();
        self.connect_choice();
        self.connect_keys();
        self.connect_closing();
    }

    fn open(&self) {
        self.state.borrow_mut().picker = Some(self.window.clone());
        self.select_first();
        self.window.present();
        self.search.grab_focus();
    }

    // ------------------------------------------------------------ signals

    fn connect_search(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.list.set_filter_func(move |row| {
            weak.upgrade().is_none_or(|picker| picker.row_matches_search(row))
        });
        let picker = self.clone();
        self.search.connect_search_changed(move |_| {
            picker.list.invalidate_filter();
            picker.select_first();
        });
    }

    fn connect_choice(self: &Rc<Self>) {
        let picker = self.clone();
        self.list.connect_row_activated(move |_, row| picker.choose(row));
    }

    fn connect_keys(self: &Rc<Self>) {
        let keys = gtk::EventControllerKey::new();
        // Before the search field, which would otherwise take arrows and Enter.
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let picker = self.clone();
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            let searching = !picker.search.text().is_empty();
            match action_for(key, modifiers, searching) {
                Some(action) => {
                    picker.perform(action);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.window.add_controller(keys);
    }

    fn connect_closing(&self) {
        // Clicking elsewhere closes the picker.
        self.window.connect_is_active_notify(|window| {
            if !window.is_active() {
                window.close();
            }
        });
        let state = self.state.clone();
        self.window.connect_close_request(move |_| {
            state.borrow_mut().picker = None;
            glib::Propagation::Proceed
        });
    }

    // ------------------------------------------------------------ actions

    fn perform(&self, action: Action) {
        match action {
            Action::Close => self.window.close(),
            Action::Next => self.select_relative(1),
            Action::Previous => self.select_relative(-1),
            Action::Choose => {
                if let Some(row) = self.selected_visible_row() {
                    self.choose(&row);
                }
            }
            Action::Remove => self.remove_selected(),
        }
    }

    fn choose(&self, row: &gtk::ListBoxRow) {
        let Some(text) = self.entry_of(row) else { return };
        WidgetExt::display(&self.window).clipboard().set_text(&text);
        self.state.borrow_mut().history.push(text);
        self.window.close();
        glib::timeout_add_local_once(PASTE_DELAY, paste::auto_paste);
    }

    fn remove_selected(&self) {
        let Some(row) = self.list.selected_row() else { return };
        let text = self.entries.borrow_mut().remove(row.index() as usize);
        self.state.borrow_mut().history.remove(&text);
        self.select_relative(1);
        self.list.remove(&row);
        if self.list.row_at_index(0).is_none() {
            self.window.close();
        }
    }

    // ------------------------------------------------------------ selection

    fn entry_of(&self, row: &gtk::ListBoxRow) -> Option<String> {
        self.entries.borrow().get(row.index() as usize).cloned()
    }

    fn row_matches_search(&self, row: &gtk::ListBoxRow) -> bool {
        let query = self.search.text();
        self.entries
            .borrow()
            .get(row.index() as usize)
            .is_none_or(|text| preview::matches(text, &query))
    }

    fn visible_rows(&self) -> Vec<gtk::ListBoxRow> {
        (0..)
            .map_while(|i| self.list.row_at_index(i))
            .filter(|row| row.is_child_visible())
            .collect()
    }

    fn selected_visible_row(&self) -> Option<gtk::ListBoxRow> {
        self.list.selected_row().filter(|row| row.is_child_visible())
    }

    fn select_first(&self) {
        if let Some(row) = self.visible_rows().first() {
            self.list.select_row(Some(row));
        }
    }

    /// Moves the selection by `step` visible rows, wrapping around.
    fn select_relative(&self, step: isize) {
        let rows = self.visible_rows();
        if rows.is_empty() {
            return;
        }
        let current = self.list.selected_row();
        let index = rows.iter().position(|row| Some(row) == current.as_ref());
        let index = index.map_or(-1, |i| i as isize);
        let row = &rows[(index + step).rem_euclid(rows.len() as isize) as usize];
        self.list.select_row(Some(row));
        self.scroll_to(row);
    }

    /// Keeps `row` in view while the focus stays in the search field.
    fn scroll_to(&self, row: &gtk::ListBoxRow) {
        let Some(bounds) = row.compute_bounds(&self.list) else { return };
        let adjustment = self.scroller.vadjustment();
        let top = f64::from(bounds.y());
        let bottom = f64::from(bounds.y() + bounds.height());
        if top < adjustment.value() {
            adjustment.set_value(top);
        } else if bottom > adjustment.value() + adjustment.page_size() {
            adjustment.set_value(bottom - adjustment.page_size());
        }
    }
}

// ---------------------------------------------------------------- widgets

fn build_window(
    app: &gtk::Application,
    search: &gtk::SearchEntry,
    scroller: &gtk::ScrolledWindow,
) -> gtk::ApplicationWindow {
    let hint = gtk::Label::new(Some("↑/↓ défiler · Entrée coller · Suppr retirer · Échap fermer"));
    hint.add_css_class("hint");

    let layout = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(10)
        .margin_end(10)
        .build();
    layout.append(search);
    layout.append(scroller);
    layout.append(&hint);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Historique du presse-papiers")
        .decorated(false)
        .resizable(false)
        .default_width(560)
        .default_height(420)
        .child(&layout)
        .build();
    window.add_css_class("clip-history");
    window
}

fn build_list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::Browse);
    list.set_activate_on_single_click(true);
    list.set_placeholder(Some(&gtk::Label::new(Some("Aucune copie dans cette session"))));
    list
}

fn build_row(index: usize, text: &str) -> gtk::ListBoxRow {
    let number = gtk::Label::new(Some(&format!("{:>2}", index + 1)));
    number.add_css_class("index");
    let label = gtk::Label::builder()
        .label(preview::one_line(text, PREVIEW_CHARS))
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    content.append(&number);
    content.append(&label);
    gtk::ListBoxRow::builder()
        .child(&content)
        .tooltip_text(preview::first_chars(text, TOOLTIP_CHARS))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: gdk::ModifierType = gdk::ModifierType::empty();

    #[test]
    fn arrows_and_tab_move_the_selection() {
        assert_eq!(action_for(gdk::Key::Down, NONE, false), Some(Action::Next));
        assert_eq!(action_for(gdk::Key::Tab, NONE, false), Some(Action::Next));
        assert_eq!(action_for(gdk::Key::Up, NONE, false), Some(Action::Previous));
        assert_eq!(action_for(gdk::Key::ISO_Left_Tab, NONE, false), Some(Action::Previous));
    }

    #[test]
    fn super_v_again_moves_to_the_next_entry() {
        let super_key = gdk::ModifierType::SUPER_MASK;
        assert_eq!(action_for(gdk::Key::v, super_key, false), Some(Action::Next));
    }

    #[test]
    fn plain_letters_go_to_the_search_field() {
        assert_eq!(action_for(gdk::Key::v, NONE, false), None);
    }

    #[test]
    fn delete_removes_an_entry_only_outside_a_search() {
        assert_eq!(action_for(gdk::Key::Delete, NONE, false), Some(Action::Remove));
        assert_eq!(action_for(gdk::Key::Delete, NONE, true), None);
    }

    #[test]
    fn enter_and_escape() {
        assert_eq!(action_for(gdk::Key::Return, NONE, true), Some(Action::Choose));
        assert_eq!(action_for(gdk::Key::KP_Enter, NONE, false), Some(Action::Choose));
        assert_eq!(action_for(gdk::Key::Escape, NONE, true), Some(Action::Close));
    }
}
