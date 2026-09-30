//! How entries are shown and searched in the picker.

/// Separator shown in place of line breaks.
const LINE_BREAK: &str = " ⏎ ";
const ELLIPSIS: char = '…';

/// The text on a single line, cut after `max_chars` characters.
pub fn one_line(text: &str, max_chars: usize) -> String {
    let joined = text
        .trim()
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(LINE_BREAK);
    match char_boundary(&joined, max_chars) {
        Some(cut) => format!("{}{ELLIPSIS}", &joined[..cut]),
        None => joined,
    }
}

/// The first `max_chars` characters of `text`.
pub fn first_chars(text: &str, max_chars: usize) -> &str {
    match char_boundary(text, max_chars) {
        Some(cut) => &text[..cut],
        None => text,
    }
}

/// Case-insensitive search; an empty query matches everything.
pub fn matches(text: &str, query: &str) -> bool {
    text.to_lowercase().contains(&query.to_lowercase())
}

/// Byte index of the `n`-th character, if the text is longer than that.
fn char_boundary(text: &str, n: usize) -> Option<usize> {
    text.char_indices().nth(n).map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_are_shown_as_a_symbol() {
        assert_eq!(
            one_line("  first\n  second  \nthird\n", 100),
            "first ⏎ second ⏎ third"
        );
    }

    #[test]
    fn long_texts_are_cut_with_an_ellipsis() {
        assert_eq!(one_line("abcdef", 3), "abc…");
    }

    #[test]
    fn texts_at_the_limit_are_not_cut() {
        assert_eq!(one_line("abc", 3), "abc");
    }

    #[test]
    fn cutting_respects_multibyte_characters() {
        assert_eq!(one_line("éèêë", 2), "éè…");
        assert_eq!(first_chars("éèêë", 3), "éèê");
    }

    #[test]
    fn first_chars_keeps_short_texts_whole() {
        assert_eq!(first_chars("abc", 10), "abc");
    }

    #[test]
    fn search_ignores_case() {
        assert!(matches("Facture Janvier", "janv"));
        assert!(matches("ÉTÉ", "été"));
        assert!(!matches("Facture", "devis"));
    }

    #[test]
    fn empty_search_matches_everything() {
        assert!(matches("anything", ""));
    }
}
