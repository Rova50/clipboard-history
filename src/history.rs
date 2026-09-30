//! The session's clipboard history: most recent first, without duplicates.

/// Texts larger than this are not kept (nor even transferred, when possible).
pub const MAX_ENTRY_BYTES: usize = 1024 * 1024;

/// Number of entries kept by default.
pub const DEFAULT_CAPACITY: usize = 100;

pub struct History {
    entries: Vec<String>,
    capacity: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self { entries: Vec::new(), capacity }
    }

    /// Puts `text` at the top, moving it there if it was already present.
    /// Returns false when the text is not worth keeping.
    pub fn push(&mut self, text: String) -> bool {
        if !is_worth_keeping(&text) {
            return false;
        }
        self.remove(&text);
        self.entries.insert(0, text);
        self.entries.truncate(self.capacity);
        true
    }

    pub fn remove(&mut self, text: &str) {
        self.entries.retain(|entry| entry != text);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The entries, most recent first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

/// Blank texts and texts over [`MAX_ENTRY_BYTES`] are not kept.
pub fn is_worth_keeping(text: &str) -> bool {
    !text.trim().is_empty() && text.len() <= MAX_ENTRY_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_of(texts: &[&str]) -> History {
        let mut history = History::default();
        for text in texts {
            history.push(text.to_string());
        }
        history
    }

    #[test]
    fn most_recent_copy_comes_first() {
        let history = history_of(&["a", "b", "c"]);
        assert_eq!(history.entries(), ["c", "b", "a"]);
    }

    #[test]
    fn copying_again_moves_the_entry_to_the_top_without_duplicate() {
        let history = history_of(&["a", "b", "a"]);
        assert_eq!(history.entries(), ["a", "b"]);
    }

    #[test]
    fn oldest_entries_are_dropped_beyond_capacity() {
        let mut history = History::new(2);
        for text in ["a", "b", "c"] {
            history.push(text.into());
        }
        assert_eq!(history.entries(), ["c", "b"]);
    }

    #[test]
    fn blank_texts_are_ignored() {
        let mut history = History::default();
        assert!(!history.push(String::new()));
        assert!(!history.push(" \n\t ".into()));
        assert!(history.entries().is_empty());
    }

    #[test]
    fn texts_up_to_the_size_limit_are_kept() {
        let mut history = History::default();
        assert!(history.push("x".repeat(MAX_ENTRY_BYTES)));
        assert!(!history.push("x".repeat(MAX_ENTRY_BYTES + 1)));
        assert_eq!(history.entries().len(), 1);
    }

    #[test]
    fn size_limit_counts_bytes_not_characters() {
        let two_bytes_each = "é".repeat(MAX_ENTRY_BYTES / 2 + 1);
        assert!(!is_worth_keeping(&two_bytes_each));
    }

    #[test]
    fn remove_deletes_only_the_given_entry() {
        let mut history = history_of(&["a", "b", "c"]);
        history.remove("b");
        assert_eq!(history.entries(), ["c", "a"]);
    }

    #[test]
    fn clear_empties_the_history() {
        let mut history = history_of(&["a", "b"]);
        history.clear();
        assert!(history.entries().is_empty());
    }
}
