/// Turns composition updates (each carrying the full in-progress text, e.g. from an IME or
/// voice recognizer) into the minimal edit to apply to what has already been typed.
///
/// Desktop platforms have no shared notion of "composition" we can drive remotely, so the
/// agent types the text for real and corrects it with backspaces as the phone revises it.
#[derive(Debug, Default)]
pub struct CompositionTracker {
    typed: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TextEdit {
    /// Characters to delete from the end of what was typed so far.
    pub backspaces: usize,
    /// Text to type after deleting.
    pub insert: String,
}

impl TextEdit {
    pub fn is_empty(&self) -> bool {
        self.backspaces == 0 && self.insert.is_empty()
    }
}

impl CompositionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self) {
        self.typed = Some(String::new());
    }

    pub fn is_active(&self) -> bool {
        self.typed.is_some()
    }

    /// The composition now reads `text`. An update without a preceding start begins one.
    pub fn update(&mut self, text: &str) -> TextEdit {
        let typed = self.typed.get_or_insert_with(String::new);
        let edit = diff(typed, text);
        *typed = text.to_string();
        edit
    }

    /// Finishes the composition. An empty `text` keeps whatever was last shown, since
    /// some senders commit without repeating the final text.
    pub fn commit(&mut self, text: &str) -> TextEdit {
        let edit = if text.is_empty() || !self.is_active() {
            if self.is_active() { TextEdit::default() } else { diff("", text) }
        } else {
            self.update(text)
        };
        self.typed = None;
        edit
    }

    /// Ends the composition, leaving the typed text in place (e.g. on disconnect).
    pub fn reset(&mut self) {
        self.typed = None;
    }
}

fn diff(old: &str, new: &str) -> TextEdit {
    let common_bytes: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a.len_utf8())
        .sum();
    TextEdit {
        backspaces: old[common_bytes..].chars().count(),
        insert: new[common_bytes..].to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(backspaces: usize, insert: &str) -> TextEdit {
        TextEdit { backspaces, insert: insert.to_string() }
    }

    #[test]
    fn growing_text_types_only_the_new_characters() {
        let mut t = CompositionTracker::new();
        t.start();
        assert_eq!(t.update("h"), edit(0, "h"));
        assert_eq!(t.update("he"), edit(0, "e"));
        assert_eq!(t.update("hel"), edit(0, "l"));
    }

    #[test]
    fn revised_text_backspaces_to_the_common_prefix() {
        let mut t = CompositionTracker::new();
        t.start();
        t.update("their");
        assert_eq!(t.update("there"), edit(2, "re"));
        assert_eq!(t.update(""), edit(5, ""));
    }

    #[test]
    fn counts_characters_not_bytes() {
        let mut t = CompositionTracker::new();
        t.start();
        t.update("café");
        assert_eq!(t.update("cafe"), edit(1, "e"));
        t.update("hi 👋");
        assert_eq!(t.update("hi"), edit(2, ""));
    }

    #[test]
    fn commit_with_final_text_applies_the_difference() {
        let mut t = CompositionTracker::new();
        t.start();
        t.update("helo");
        assert_eq!(t.commit("hello"), edit(1, "lo"));
        assert!(!t.is_active());
    }

    #[test]
    fn empty_commit_keeps_what_was_typed() {
        let mut t = CompositionTracker::new();
        t.start();
        t.update("hello");
        assert!(t.commit("").is_empty());
    }

    #[test]
    fn commit_without_composition_types_the_text() {
        let mut t = CompositionTracker::new();
        assert_eq!(t.commit("ok"), edit(0, "ok"));
    }

    #[test]
    fn next_composition_starts_fresh() {
        let mut t = CompositionTracker::new();
        t.start();
        t.update("one");
        t.commit("");
        t.start();
        assert_eq!(t.update("two"), edit(0, "two"));
    }
}
