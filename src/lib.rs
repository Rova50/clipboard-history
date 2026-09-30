//! Clipboard history for Linux/Unix desktops (X11, and Wayland through XWayland).
//!
//! This library holds everything that does not need GTK, so that it can be
//! tested without a desktop. The binary adds the GTK picker on top.

pub mod cli;
pub mod favorites;
pub mod history;
pub mod paste;
pub mod preview;
pub mod x11_watch;
