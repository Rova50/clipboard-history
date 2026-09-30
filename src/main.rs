mod app;
mod picker;
mod shortcut;

use clipboard_history::cli::{Command, USAGE};
use gtk::glib::ExitCode;

fn main() -> ExitCode {
    let arg = std::env::args().nth(1);
    match Command::parse(arg.as_deref()) {
        command if command.needs_daemon() => app::run(),
        Command::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("clipboard-history {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::InstallShortcut => report(shortcut::install(), "Raccourci Super+V configuré."),
        Command::RemoveShortcut => report(shortcut::remove(), "Raccourci retiré."),
        Command::Unknown(name) => report(Err(format!("commande inconnue : {name}\n\n{USAGE}")), ""),
        Command::Show | Command::Daemon | Command::List | Command::Clear => {
            unreachable!("handled by the daemon")
        }
    }
}

fn report(result: Result<(), String>, success: &str) -> ExitCode {
    match result {
        Ok(()) => {
            println!("{success}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("clipboard-history: {err}");
            ExitCode::FAILURE
        }
    }
}
