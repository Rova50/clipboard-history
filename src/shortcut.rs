//! Installs or removes the Super+V shortcut on GNOME.

use std::path::PathBuf;

use gtk::gio;
use gtk::prelude::*;

const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const CUSTOM_KEYBINDING: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const SHELL_KEYBINDINGS: &str = "org.gnome.shell.keybindings";
const MESSAGE_TRAY_KEY: &str = "toggle-message-tray";
const CUSTOM_KEYBINDINGS_KEY: &str = "custom-keybindings";
const KEYBINDING_PATH: &str =
    "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/clipboard-history/";
const SHORTCUT: &str = "<Super>v";

type Result = std::result::Result<(), String>;

pub fn install() -> Result {
    ensure_gnome()?;
    free_shortcut_from_message_tray()?;
    register_keybinding()?;
    configure_keybinding()?;
    gio::Settings::sync();
    mark_installed()
}

pub fn remove() -> Result {
    if !gnome_available() {
        return Ok(());
    }
    unregister_keybinding()?;
    reset_keybinding();
    restore_message_tray();
    gio::Settings::sync();
    unmark_installed()
}

/// Called when the daemon starts: configures the shortcut the first time
/// only, so that a shortcut changed by the user is not overwritten.
pub fn install_once() {
    if marker().exists() || !gnome_available() {
        return;
    }
    if let Err(err) = install() {
        eprintln!("clipboard-history: raccourci non configuré : {err}");
    }
}

// ---------------------------------------------------------------- steps

fn ensure_gnome() -> Result {
    if gnome_available() {
        return Ok(());
    }
    Err(
        "bureau non-GNOME : associez Super+V à « clipboard-history show » \
         dans les réglages de votre bureau"
            .into(),
    )
}

/// GNOME opens the notification list with Super+V; Super+M still does.
fn free_shortcut_from_message_tray() -> Result {
    if !has_schema(SHELL_KEYBINDINGS) {
        return Ok(());
    }
    let shell = gio::Settings::new(SHELL_KEYBINDINGS);
    let keys = strings(&shell, MESSAGE_TRAY_KEY);
    let kept: Vec<String> = keys
        .into_iter()
        .filter(|k| !k.eq_ignore_ascii_case(SHORTCUT))
        .collect();
    shell
        .set_strv(MESSAGE_TRAY_KEY, kept)
        .map_err(|e| e.to_string())
}

fn restore_message_tray() {
    if has_schema(SHELL_KEYBINDINGS) {
        gio::Settings::new(SHELL_KEYBINDINGS).reset(MESSAGE_TRAY_KEY);
    }
}

fn register_keybinding() -> Result {
    let media_keys = gio::Settings::new(MEDIA_KEYS);
    let mut paths = strings(&media_keys, CUSTOM_KEYBINDINGS_KEY);
    if paths.iter().any(|path| path == KEYBINDING_PATH) {
        return Ok(());
    }
    paths.push(KEYBINDING_PATH.into());
    media_keys
        .set_strv(CUSTOM_KEYBINDINGS_KEY, paths)
        .map_err(|e| e.to_string())
}

fn unregister_keybinding() -> Result {
    let media_keys = gio::Settings::new(MEDIA_KEYS);
    let paths = strings(&media_keys, CUSTOM_KEYBINDINGS_KEY);
    let kept: Vec<String> = paths
        .into_iter()
        .filter(|path| path != KEYBINDING_PATH)
        .collect();
    media_keys
        .set_strv(CUSTOM_KEYBINDINGS_KEY, kept)
        .map_err(|e| e.to_string())
}

fn configure_keybinding() -> Result {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let binding = keybinding_settings();
    let command = format!("{} show", exe.display());
    for (key, value) in [
        ("name", "Clipboard History"),
        ("command", &command),
        ("binding", SHORTCUT),
    ] {
        binding.set_string(key, value).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn reset_keybinding() {
    let binding = keybinding_settings();
    for key in ["name", "command", "binding"] {
        binding.reset(key);
    }
}

fn mark_installed() -> Result {
    let marker = marker();
    let create = |path: &PathBuf| {
        std::fs::create_dir_all(path.parent().expect("marker has a parent directory"))?;
        std::fs::write(path, "")
    };
    create(&marker).map_err(|e| format!("impossible d'écrire {} : {e}", marker.display()))
}

fn unmark_installed() -> Result {
    let marker = marker();
    match std::fs::remove_file(&marker) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!(
            "impossible de supprimer {} : {e}",
            marker.display()
        )),
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------- helpers

/// File marking that the shortcut was configured once for this user.
fn marker() -> PathBuf {
    gtk::glib::user_config_dir()
        .join("clipboard-history")
        .join("shortcut-installed")
}

fn has_schema(id: &str) -> bool {
    gio::SettingsSchemaSource::default().is_some_and(|source| source.lookup(id, true).is_some())
}

fn gnome_available() -> bool {
    has_schema(MEDIA_KEYS) && has_schema(CUSTOM_KEYBINDING)
}

fn keybinding_settings() -> gio::Settings {
    gio::Settings::with_path(CUSTOM_KEYBINDING, KEYBINDING_PATH)
}

fn strings(settings: &gio::Settings, key: &str) -> Vec<String> {
    settings.strv(key).iter().map(ToString::to_string).collect()
}
