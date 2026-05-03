use std::ops::Range;

use gpui::*;
use unicode_width::UnicodeWidthStr;

pub(crate) trait TerminalImeView: Sized + 'static {
    fn ime_marked_text(&self) -> Option<&str>;
    fn ime_selected_range(&self) -> Option<Range<usize>>;
    fn set_ime_state(&mut self, marked_text: Option<String>, selected_range: Option<Range<usize>>);
    fn clear_ime_state(&mut self);
    fn send_ime_text(&mut self, text: &str, cx: &mut Context<Self>);
    fn prepare_ime_marked_text(&mut self, _marked_text: &str, _cx: &mut Context<Self>) {}
    fn ime_cursor_bounds(&self) -> Option<Bounds<Pixels>>;
}

pub(crate) struct TerminalImeInputHandler<V> {
    view: WeakEntity<V>,
}

impl<V> TerminalImeInputHandler<V> {
    pub(crate) fn new(view: WeakEntity<V>) -> Self {
        Self { view }
    }
}

fn selected_range_for_text(
    selected_range: Option<Range<usize>>,
    text: &str,
) -> Option<Range<usize>> {
    if text.is_empty() {
        return None;
    }

    let text_len = text.encode_utf16().count();
    let range = selected_range.unwrap_or(text_len..text_len);
    let start = range.start.min(text_len);
    let end = range.end.min(text_len);
    Some(start.min(end)..start.max(end))
}

fn prefix_cell_width(text: &str, utf16_index: usize) -> usize {
    let mut consumed_utf16 = 0;
    for (byte_index, ch) in text.char_indices() {
        let next_utf16 = consumed_utf16 + ch.len_utf16();
        if next_utf16 > utf16_index {
            return UnicodeWidthStr::width(&text[..byte_index]);
        }
        consumed_utf16 = next_utf16;
    }

    UnicodeWidthStr::width(text)
}

impl<V: TerminalImeView> InputHandler for TerminalImeInputHandler<V> {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<UTF16Selection> {
        let range = self
            .view
            .read_with(cx, |view, _| view.ime_selected_range())
            .ok()
            .flatten()
            .unwrap_or(0..0);
        Some(UTF16Selection {
            range,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.view
            .read_with(cx, |view, _| {
                view.ime_marked_text()
                    .map(|text| 0..text.encode_utf16().count())
            })
            .ok()
            .flatten()
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        *adjusted_range = Some(0..0);
        Some(String::new())
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.view.update(cx, |view, cx| {
            view.clear_ime_state();
            if !text.is_empty() {
                view.send_ime_text(text, cx);
                cx.notify();
            }
        });
        window.invalidate_character_coordinates();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.view.update(cx, |view, cx| {
            view.prepare_ime_marked_text(new_text, cx);
            view.set_ime_state(
                (!new_text.is_empty()).then(|| new_text.to_string()),
                selected_range_for_text(new_selected_range, new_text),
            );
            cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        let _ = self.view.update(cx, |view, cx| {
            view.clear_ime_state();
            cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.view
            .read_with(cx, |view, _| {
                let mut bounds = view.ime_cursor_bounds()?;
                let cell_width = bounds.size.width;
                let cell_offset = view
                    .ime_marked_text()
                    .map(|text| prefix_cell_width(text, range_utf16.start))
                    .unwrap_or(range_utf16.start);
                bounds.origin.x += cell_width * cell_offset as f32;
                Some(bounds)
            })
            .ok()
            .flatten()
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        Some(0)
    }

    fn prefers_ime_for_printable_keys(&mut self, _window: &mut Window, _cx: &mut App) -> bool {
        true
    }
}
