//! The clipboard history: favorites first, then the session's copies, most
//! recent first, without duplicates.

/// Texts larger than this are not kept (nor even transferred, when possible).
pub const MAX_ENTRY_BYTES: usize = 1024 * 1024;

/// Number of session copies kept by default; favorites do not count.
pub const DEFAULT_CAPACITY: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub text: String,
    pub pinned: bool,
}

pub struct History {
    /// Favorites, most recently pinned first. Kept across sessions.
    pinned: Vec<String>,
    /// Copies of the session, most recent first.
    recent: Vec<String>,
    capacity: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            pinned: Vec::new(),
            recent: Vec::new(),
            capacity,
        }
    }

    /// A history starting with the favorites saved by a previous session.
    pub fn with_favorites(capacity: usize, favorites: Vec<String>) -> Self {
        let mut history = Self::new(capacity);
        for text in favorites {
            if is_worth_keeping(&text) && !history.is_pinned(&text) {
                history.pinned.push(text);
            }
        }
        history
    }

    /// Puts `text` at the top of the session copies, moving it there if it
    /// was already present. A favorite copied again stays where it is.
    /// Returns false when the text is not worth keeping.
    pub fn push(&mut self, text: String) -> bool {
        if !is_worth_keeping(&text) {
            return false;
        }
        if self.is_pinned(&text) {
            return true;
        }
        self.recent.retain(|entry| *entry != text);
        self.recent.insert(0, text);
        self.recent.truncate(self.capacity);
        true
    }

    /// Makes `text` a favorite, or a plain copy again. Returns whether it is
    /// now pinned; unknown texts are left alone.
    pub fn toggle_pin(&mut self, text: &str) -> bool {
        if self.is_pinned(text) {
            self.pinned.retain(|entry| entry != text);
            self.recent.insert(0, text.to_string());
            self.recent.truncate(self.capacity);
            false
        } else if self.recent.iter().any(|entry| entry == text) {
            self.recent.retain(|entry| entry != text);
            self.pinned.insert(0, text.to_string());
            true
        } else {
            false
        }
    }

    pub fn is_pinned(&self, text: &str) -> bool {
        self.pinned.iter().any(|entry| entry == text)
    }

    /// Removes `text`, favorite or not.
    pub fn remove(&mut self, text: &str) {
        self.pinned.retain(|entry| entry != text);
        self.recent.retain(|entry| entry != text);
    }

    /// Forgets the session copies; favorites are kept.
    pub fn clear(&mut self) {
        self.recent.clear();
    }

    /// Favorites first, then the session copies.
    pub fn entries(&self) -> Vec<Entry> {
        let pinned = self.pinned.iter().map(|text| Entry {
            text: text.clone(),
            pinned: true,
        });
        let recent = self.recent.iter().map(|text| Entry {
            text: text.clone(),
            pinned: false,
        });
        pinned.chain(recent).collect()
    }

    /// The favorites, as they should be saved.
    pub fn favorites(&self) -> &[String] {
        &self.pinned
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

    fn texts(history: &History) -> Vec<String> {
        history.entries().into_iter().map(|e| e.text).collect()
    }

    fn pinned_texts(history: &History) -> Vec<String> {
        history
            .entries()
            .into_iter()
            .filter(|e| e.pinned)
            .map(|e| e.text)
            .collect()
    }

    // ------------------------------------------------------------ copies

    #[test]
    fn most_recent_copy_comes_first() {
        assert_eq!(texts(&history_of(&["a", "b", "c"])), ["c", "b", "a"]);
    }

    #[test]
    fn copying_again_moves_the_entry_to_the_top_without_duplicate() {
        assert_eq!(texts(&history_of(&["a", "b", "a"])), ["a", "b"]);
    }

    #[test]
    fn oldest_entries_are_dropped_beyond_capacity() {
        let mut history = History::new(2);
        for text in ["a", "b", "c"] {
            history.push(text.into());
        }
        assert_eq!(texts(&history), ["c", "b"]);
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
        assert_eq!(texts(&history), ["c", "a"]);
    }

    // ------------------------------------------------------------ favorites

    #[test]
    fn favorites_come_before_session_copies() {
        let mut history = history_of(&["a", "b", "c"]);
        history.toggle_pin("a");
        assert_eq!(texts(&history), ["a", "c", "b"]);
        assert_eq!(pinned_texts(&history), ["a"]);
    }

    #[test]
    fn latest_favorite_comes_first() {
        let mut history = history_of(&["a", "b"]);
        history.toggle_pin("a");
        history.toggle_pin("b");
        assert_eq!(pinned_texts(&history), ["b", "a"]);
    }

    #[test]
    fn unpinning_puts_the_entry_back_at_the_top_of_the_copies() {
        let mut history = history_of(&["a", "b", "c"]);
        history.toggle_pin("a");
        assert!(!history.toggle_pin("a"));
        assert_eq!(texts(&history), ["a", "c", "b"]);
        assert!(pinned_texts(&history).is_empty());
    }

    #[test]
    fn copying_a_favorite_again_does_not_duplicate_it() {
        let mut history = history_of(&["a", "b"]);
        history.toggle_pin("a");
        history.push("a".into());
        assert_eq!(texts(&history), ["a", "b"]);
    }

    #[test]
    fn favorites_do_not_count_in_the_capacity() {
        let mut history = History::new(2);
        history.push("fav".into());
        history.toggle_pin("fav");
        for text in ["a", "b", "c"] {
            history.push(text.into());
        }
        assert_eq!(texts(&history), ["fav", "c", "b"]);
    }

    #[test]
    fn clear_keeps_the_favorites() {
        let mut history = history_of(&["a", "b"]);
        history.toggle_pin("a");
        history.clear();
        assert_eq!(texts(&history), ["a"]);
    }

    #[test]
    fn remove_also_deletes_favorites() {
        let mut history = history_of(&["a"]);
        history.toggle_pin("a");
        history.remove("a");
        assert!(history.entries().is_empty());
        assert!(history.favorites().is_empty());
    }

    #[test]
    fn unknown_texts_cannot_be_pinned() {
        let mut history = history_of(&["a"]);
        assert!(!history.toggle_pin("z"));
        assert!(history.favorites().is_empty());
    }

    #[test]
    fn saved_favorites_are_restored_in_order_without_invalid_ones() {
        let saved = vec!["b".into(), " ".into(), "a".into(), "b".into()];
        let history = History::with_favorites(DEFAULT_CAPACITY, saved);
        assert_eq!(history.favorites(), ["b", "a"]);
    }
}
