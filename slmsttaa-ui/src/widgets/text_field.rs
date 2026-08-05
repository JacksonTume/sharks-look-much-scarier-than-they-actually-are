//! A single-line text field: the widget the keyboard slice exists for.
//!
//! Every other widget in this crate reads a *level* — is the pointer down, is
//! this the hot id. This one is the first that has to read the host's key log in
//! **order**, because typing `ab` and then pressing Backspace is not the same
//! frame as pressing Backspace and then typing `ab`. That ordering requirement is
//! why [`UiInput::events`](crate::UiInput::events) is a slice rather than a set
//! of flags, and this file is the only place in the crate that would notice.
//!
//! # Bytes, not characters
//!
//! The caret and the selection anchor are **byte** offsets into the consumer's
//! `String`, always on a `char` boundary. Character *counts* would look tidier
//! and would be wrong the moment a name contains `ō` — every `String` operation
//! is byte-indexed, so the two silently disagree until an edit lands
//! mid-codepoint and panics. Movement goes through
//! [`prev_boundary`]/[`next_boundary`], never through `caret ± 1`.
//!
//! # It does not own a clipboard
//!
//! Copy and cut leave their text in [`UiState`](crate::UiState) for the host to
//! collect; paste never reaches this file at all, because a host delivers pasted
//! text as ordinary [`Event::Text`](crate::Event::Text) characters. A crate with
//! no dependencies has nothing to talk to an operating system *with*, and this is
//! what that constraint looks like when it is taken seriously rather than
//! worked around.

use crate::interact::TextState;
use crate::theme::Size;
use crate::{anim, font, Event, Key, Rect, Response, Ui};

/// An editable single-line text field, configured then shown.
///
/// Built by [`Ui::text_field`]. Nothing is drawn until [`TextField::show`] is
/// called, which is what the `must_use` is guarding.
///
/// ```
/// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
/// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
/// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
/// # let (mut name, mut filter) = (String::from("crate"), String::new());
/// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
/// // Edited in place; `changed` fires on every keystroke.
/// if ui.text_field("name", &mut name).show().changed { /* rename it */ }
///
/// // A filter box that also reports Enter separately.
/// if ui.text_field("filter", &mut filter)
///     .placeholder("type to filter")
///     .show()
///     .submitted
/// { /* commit the query */ }
/// # });
/// ```
#[must_use = "a text field draws nothing until `.show()` is called"]
pub struct TextField<'u, 'a, 'v> {
    ui: &'u mut Ui<'a>,
    label: &'u str,
    value: &'v mut String,
    placeholder: &'u str,
    size: Size,
}

impl<'a> Ui<'a> {
    /// Begin an editable single-line text field over `value`.
    ///
    /// `label` identifies the field and is **not drawn** — a field is usually
    /// captioned by the row above it or by its placeholder, and a widget that
    /// insisted on drawing its own label could not sit in a
    /// [`horizontal`](Ui::horizontal) row beside one. Pair it with
    /// [`Ui::label`] when you want a caption.
    ///
    /// Returns a [`TextField`] to configure; call [`TextField::show`] to draw it
    /// and get the [`Response`], whose `changed` fires on every keystroke and
    /// whose `submitted` fires on Enter.
    pub fn text_field<'u, 'v>(
        &'u mut self,
        label: &'u str,
        value: &'v mut String,
    ) -> TextField<'u, 'a, 'v> {
        TextField {
            ui: self,
            label,
            value,
            placeholder: "",
            size: Size::default(),
        }
    }
}

impl<'u, 'a, 'v> TextField<'u, 'a, 'v> {
    /// Muted text shown while the field is empty, in place of the contents.
    pub fn placeholder(mut self, placeholder: &'u str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// How large the field is drawn. Defaults to [`Size::Md`].
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Lay the field out, apply this frame's typing, draw it, and report what
    /// happened.
    pub fn show(self) -> Response {
        let TextField {
            ui,
            label,
            value,
            placeholder,
            size,
        } = self;
        let theme = *ui.theme();
        let (px, weight) = size.text(&theme).parts();

        let id = ui.next_id(label);
        ui.focusable(id);
        let face_h = size.face_height(&theme);
        // The row owns a 4-point trailing gap, exactly as a button's does, so a
        // field and a button on consecutive rows sit on the same rhythm.
        let row = ui.allocate([0.0, face_h + 4.0]);
        let face = Rect::new(row.x, row.y, row.w, face_h);
        // The run is inset by a gap on each side, and the caret needs somewhere
        // to stand at the very end, so the usable width is a point narrower.
        let pad = theme.space.gap;
        let inner = Rect::new(
            face.x + pad,
            face.y,
            (face.w - 2.0 * pad - CARET_W).max(0.0),
            face.h,
        );

        let mut response = ui.interact(face, id);
        let mut state = ui.state_for(id, value);

        if response.clicked {
            // A fresh click plants the caret and drops any selection; the drag
            // that may follow extends from there.
            let at = index_at(value, inner, state.scroll, ui.input().cursor, px, weight);
            state.collapse_to(at);
        } else if response.held {
            // Dragging *after* the press extends the selection, which is why the
            // anchor is left alone here.
            state.caret = index_at(value, inner, state.scroll, ui.input().cursor, px, weight);
        }

        if response.focused {
            // Claimed for as long as it holds focus rather than only on frames a
            // key arrives: a consumer reading *held* keys for a camera has to be
            // suppressed continuously, or holding `W` both types and flies.
            ui.capture_keyboard();
            let (edited, submitted) = apply_keys(ui, value, &mut state);
            response.submitted = submitted;
            if edited {
                response.changed = true;
                ui.mark_changed();
            }
        }

        state.clamp_to(value);
        state.scroll = scrolled_to_show_caret(value, &state, inner.w, px, weight);
        ui.set_state_for(id, state);

        draw(ui, face, inner, value, placeholder, &state, &response, size);
        response
    }
}

/// The caret's drawn width, in points. One point reads as a caret at every size
/// the type scale offers; two reads as a block.
const CARET_W: f32 = 1.0;

impl Ui<'_> {
    /// This field's caret and selection, clamped to the string as it is *now*.
    fn state_for(&self, id: u64, text: &str) -> TextState {
        self.state.text_state(id, text)
    }

    /// Remember this field's caret and selection.
    fn set_state_for(&mut self, id: u64, state: TextState) {
        self.state.set_text_state(id, state);
    }

    /// Hand `text` to the host to put on the system clipboard.
    fn offer_clipboard(&mut self, text: String) {
        self.state.clipboard = Some(text);
    }
}

/// Replay this frame's key log against `value`, returning whether anything was
/// edited and whether Enter was pressed.
///
/// One pass over the log **in order**, which is the entire point — see the module
/// docs.
fn apply_keys(ui: &mut Ui, value: &mut String, state: &mut TextState) -> (bool, bool) {
    let mut edited = false;
    let mut submitted = false;

    for event in ui.input().events {
        match event {
            Event::Text(ch) => {
                replace_selection(value, state, &ch.to_string());
                edited = true;
            }
            Event::Key(key) if key.pressed => {
                let shift = key.modifiers.shift;
                if key.modifiers.command() {
                    match key.key {
                        Key::A => {
                            state.anchor = 0;
                            state.caret = value.len();
                        }
                        // Copy and cut hand the text upward; there is nothing to
                        // hand it *to* from in here.
                        Key::C | Key::X => {
                            let (start, end) = state.selection();
                            if start != end {
                                ui.offer_clipboard(value[start..end].to_string());
                                if key.key == Key::X {
                                    delete_selection(value, state);
                                    edited = true;
                                }
                            }
                        }
                        // Paste is not handled here and never will be: a host
                        // delivers it as `Event::Text`, so it has already gone
                        // through the arm above by the time we see the `V`.
                        _ => {}
                    }
                    continue;
                }
                match key.key {
                    Key::Backspace => {
                        if state.has_selection() {
                            delete_selection(value, state);
                        } else if state.caret > 0 {
                            let from = prev_boundary(value, state.caret);
                            value.replace_range(from..state.caret, "");
                            state.collapse_to(from);
                        }
                        edited = true;
                    }
                    Key::Delete => {
                        if state.has_selection() {
                            delete_selection(value, state);
                        } else if state.caret < value.len() {
                            let to = next_boundary(value, state.caret);
                            value.replace_range(state.caret..to, "");
                        }
                        edited = true;
                    }
                    // A plain arrow with a selection collapses to that edge
                    // rather than stepping off it, which is what every editor
                    // does and what makes "select, then press Left" land where
                    // the eye expects.
                    Key::Left => {
                        let (start, _) = state.selection();
                        match (shift, state.has_selection()) {
                            (true, _) => state.caret = prev_boundary(value, state.caret),
                            (false, true) => state.collapse_to(start),
                            (false, false) => state.collapse_to(prev_boundary(value, state.caret)),
                        }
                    }
                    Key::Right => {
                        let (_, end) = state.selection();
                        match (shift, state.has_selection()) {
                            (true, _) => state.caret = next_boundary(value, state.caret),
                            (false, true) => state.collapse_to(end),
                            (false, false) => state.collapse_to(next_boundary(value, state.caret)),
                        }
                    }
                    Key::Home => move_to(state, 0, shift),
                    Key::End => move_to(state, value.len(), shift),
                    Key::Enter => submitted = true,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    (edited, submitted)
}

/// Move the caret to `at`, extending the selection if `shift` is down.
fn move_to(state: &mut TextState, at: usize, shift: bool) {
    state.caret = at;
    if !shift {
        state.anchor = at;
    }
}

/// Drop the selected range, leaving the caret where it was.
fn delete_selection(value: &mut String, state: &mut TextState) {
    let (start, end) = state.selection();
    value.replace_range(start..end, "");
    state.collapse_to(start);
}

/// Insert `text` at the caret, replacing any selection.
fn replace_selection(value: &mut String, state: &mut TextState, text: &str) {
    let (start, end) = state.selection();
    value.replace_range(start..end, text);
    state.collapse_to(start + text.len());
}

/// The `char` boundary immediately before `index`.
fn prev_boundary(text: &str, index: usize) -> usize {
    text[..index]
        .char_indices()
        .next_back()
        .map_or(0, |(at, _)| at)
}

/// The `char` boundary immediately after `index`.
fn next_boundary(text: &str, index: usize) -> usize {
    text[index..]
        .chars()
        .next()
        .map_or(index, |ch| index + ch.len_utf8())
}

/// Which byte offset the cursor is pointing at, for click-to-place and drag.
///
/// Rounds to the *nearest* boundary rather than the one before, so clicking the
/// right half of a glyph puts the caret after it — which is what makes clicking
/// at the end of a run land at the end rather than one character short.
fn index_at(
    text: &str,
    inner: Rect,
    scroll: f32,
    cursor: Option<(f32, f32)>,
    px: f32,
    weight: font::Weight,
) -> usize {
    let Some((x, _)) = cursor else {
        return text.len();
    };
    let target = x - inner.x + scroll;
    let mut pen = 0.0;
    for (at, ch) in text.char_indices() {
        let advance = font::advance(ch, px, weight);
        if target < pen + advance * 0.5 {
            return at;
        }
        pen += advance;
    }
    text.len()
}

/// How far the run has to be shifted left to keep the caret inside `width`.
///
/// Only ever moves as far as it must, so a caret walked back to the start
/// unwinds the scroll instead of leaving the text stranded off to the left.
fn scrolled_to_show_caret(
    text: &str,
    state: &TextState,
    width: f32,
    px: f32,
    weight: font::Weight,
) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    let caret = font::text_width(&text[..state.caret], px, weight);
    let total = font::text_width(text, px, weight);
    // Never scroll past the end: a field whose contents were just shortened
    // should show them, not the empty space they used to run into.
    let mut scroll = state.scroll.clamp(0.0, (total - width).max(0.0));
    // Then move only as far as the caret demands, in whichever direction it has
    // fallen out — so walking back to the start unwinds the scroll rather than
    // leaving the text stranded off to the left.
    if caret < scroll {
        scroll = caret;
    } else if caret > scroll + width {
        scroll = caret - width;
    }
    scroll.max(0.0)
}

/// Draw the well, the selection, the run, and the caret.
///
/// Split out for the same reason the slider's `draw_track` is: it is written
/// against nothing but [`Ui::painter`], so a consumer's own field can produce the
/// identical picture.
#[allow(clippy::too_many_arguments)]
fn draw(
    ui: &mut Ui,
    face: Rect,
    inner: Rect,
    value: &str,
    placeholder: &str,
    state: &TextState,
    response: &Response,
    size: Size,
) {
    let theme = *ui.theme();
    let (px, weight) = size.text(&theme).parts();
    let id = response.id;

    let hover = ui.animate(id, "hover", if response.hovered { 1.0 } else { 0.0 });
    let ring = ui.animate(id, "ring", if response.focused { 1.0 } else { 0.0 });

    let border = anim::lerp(theme.color.border, theme.color.accent_hover, hover);
    let painter = ui.painter();
    painter.fill_rect(face, theme.radius.md, theme.color.surface);
    painter.stroke_rect(face, theme.radius.md, theme.control.border, border);
    if ring > 0.0 {
        painter.stroke_rect(
            face,
            theme.radius.md,
            theme.control.ring,
            anim::fade(theme.color.ring, ring),
        );
    }

    // Everything below is clipped to the well, so a run longer than the field
    // scrolls under its own edges instead of over the panel.
    painter.push_clip(Rect::new(inner.x, face.y, face.max_x() - inner.x, face.h));
    let text_y = font::centered_top(face.y, face.h, px);
    let x = inner.x - state.scroll;

    if value.is_empty() {
        painter.text(x, text_y, placeholder, px, weight, theme.color.muted);
    } else {
        let (start, end) = state.selection();
        if start != end {
            let from = x + font::text_width(&value[..start], px, weight);
            let to = x + font::text_width(&value[..end], px, weight);
            // The band covers the cap band plus a little air, not the whole
            // row — a selection as tall as the control reads as a fill.
            let band = Rect::new(from, face.y + 3.0, to - from, face.h - 6.0);
            painter.fill_rect(band, theme.radius.sm, theme.color.selection);
        }
        painter.text(x, text_y, value, px, weight, theme.color.foreground);
    }

    if response.focused {
        let caret_x = x + font::text_width(&value[..state.caret], px, weight);
        painter.fill_rect(
            Rect::new(caret_x, face.y + 3.0, CARET_W, face.h - 6.0),
            0.0,
            theme.color.foreground,
        );
    }
    painter.pop_clip();
}
