//! Automatic paste: simulates Ctrl+V when a key-injection tool is installed.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    X11,
    Wayland,
}

impl Session {
    pub fn current() -> Self {
        match std::env::var("XDG_SESSION_TYPE").as_deref() {
            Ok("wayland") => Self::Wayland,
            _ => Self::X11,
        }
    }
}

/// ydotool works everywhere (kernel input); wtype only on wlroots Wayland
/// compositors; xdotool only reaches X11 windows.
const YDOTOOL: &[&str] = &["ydotool", "key", "29:1", "47:1", "47:0", "29:0"]; // LEFTCTRL, V
const WTYPE: &[&str] = &["wtype", "-M", "ctrl", "v", "-m", "ctrl"];
const XDOTOOL: &[&str] = &["xdotool", "key", "--clearmodifiers", "ctrl+v"];

/// The command simulating Ctrl+V in `session`, among the installed tools.
pub fn paste_command(
    session: Session,
    is_installed: impl Fn(&str) -> bool,
) -> Option<&'static [&'static str]> {
    let candidates: &[&'static [&'static str]] = match session {
        Session::Wayland => &[YDOTOOL, WTYPE],
        Session::X11 => &[YDOTOOL, XDOTOOL],
    };
    candidates.iter().copied().find(|command| is_installed(command[0]))
}

/// Simulates Ctrl+V in the focused window; does nothing without a tool.
pub fn auto_paste() {
    let Some(command) = paste_command(Session::current(), is_in_path) else {
        return;
    };
    if let Err(err) = Command::new(command[0])
        .args(&command[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        eprintln!("clipboard-history: collage automatique impossible ({}) : {err}", command[0]);
    }
}

fn is_in_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| Path::new(&dir).join(program).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(tools: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |program| tools.contains(&program)
    }

    #[test]
    fn ydotool_is_preferred_everywhere() {
        let all = installed(&["ydotool", "wtype", "xdotool"]);
        assert_eq!(paste_command(Session::Wayland, &all), Some(YDOTOOL));
        assert_eq!(paste_command(Session::X11, &all), Some(YDOTOOL));
    }

    #[test]
    fn wtype_is_used_on_wayland_only() {
        let tools = installed(&["wtype"]);
        assert_eq!(paste_command(Session::Wayland, &tools), Some(WTYPE));
        assert_eq!(paste_command(Session::X11, &tools), None);
    }

    #[test]
    fn xdotool_is_used_on_x11_only() {
        let tools = installed(&["xdotool"]);
        assert_eq!(paste_command(Session::X11, &tools), Some(XDOTOOL));
        assert_eq!(paste_command(Session::Wayland, &tools), None);
    }

    #[test]
    fn nothing_happens_without_a_tool() {
        assert_eq!(paste_command(Session::Wayland, installed(&[])), None);
    }
}
