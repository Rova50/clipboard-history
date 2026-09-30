//! Saves the favorites on disk, the only entries kept across sessions.
//!
//! The file is readable by its owner only and replaced atomically, so that a
//! crash while saving never loses the previous favorites.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    /// The file exists but cannot be read: it must not be overwritten.
    Unreadable(String),
    /// The file was not valid and has been moved aside: saving is safe.
    MovedAside(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unreadable(message) | Self::MovedAside(message) => f.write_str(message),
        }
    }
}

pub struct FavoritesFile {
    path: PathBuf,
}

impl FavoritesFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `$XDG_DATA_HOME/clipboard-history/favorites.json`.
    pub fn in_data_dir(data_dir: &Path) -> Self {
        Self::new(data_dir.join("clipboard-history").join("favorites.json"))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The saved favorites; none if the file does not exist yet.
    ///
    /// An invalid file is moved aside (`.corrupt`) rather than overwritten
    /// by the next save.
    pub fn load(&self) -> Result<Vec<String>, LoadError> {
        let content = match fs::read_to_string(&self.path) {
            Ok(content) => content,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                let message = format!("lecture de {} : {err}", self.path.display());
                return Err(LoadError::Unreadable(message));
            }
        };
        serde_json::from_str(&content).map_err(|err| self.quarantine(&err.to_string()))
    }

    pub fn save(&self, favorites: &[String]) -> Result<(), String> {
        self.write(favorites)
            .map_err(|err| format!("écriture de {} : {err}", self.path.display()))
    }

    fn write(&self, favorites: &[String]) -> io::Result<()> {
        let dir = self.path.parent().expect("favorites file has a parent");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let temporary = self.path.with_extension("json.tmp");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(serde_json::to_string_pretty(favorites)?.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &self.path)
    }

    fn quarantine(&self, reason: &str) -> LoadError {
        let aside = self.path.with_extension("json.corrupt");
        match fs::rename(&self.path, &aside) {
            Ok(()) => LoadError::MovedAside(format!(
                "{} invalide ({reason}), mis de côté dans {}",
                self.path.display(),
                aside.display()
            )),
            Err(err) => LoadError::Unreadable(format!(
                "{} invalide ({reason}) et impossible à mettre de côté : {err}",
                self.path.display()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn file_in(dir: &tempfile::TempDir) -> FavoritesFile {
        FavoritesFile::in_data_dir(dir.path())
    }

    fn favorites(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| t.to_string()).collect()
    }

    #[test]
    fn no_file_means_no_favorites() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(file_in(&dir).load(), Ok(Vec::new()));
    }

    #[test]
    fn saved_favorites_are_loaded_back_identically() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        let saved = favorites(&["IBAN FR76…", "ligne 1\nligne 2", "guillemets \" et \\"]);
        file.save(&saved).unwrap();
        assert_eq!(file.load(), Ok(saved));
    }

    #[test]
    fn saving_again_replaces_the_previous_favorites() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        file.save(&favorites(&["a", "b"])).unwrap();
        file.save(&favorites(&["c"])).unwrap();
        assert_eq!(file.load(), Ok(favorites(&["c"])));
    }

    #[test]
    fn the_file_is_private_to_its_owner() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        file.save(&favorites(&["secret"])).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(file.path()), 0o600);
        assert_eq!(mode(file.path().parent().unwrap()), 0o700);
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_instead_of_being_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        file.save(&favorites(&["a"])).unwrap();
        fs::write(file.path(), "not json").unwrap();

        assert!(matches!(file.load(), Err(LoadError::MovedAside(_))));
        let aside = file.path().with_extension("json.corrupt");
        assert_eq!(fs::read_to_string(aside).unwrap(), "not json");
        assert_eq!(file.load(), Ok(Vec::new()));
    }

    #[test]
    fn an_unreadable_file_is_reported_as_such() {
        let dir = tempfile::tempdir().unwrap();
        let file = file_in(&dir);
        // A directory in place of the file cannot be read as text.
        fs::create_dir_all(file.path()).unwrap();
        assert!(matches!(file.load(), Err(LoadError::Unreadable(_))));
    }
}
