//! Command-line interface.

pub const USAGE: &str = "\
Historique des copies (Ctrl+C) de la session, ouvert avec Super+V.

Usage : clipboard-history [COMMANDE]

Commandes :
  show               ouvrir la liste (commande associée à Super+V, par défaut)
  --daemon           démarrer la surveillance en arrière-plan (au login)
  list               afficher l'historique dans le terminal
  clear              vider l'historique
  install-shortcut   associer Super+V (GNOME)
  remove-shortcut    retirer le raccourci et rendre Super+V aux notifications
  -h, --help         afficher cette aide
  -V, --version      afficher la version
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Show,
    Daemon,
    List,
    Clear,
    InstallShortcut,
    RemoveShortcut,
    Help,
    Version,
    Unknown(String),
}

impl Command {
    /// Parses the first argument; no argument means [`Command::Show`].
    pub fn parse(arg: Option<&str>) -> Self {
        match arg {
            None | Some("show") => Self::Show,
            Some("--daemon") => Self::Daemon,
            Some("list") => Self::List,
            Some("clear") => Self::Clear,
            Some("install-shortcut") => Self::InstallShortcut,
            Some("remove-shortcut") => Self::RemoveShortcut,
            Some("-h" | "--help") => Self::Help,
            Some("-V" | "--version") => Self::Version,
            Some(other) => Self::Unknown(other.to_string()),
        }
    }

    /// Commands carried out by the background process holding the history.
    pub fn needs_daemon(&self) -> bool {
        matches!(self, Self::Show | Self::Daemon | Self::List | Self::Clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_argument_opens_the_picker() {
        assert_eq!(Command::parse(None), Command::Show);
    }

    #[test]
    fn known_commands_are_recognised() {
        let cases = [
            ("show", Command::Show),
            ("--daemon", Command::Daemon),
            ("list", Command::List),
            ("clear", Command::Clear),
            ("install-shortcut", Command::InstallShortcut),
            ("remove-shortcut", Command::RemoveShortcut),
            ("-h", Command::Help),
            ("--help", Command::Help),
            ("-V", Command::Version),
            ("--version", Command::Version),
        ];
        for (arg, expected) in cases {
            assert_eq!(Command::parse(Some(arg)), expected, "argument {arg}");
        }
    }

    #[test]
    fn unknown_commands_are_reported_with_their_name() {
        assert_eq!(Command::parse(Some("paste")), Command::Unknown("paste".into()));
    }

    #[test]
    fn only_history_commands_go_to_the_daemon() {
        assert!(Command::Show.needs_daemon());
        assert!(Command::List.needs_daemon());
        assert!(!Command::InstallShortcut.needs_daemon());
        assert!(!Command::Help.needs_daemon());
        assert!(!Command::Unknown("x".into()).needs_daemon());
    }
}
