//! The background process: holds the history and opens the picker.
//!
//! Later invocations (`show`, `list`, `clear`) are forwarded by GApplication
//! to the process already running, which owns the history.

use std::cell::RefCell;
use std::ffi::{CString, c_char};
use std::rc::Rc;

use clipboard_history::cli::{Command, USAGE};
use clipboard_history::history::History;
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
";

#[derive(Default)]
pub struct State {
    pub history: History,
    pub picker: Option<gtk::ApplicationWindow>,
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
    install_css();
    record_copies(state.clone());
    shortcut::install_once();
}

fn install_css() {
    let Some(display) = gdk::Display::default() else { return };
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
            print_error(cmdline, &format!("Commande non gérée : {other:?}\n\n{USAGE}"));
            return glib::ExitCode::FAILURE;
        }
    }
    glib::ExitCode::SUCCESS
}

fn print_history(cmdline: &gio::ApplicationCommandLine, history: &History) {
    for (i, text) in history.entries().iter().enumerate() {
        print_output(cmdline, &format!("{:>3}  {text:?}\n", i + 1));
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
    print_remote(cmdline, message, gio::ffi::g_application_command_line_printerr);
}

fn print_remote(cmdline: &gio::ApplicationCommandLine, message: &str, printer: RemotePrinter) {
    use glib::translate::ToGlibPtr;
    let Ok(message) = CString::new(message) else { return };
    // SAFETY: "%s" consumes exactly one NUL-terminated string argument.
    unsafe { printer(cmdline.to_glib_none().0, c"%s".as_ptr(), message.as_ptr()) };
}
