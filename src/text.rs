use std::ops::Range;

use edict::{component::Component, entity::EntityId, world::WorldLocal};

use crate::{
    event::Key,
    trigger::{OnKey, OnPaste},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Glyph(pub u32);

/// Text is a component for a widget to display text.
#[derive(Component)]
pub struct Text {
    pub string: String,
}

impl Text {
    pub const fn new(string: String) -> Self {
        Text { string }
    }
}

/// Selection of a text-input widget. `selection` is a byte-offset range into
/// `string` (always on char boundaries); an empty range is a cursor with no selection.
#[derive(Debug, Clone, PartialEq, Eq, Component)]
pub struct TextInput {
    pub selection: Range<usize>,
}

impl TextInput {
    pub const fn new() -> Self {
        TextInput { selection: 0..0 }
    }
}

pub const fn make_text_input(string: String) -> (TextInput, Text) {
    let end = string.len();
    (
        TextInput {
            selection: end..end,
        },
        Text { string },
    )
}

/// Replaces the byte range `input.selection` with `insert`, collapsing the selection to a cursor
/// right after the inserted text.
fn replace_selection(input: &mut TextInput, text: &mut Text, insert: &str) {
    text.string.replace_range(input.selection.clone(), insert);
    let cursor = input.selection.start + insert.len();
    input.selection = cursor..cursor;
}

/// Returns the byte offset of the char boundary immediately before `cursor` in `string`.
fn prev_char_boundary(string: &str, cursor: usize) -> usize {
    match string[..cursor].chars().next_back() {
        Some(c) => cursor - c.len_utf8(),
        None => cursor,
    }
}

/// Returns the byte offset of the char boundary immediately after `cursor` in `string`.
fn next_char_boundary(string: &str, cursor: usize) -> usize {
    match string[cursor..].chars().next() {
        Some(c) => cursor + c.len_utf8(),
        None => cursor,
    }
}

fn backspace(input: &mut TextInput, text: &mut Text) {
    if !input.selection.is_empty() {
        replace_selection(input, text, "");
        return;
    }
    let cursor = input.selection.start;
    let new_cursor = prev_char_boundary(&text.string, cursor);
    text.string.replace_range(new_cursor..cursor, "");
    input.selection = new_cursor..new_cursor;
}

fn delete_forward(input: &mut TextInput, text: &mut Text) {
    if !input.selection.is_empty() {
        replace_selection(input, text, "");
        return;
    }
    let cursor = input.selection.start;
    let new_end = next_char_boundary(&text.string, cursor);
    text.string.replace_range(cursor..new_end, "");
    input.selection = cursor..cursor;
}

fn move_left(input: &mut TextInput, text: &mut Text) {
    if !input.selection.is_empty() {
        let cursor = input.selection.start;
        input.selection = cursor..cursor;
        return;
    }
    let cursor = prev_char_boundary(&text.string, input.selection.start);
    input.selection = cursor..cursor;
}

fn move_right(input: &mut TextInput, text: &mut Text) {
    if !input.selection.is_empty() {
        let cursor = input.selection.end;
        input.selection = cursor..cursor;
        return;
    }
    let cursor = next_char_boundary(&text.string, input.selection.end);
    input.selection = cursor..cursor;
}

fn move_home(input: &mut TextInput) {
    input.selection = 0..0;
}

fn move_end(input: &mut TextInput, text: &mut Text) {
    let end = text.string.len();
    input.selection = end..end;
}

/// Default [`OnKey`] handler: edits the widget's [`TextInput`] per `key`.
pub fn edit_on_key() -> OnKey {
    OnKey::invoke(|world: &WorldLocal, id, key| {
        let Ok(mut view) = world.try_view_one::<(&mut TextInput, &mut Text)>(id) else {
            return;
        };
        let Some((input, text)) = view.get_mut() else {
            return;
        };
        match key {
            Key::Char(c) => {
                let mut buf = [0u8; 4];
                replace_selection(input, text, c.encode_utf8(&mut buf));
            }
            Key::Backspace => backspace(input, text),
            Key::Delete => delete_forward(input, text),
            Key::ArrowLeft => move_left(input, text),
            Key::ArrowRight => move_right(input, text),
            Key::Home => move_home(input),
            Key::End => move_end(input, text),
            Key::Tab => {}
        }
    })
}

/// Default [`OnPaste`] handler: replaces the widget's [`TextInput`] selection
/// with the pasted text.
pub fn edit_on_paste() -> OnPaste {
    OnPaste::invoke(|world: &WorldLocal, id: EntityId, pasted: String| {
        let Ok(mut view) = world.try_view_one::<(&mut TextInput, &mut Text)>(id) else {
            return;
        };
        let Some((input, text)) = view.get_mut() else {
            return;
        };
        replace_selection(input, text, &*pasted);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_a_char_appends_and_moves_cursor() {
        let (mut input, mut text) = make_text_input("ab".to_string());
        replace_selection(&mut input, &mut text, "c");
        assert_eq!(text.string, "abc");
        assert_eq!(input.selection, 3..3);
    }

    #[test]
    fn typing_with_active_selection_replaces_it() {
        let mut input = TextInput { selection: 1..3 };
        let mut text = Text {
            string: "abcdef".to_string(),
        };
        replace_selection(&mut input, &mut text, "X");
        assert_eq!(text.string, "aXdef");
        assert_eq!(input.selection, 2..2);
    }

    #[test]
    fn backspace_at_cursor_removes_previous_char() {
        let (mut input, mut text) = make_text_input("abc".to_string());
        backspace(&mut input, &mut text);
        assert_eq!(text.string, "ab");
        assert_eq!(input.selection, 2..2);
    }

    #[test]
    fn backspace_with_active_selection_clears_selection_only() {
        let (mut input, mut text) = make_text_input("abcdef".to_string());
        input.selection = 1..3;
        backspace(&mut input, &mut text);
        assert_eq!(text.string, "adef");
        assert_eq!(input.selection, 1..1);
    }

    #[test]
    fn delete_forward_at_end_of_string_is_a_no_op() {
        let (mut input, mut text) = make_text_input("abc".to_string());
        delete_forward(&mut input, &mut text);
        assert_eq!(text.string, "abc");
        assert_eq!(input.selection, 3..3);
    }

    #[test]
    fn arrow_left_right_move_by_one_char_utf8_safe() {
        let (mut input, mut text) = make_text_input("aée".to_string());
        // Cursor starts at the end (byte len = 1 + 2 + 1 = 4).
        assert_eq!(text.string.len(), 4);
        assert_eq!(input.selection, 4..4);

        move_left(&mut input, &mut text);
        assert_eq!(input.selection, 3..3);
        move_left(&mut input, &mut text);
        assert_eq!(input.selection, 1..1);
        move_left(&mut input, &mut text);
        assert_eq!(input.selection, 0..0);

        move_right(&mut input, &mut text);
        assert_eq!(input.selection, 1..1);
        move_right(&mut input, &mut text);
        assert_eq!(input.selection, 3..3);
        move_right(&mut input, &mut text);
        assert_eq!(input.selection, 4..4);
    }

    #[test]
    fn home_and_end_jump_to_string_bounds() {
        let (mut input, mut text) = make_text_input("hello".to_string());
        input.selection = 2..2;
        move_home(&mut input);
        assert_eq!(input.selection, 0..0);
        move_end(&mut input, &mut text);
        assert_eq!(input.selection, 5..5);
    }

    #[test]
    fn paste_replaces_active_selection() {
        let (mut input, mut text) = make_text_input("hello world".to_string());
        input.selection = 6..11;
        replace_selection(&mut input, &mut text, "there");
        assert_eq!(text.string, "hello there");
        assert_eq!(input.selection, 11..11);
    }
}
