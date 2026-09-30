//! The background process: holds the history and opens the picker.
//!
//! Later invocations (`show`, `list`, `clear`) are forwarded by GApplication
//! to the process already running, which owns the history.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::rc::Rc;

use clipboard_history::cli::{Command, USAGE};
use clipboard_history::favorites::{FavoritesFile, LoadError};
use clipboard_history::history::{DEFAULT_CAPACITY, History};
use clipboard_history::x11_watch;
use gtk::prelude::*;
use gtk::{gdk, gio, glib};

use crate::{picker, shortcut};

const APP_ID: &str = "com.vatilab.ClipboardHistory";

const CSS: &str = "
window.clip-history { border-radius: 10px; }
.clip-history list row { padding: 6px 10px; }
.clip-history .index { opacity: 0.5; font-family: monospace; }
.clip-history .hint { opacity: 0.6; font-size: small; }
.clip-history .pin { min-height: 0; min-width: 0; padding: 2px; opacity: 0.35; }
.clip-history row:hover .pin, .clip-history .pinned .pin { opacity: 1; }
.clip-history .pinned .pin { color: @accent_color; }
";

#[derive(Default)]
pub struct State {
    pub history: History,
    pub picker: Option<gtk::ApplicationWindow>,
    /// None until loaded, or if the file must not be overwritten.
    favorites_file: Option<FavoritesFile>,
}

impl State {
    /// Restores the favorites of previous sessions.
    fn load_favorites(&mut self) {
        let file = FavoritesFile::in_data_dir(&glib::user_data_dir());
        let favorites = match file.load() {
            Ok(favorites) => favorites,
            Err(err @ LoadError::MovedAside(_)) => {
                eprintln!("clipboard-history: favoris : {err}");
                Vec::new()
            }
            Err(err @ LoadError::Unreadable(_)) => {
                eprintln!("clipboard-history: favoris non sauvegardés cette session : {err}");
                return;
            }
        };
        self.history = History::with_favorites(DEFAULT_CAPACITY, favorites);
        self.favorites_file = Some(file);
    }

    /// Pins or unpins `text`; returns whether it is now a favorite.
    pub fn toggle_pin(&mut self, text: &str) -> bool {
        let pinned = self.history.toggle_pin(text);
        self.save_favorites();
        pinned
    }

    pub fn remove(&mut self, text: &str) {
        let was_pinned = self.history.is_pinned(text);
        self.history.remove(text);
        if was_pinned {
            self.save_favorites();
        }
    }

    fn save_favorites(&self) {
        let Some(file) = &self.favorites_file else {
            return;
        };
        if let Err(err) = file.save(self.history.favorites()) {
            eprintln!("clipboard-history: {err}");
        }
    }
}

pub type SharedState = Rc<RefCell<State>>;

pub fn run() -> glib::ExitCode {
    configure_gtk_environment();
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state = SharedState::default();
    app.connect_startup({
        let state = state.clone();
        move |app| start_daemon(app, &state)
    });
    app.connect_command_line(move |app, cmdline| execute(app, cmdline, &state));
    app.run()
}

fn configure_gtk_environment() {
    // Run through XWayland like the watcher, so that the clipboard we set is
    // the one XWayland mirrors back to Wayland apps.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
        // SAFETY: called before any other thread exists.
        unsafe { std::env::set_var("GDK_BACKEND", "x11") };
    }
    // A plain list needs no GPU renderer, which would keep ~45 MB of GL state
    // in the daemon after the first picker.
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: called before any other thread exists.
        unsafe { std::env::set_var("GSK_RENDERER", "cairo") };
    }
}

fn start_daemon(app: &gtk::Application, state: &SharedState) {
    // Keep running without any window.
    std::mem::forget(app.hold());
    state.borrow_mut().load_favorites();
    install_css();
    record_copies(state.clone());
    shortcut::install_once();
}

fn install_css() {
    let Some(display) = gdk::Display::default() else {
        return;
    };
    let provider = gtk::CssProvider::new();
    provider.load_from_data(CSS);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn record_copies(state: SharedState) {
    let (sender, copies) = async_channel::unbounded();
    x11_watch::spawn(None, sender);
    glib::spawn_future_local(async move {
        while let Ok(text) = copies.recv().await {
            state.borrow_mut().history.push(text);
        }
    });
}

fn execute(
    app: &gtk::Application,
    cmdline: &gio::ApplicationCommandLine,
    state: &SharedState,
) -> glib::ExitCode {
    let args = cmdline.arguments();
    match Command::parse(args.get(1).and_then(|arg| arg.to_str())) {
        Command::Show => picker::show(app, state),
        Command::Daemon => {}
        Command::List => print_history(cmdline, &state.borrow().history),
        Command::Clear => state.borrow_mut().history.clear(),
        other => {
            print_error(
                cmdline,
                &format!("Commande non gérée : {other:?}\n\n{USAGE}"),
            );
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}

fn print_history(cmdline: &gio::ApplicationCommandLine, history: &History) {
    for (i, entry) in history.entries().iter().enumerate() {
        let star = if entry.pinned { "★" } else { " " };
        print_output(cmdline, &format!("{:>3} {star} {:?}\n", i + 1, entry.text));
    }
}

// The *_literal variants of these functions would require GLib 2.80.
type RemotePrinter =
    unsafe extern "C" fn(*mut gio::ffi::GApplicationCommandLine, *const c_char, ...);

/// Prints on the standard output of the process that sent the command.
fn print_output(cmdline: &gio::ApplicationCommandLine, message: &str) {
    print_remote(cmdline, message, gio::ffi::g_application_command_line_print);
}

/// Prints on the error output of the process that sent the command.
fn print_error(cmdline: &gio::ApplicationCommandLine, message: &str) {
    print_remote(
        cmdline,
        message,
        gio::ffi::g_application_command_line_printerr,
    );
}

fn print_remote(cmdline: &gio::ApplicationCommandLine, message: &str, printer: RemotePrinter) {
    use glib::translate::ToGlibPtr;
    let Ok(message) = CString::new(message) else {
        return;
    };
    // SAFETY: "%s" consumes exactly one NUL-terminated string argument.
    unsafe { printer(cmdline.to_glib_none().0, c"%s".as_ptr(), message.as_ptr()) };
}
