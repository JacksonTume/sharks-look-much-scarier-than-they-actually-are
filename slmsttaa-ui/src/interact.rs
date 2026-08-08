//! Input, interaction state, and what a widget reports back.
//!
//! Three things live here, and the split between them is the point:
//!
//! - [`UiInput`] is *this frame*, handed in fresh every time.
//! - [`UiState`] is the little that must survive between frames — which widget
//!   is being dragged, which is focused, which sections are collapsed.
//! - [`Response`] is what every widget returns: where it ended up and what the
//!   pointer did to it.
//!
//! [`UiInput`] is also where the crate's independence is bought. The toolkit
//! could have read the engine's `Input` directly — but the engine depends on
//! *this* crate, so importing back would be a cycle. Instead this crate declares
//! the minimal snapshot it needs and the engine copies into it once per frame.
//! Three field assignments, and in exchange the toolkit has no dependencies at
//! all (see README § Dependency direction).
//!
//! It is deliberately narrow: one pointer, one button. Right and middle buttons
//! still aren't here, because nothing has needed them.
//!
//! # The keyboard, and why it is a *log*
//!
//! Everything else in [`UiInput`] is a **level** — what is true at the end of the
//! frame. A text field needs something a level cannot express: order. Typing
//! `ab` and then pressing Backspace inside one frame leaves `a`; the other order
//! leaves `ab`, and a set of flags has already thrown the difference away.
//!
//! So keyboard input arrives as an ordered `&[Event]` the host lends for the
//! frame. Borrowing rather than owning is what keeps [`UiInput`] [`Copy`] and
//! allocation-free — at the price of a lifetime parameter, which is the only
//! reason this type has one.

use crate::Rect;
use std::collections::HashMap;

/// A keyboard key, as the UI sees it.
///
/// **Physical** positions rather than layout-dependent labels, matching what a
/// host reports: [`Key::W`] is whichever key sits where `W` does on a US layout.
/// Bind shortcuts with this; never build text out of it — that is what
/// [`Event::Text`] is for, and the difference is every non-US keyboard.
///
/// This is declared here rather than imported because the toolkit imports
/// nothing (see the module docs above). A host with its own key enum writes one
/// `match` per frame, which is the same trade [`UiInput`] itself makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)] // Fifty single-letter variants; the enum's own docs say it.
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// The left arrow.
    Left,
    /// The right arrow.
    Right,
    /// Escape — cancels, and drops focus.
    Escape,
    /// Tab — moves focus to the next widget.
    Tab,
    /// Enter / Return — activates a focused control.
    Enter,
    /// The space bar — also activates a focused control, and types a space.
    Space,
    /// Backspace — deletes backward from the caret.
    Backspace,
    /// Delete — deletes forward from the caret.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
}

/// Which modifier keys were down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    /// Either Shift. Extends a selection rather than moving the caret.
    pub shift: bool,
    /// Either Ctrl.
    pub ctrl: bool,
    /// Either Alt (Option on macOS).
    pub alt: bool,
    /// The platform "logo" key — Windows, Command, or Super.
    pub logo: bool,
}

impl Modifiers {
    /// Whether the platform's **shortcut** modifier is down: Command on macOS,
    /// Ctrl everywhere else. Bind `Ctrl+C`-shaped shortcuts through this.
    pub fn command(&self) -> bool {
        if cfg!(target_os = "macos") {
            self.logo
        } else {
            self.ctrl
        }
    }

    /// Whether no modifier at all is down — the guard an unmodified shortcut
    /// wants so it doesn't also fire under Ctrl.
    pub fn none(&self) -> bool {
        !self.shift && !self.ctrl && !self.alt && !self.logo
    }
}

/// One keyboard transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    /// Which key moved.
    pub key: Key,
    /// `true` on the way down, `false` on the way up.
    pub pressed: bool,
    /// Whether this is the operating system's auto-repeat rather than a fresh
    /// press. A text field honors repeats — holding Backspace should keep
    /// deleting — where a one-shot shortcut ignores them.
    pub repeat: bool,
    /// The modifiers that were down at the moment of the transition.
    pub modifiers: Modifiers,
}

/// One entry in the host's ordered per-frame keyboard log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A key went down or came up.
    Key(KeyEvent),
    /// A character was **typed**, with the layout, shift state and any dead-key
    /// composition already applied by the platform.
    ///
    /// Control characters never appear here, so this is always something the
    /// font can draw. A host that supports pasting delivers the pasted text as a
    /// run of these, which is why nothing in this crate knows what a clipboard
    /// is — see [`UiState::take_clipboard`] for the other direction.
    ///
    /// **A host must not report a character produced under a shortcut
    /// modifier.** Ctrl+A is a command, not the letter `a` — and some platforms
    /// will hand it to you as though it were both. The check belongs on the host
    /// side because the exception does too: Ctrl+Alt is AltGr on a European
    /// layout, and it types real characters.
    Text(char),
}

/// One frame of host state, as the UI sees it: where the pointer is, what the
/// keyboard did, and how big the window is.
///
/// The host fills this in each frame. Coordinates are **logical points** with
/// the origin at the top-left, matching what a [`Painter`](crate::Painter) draws
/// in — a host on a HiDPI display divides physical cursor coordinates by the
/// scale factor before filling this in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UiInput<'a> {
    /// Where the pointer is, or `None` if it hasn't been seen yet (or has left
    /// the window). Hit-testing against `None` never hits.
    pub cursor: Option<(f32, f32)>,
    /// Whether the primary (left) button is *currently down*. Drives drags.
    pub primary_held: bool,
    /// Whether the primary button went down *this frame* — a press edge, true
    /// for exactly one frame. Drives clicks.
    pub primary_pressed: bool,
    /// Wheel movement this frame, in notches; positive is scrolling up.
    ///
    /// Only a scroll area reads it. A host that also uses the wheel for
    /// something else (the terrain demo zooms its camera with it) decides who
    /// wins by checking [`Ui::wants_pointer`](crate::Ui::wants_pointer).
    pub scroll_delta: f32,
    /// The drawable area's `(width, height)` in points.
    ///
    /// Only edge anchoring reads it: a `TopRight` panel has to know where the
    /// right edge *is*. A host that only ever puts panels in the top-left corner
    /// can leave it at `(0.0, 0.0)` and nothing will notice.
    pub viewport: (f32, f32),
    /// Seconds elapsed since the previous frame, for
    /// [`Ui::animate`](crate::Ui::animate).
    ///
    /// The toolkit owns no clock — this is the whole of what it knows about time,
    /// and it is a duration rather than a timestamp on purpose: a timestamp would
    /// invite something here to reason about *when* things happened, and nothing
    /// needs to.
    ///
    /// Leaving it at `0.0` is a supported state, not a broken one: every value
    /// snaps straight to its target and the UI behaves exactly as it did before
    /// animation existed. That is what every test in this crate does, and it is
    /// why they did not have to learn about frames.
    pub dt: f32,
    /// This frame's keyboard events, **in the order they arrived**.
    ///
    /// Borrowed for the frame, which is what keeps this struct [`Copy`]. An
    /// empty slice is a supported state and is what [`Default`] gives you, so a
    /// host with no keyboard — or a test that doesn't care — leaves it alone and
    /// every widget behaves as it did before keys existed.
    pub events: &'a [Event],
    /// Which modifiers are down *now*, as opposed to when a given event fired.
    ///
    /// Read it for pointer gestures that a modifier changes — shift-clicking a
    /// row to extend a selection. Key handling should prefer the copy riding on
    /// [`KeyEvent::modifiers`], which cannot have gone stale.
    pub modifiers: Modifiers,
}

impl UiInput<'_> {
    /// Whether the pointer is inside the given rectangle this frame.
    pub(crate) fn hits(&self, rect: Rect) -> bool {
        self.cursor.is_some_and(|p| rect.contains(p))
    }

    /// Whether `key` went down this frame, **auto-repeat included**.
    ///
    /// Repeats count because the things this answers for — moving a caret,
    /// walking a list, nudging a slider — are all things a held key should keep
    /// doing. A widget that wants one-shot semantics filters
    /// [`KeyEvent::repeat`] out of [`UiInput::events`] itself.
    pub fn key_pressed(&self, key: Key) -> bool {
        self.events
            .iter()
            .any(|event| matches!(event, Event::Key(k) if k.key == key && k.pressed))
    }

    /// The presses in this frame's log, in order, releases filtered out.
    pub fn key_presses(&self) -> impl Iterator<Item = KeyEvent> + '_ {
        self.events.iter().filter_map(|event| match event {
            Event::Key(k) if k.pressed => Some(*k),
            _ => None,
        })
    }
}

/// What a widget reports back: where it landed, and what the pointer did to it.
///
/// Every widget returns one of these, including the ones with nothing to say
/// (a label reports its rectangle and `hovered`, and that is genuinely useful —
/// it is how a consumer hangs a tooltip on one).
///
/// The interesting fields are deliberately plain `bool`s rather than an event
/// enum: immediate mode means the consumer is already inside an `if`, and
/// `if ui.button("go").clicked` is the whole point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Response {
    /// The widget's stable id this frame.
    pub id: u64,
    /// Where the widget's interactive area ended up.
    pub rect: Rect,
    /// The pointer is over it.
    pub hovered: bool,
    /// The pointer is over it *and* the primary button is down.
    pub held: bool,
    /// The primary button went down on it this frame — or, for a focusable
    /// control, Enter or Space was pressed while it held focus. A keyboard
    /// activation is a click as far as a caller is concerned, which is what
    /// makes every existing `if …clicked` call site keyboard-operable for free.
    pub clicked: bool,
    /// It holds focus — it was the last thing clicked, or Tab walked onto it.
    /// Widgets draw a focus ring from this, and keyboard input routes by it.
    pub focused: bool,
    /// The widget edited its bound value this frame. Always `false` for widgets
    /// that don't bind one.
    pub changed: bool,
    /// The user pressed Enter to commit — a search box submitting its query.
    ///
    /// Only [`Ui::text_field`](crate::Ui::text_field) ever reports `true`, the
    /// same way `open` is only ever meaningful for a section. It is distinct
    /// from `changed`, which fires on every keystroke: a filter box wants
    /// `changed`, a form field wants this.
    pub submitted: bool,
    /// Whether this widget's contents are expanded.
    ///
    /// Only [`Ui::section`](crate::Ui::section) — the one collapsible widget —
    /// ever reports `false`, and it is **informational**: a section takes its
    /// contents as a closure and decides for itself what to show, so nothing has
    /// to branch on this. Read it to mirror the state somewhere else (a caret in
    /// a status line, a preference to persist).
    pub open: bool,
}

/// Persistent UI state that survives between frames.
///
/// Immediate-mode UIs need almost no retained state; this is all of it. The host
/// owns one and lends it to each [`Ui`](crate::Ui) frame.
///
/// The three interaction slots are the classic immediate-mode trio:
///
/// - **hot** — what the pointer is over *right now*. Recomputed every frame.
/// - **active** — what has captured the pointer (a slider mid-drag). Survives
///   the cursor wandering off the widget, and is what makes dragging work.
/// - **focused** — what receives the keyboard. Set by a click, moved by Tab, and
///   read by every keyboard-operable widget.
#[derive(Debug, Default)]
pub struct UiState {
    /// The widget the pointer is over this frame.
    pub(crate) hot: Option<u64>,
    /// The widget currently capturing the pointer (a slider being dragged), so
    /// the drag continues even if the cursor leaves the track.
    pub(crate) active: Option<u64>,
    /// The widget that would receive keyboard input.
    pub(crate) focused: Option<u64>,
    /// Every id that called [`Ui::focusable`](crate::Ui::focusable) this frame,
    /// in declaration order — the ring Tab and Shift-Tab walk.
    ///
    /// **This is the one place position is load-bearing**, and it is not the
    /// order-keyed-id bug returning: ids are still `hash(scope, label)`, so a row
    /// appearing above a widget cannot change its identity. All that shifts is
    /// where it sits in the tab ring, which is what a tab ring *is*.
    ///
    /// Cleared at the top of each frame, after [`Ui::new`](crate::Ui::new) has
    /// resolved this frame's Tab against the *previous* frame's ring. Reading
    /// last frame's order is what lets the newly focused widget draw its ring on
    /// the same frame the key was pressed, rather than one later.
    pub(crate) focus_order: Vec<u64>,
    /// Caret, selection and horizontal scroll per text field id.
    pub(crate) text: Vec<(u64, TextState)>,
    /// Text a widget has asked the host to put on the system clipboard.
    ///
    /// The toolkit has no clipboard and never will — it has no dependencies to
    /// have one *with*. This is the outbound half of the seam, drained by the
    /// host through [`UiState::take_clipboard`]; inbound, a paste simply arrives
    /// as [`Event::Text`] characters like any other typing.
    pub(crate) clipboard: Option<String>,
    /// Where each panel ended up last frame, keyed by panel id.
    ///
    /// A bottom-anchored panel is the reason this exists: it has to position its
    /// contents *before* laying them out, so it borrows last frame's height and
    /// settles on the second frame. Top-anchored panels never read it.
    pub(crate) panels: Vec<(u64, Rect)>,
    /// Collapsed/expanded state per section id.
    ///
    /// A `Vec` rather than a `HashMap`: a panel has a handful of sections, a
    /// linear scan over them beats hashing, and it keeps this crate's state
    /// trivially `Debug`-able. Revisit if a consumer ever has hundreds.
    pub(crate) open: Vec<(u64, bool)>,
    /// How far each scroll area is scrolled, in points from its top.
    ///
    /// This is the *target*, moved in whole notches by the wheel. What is drawn
    /// eases toward it through [`UiState::animate`].
    pub(crate) scroll: Vec<(u64, f32)>,
    /// How tall a container's contents measured, the last time they were laid
    /// out — keyed by the container's id.
    ///
    /// Two containers need it and for the same reason: they have to decide how
    /// much of their contents to show *before* laying them out, so they borrow
    /// last frame's measurement. A scroll area sizes its viewport from it; a
    /// section clips a collapse animation to a fraction of it.
    pub(crate) measured: Vec<(u64, f32)>,
    /// The row a virtualized scroll area was last asked to reveal, keyed by the
    /// area's id.
    ///
    /// Stored so revealing can be **edge-triggered**. A consumer passes the row it
    /// wants visible on every frame that row is selected, not on the one frame it
    /// became selected — so acting on the value itself would drag the view back to
    /// the selection the instant the wheel moved away from it. Acting on a
    /// *change* is the same rule [`Ui::focus_moved`](crate::Ui) already applies to
    /// the focus chase, which exists for exactly this reason.
    pub(crate) revealed: Vec<(u64, usize)>,
    /// One eased float per `(widget, property)` pair — see [`anim`](crate::anim).
    ///
    /// Swept every frame (see [`UiState::begin_frame`]), which is the difference
    /// between this and the maps above. Those are keyed by containers, of which a
    /// panel has a handful; this one gets an entry per *animated property of
    /// every widget declared*, so a consumer that generates rows from changing
    /// labels would grow it without bound if nothing pruned it.
    ///
    /// And a real map, for the same reason: a hundred widgets never noticed the
    /// linear scan, and a few thousand rows each easing a hover made it the single
    /// largest cost in the frame — 6.5 ms at five thousand, measured, more than
    /// the layout it was decorating.
    pub(crate) anim: HashMap<u64, Animated>,
}

/// Where a text field's caret is, what it has selected, and how far its contents
/// have been scrolled sideways.
///
/// `caret` and `anchor` are **byte** offsets into the consumer's string, always
/// on a `char` boundary. Bytes rather than character counts because every
/// operation on a `String` is byte-indexed, and a name like `Ōsawa` makes the two
/// disagree — which is the bug where a caret lands mid-codepoint and the next
/// edit panics.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct TextState {
    /// Where the caret sits, and the moving end of a selection.
    pub(crate) caret: usize,
    /// The fixed end of a selection. Equal to `caret` when nothing is selected.
    pub(crate) anchor: usize,
    /// How far the run is shifted left, in points, to keep the caret in view.
    pub(crate) scroll: f32,
}

impl TextState {
    /// The selection as an ordered byte range; empty when there is none.
    pub(crate) fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    /// Whether anything is selected.
    pub(crate) fn has_selection(&self) -> bool {
        self.caret != self.anchor
    }

    /// Put both ends at `at`, collapsing any selection.
    pub(crate) fn collapse_to(&mut self, at: usize) {
        self.caret = at;
        self.anchor = at;
    }

    /// Pull the caret and anchor back onto valid `char` boundaries of `text`.
    ///
    /// The consumer owns the string and may have replaced it between frames —
    /// selecting a different object refills the same field — so neither offset
    /// can be trusted without this.
    pub(crate) fn clamp_to(&mut self, text: &str) {
        self.caret = clamp_boundary(text, self.caret);
        self.anchor = clamp_boundary(text, self.anchor);
    }
}

/// The nearest `char` boundary of `text` at or before `index`.
fn clamp_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// One eased float, and whether anything asked for it this frame.
///
/// Stored against its `hash(widget id, property name)` key rather than carrying
/// it, so the lookup is a hash rather than a walk.
#[derive(Debug)]
pub(crate) struct Animated {
    /// Where the value is right now.
    pub(crate) value: f32,
    /// Set by [`UiState::animate`], cleared by [`UiState::begin_frame`]. An entry
    /// that survives a whole frame without being asked for belonged to a widget
    /// that is no longer declared, and is dropped.
    pub(crate) alive: bool,
}

impl UiState {
    /// Take whatever a widget has asked to put on the system clipboard, if
    /// anything, leaving the slot empty.
    ///
    /// Call it from the host once per frame, after the UI has been declared. On
    /// a host with no clipboard, never calling it is fine — the slot simply holds
    /// the most recent request and is overwritten by the next.
    ///
    /// There is no inbound counterpart on purpose: a paste is just typing, so a
    /// host delivers it as [`Event::Text`] characters and this crate stays
    /// unaware that clipboards exist.
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    /// Move focus one step around last frame's tab ring.
    ///
    /// `forward` walks declaration order, `!forward` walks it backwards; both
    /// wrap. Focus that is currently nowhere — or on something no longer
    /// declared — enters at whichever end the direction implies, so the first Tab
    /// after a click into empty space lands on the first control rather than on
    /// nothing.
    pub(crate) fn step_focus(&mut self, forward: bool) {
        if self.focus_order.is_empty() {
            return;
        }
        let last = self.focus_order.len() - 1;
        let current = self
            .focused
            .and_then(|id| self.focus_order.iter().position(|&other| other == id));
        let next = match (current, forward) {
            (Some(i), true) => {
                if i == last {
                    0
                } else {
                    i + 1
                }
            }
            (Some(i), false) => {
                if i == 0 {
                    last
                } else {
                    i - 1
                }
            }
            (None, true) => 0,
            (None, false) => last,
        };
        self.focused = Some(self.focus_order[next]);
    }

    /// `id`'s caret and selection, clamped to `text`.
    pub(crate) fn text_state(&self, id: u64, text: &str) -> TextState {
        let mut state = self
            .text
            .iter()
            .find(|(k, _)| *k == id)
            .map_or(TextState::default(), |(_, v)| *v);
        state.clamp_to(text);
        state
    }

    /// Remember `id`'s caret and selection.
    pub(crate) fn set_text_state(&mut self, id: u64, value: TextState) {
        match self.text.iter_mut().find(|(k, _)| *k == id) {
            Some((_, v)) => *v = value,
            None => self.text.push((id, value)),
        }
    }

    /// Retire animated values nothing asked for last frame, and re-arm the
    /// survivors to be asked for again.
    ///
    /// Called once per frame by [`Ui::new`](crate::Ui::new). A widget inside a
    /// collapsed section stops being declared and so loses its slots — which is
    /// correct rather than merely acceptable: it is invisible, and when it comes
    /// back it should come back settled, not mid-fade from a hover the user has
    /// long since forgotten.
    pub(crate) fn begin_frame(&mut self) {
        self.anim.retain(|_, slot| slot.alive);
        for slot in self.anim.values_mut() {
            slot.alive = false;
        }
    }

    /// Step `key`'s eased value toward `target` and return where it now is.
    ///
    /// A key seen for the first time is seeded **at** its target rather than at
    /// zero. Otherwise every widget would fade in from "not hovered" on the frame
    /// it first appears — a panel would ripple on open, and a section's contents
    /// would fade every time it was expanded.
    pub(crate) fn animate(&mut self, key: u64, target: f32, rate: f32, dt: f32) -> f32 {
        match self.anim.get_mut(&key) {
            Some(slot) => {
                slot.alive = true;
                slot.value = crate::anim::approach(slot.value, target, rate, dt);
                slot.value
            }
            None => {
                self.anim.insert(
                    key,
                    Animated {
                        value: target,
                        alive: true,
                    },
                );
                target
            }
        }
    }

    /// How tall `id`'s contents measured last time they were laid out, or `0.0`
    /// if they never have been.
    pub(crate) fn measured(&self, id: u64) -> f32 {
        self.measured
            .iter()
            .find(|(k, _)| *k == id)
            .map_or(0.0, |(_, v)| *v)
    }

    /// Remember how tall `id`'s contents measured this frame.
    pub(crate) fn set_measured(&mut self, id: u64, height: f32) {
        match self.measured.iter_mut().find(|(k, _)| *k == id) {
            Some((_, h)) => *h = height,
            None => self.measured.push((id, height)),
        }
    }

    /// Record which row `id` was asked to reveal, and report whether that is a
    /// change from what it was last asked.
    ///
    /// The answer, not the value, is what a scroll area acts on — see
    /// [`UiState::revealed`](Self::revealed)'s field docs.
    pub(crate) fn take_reveal(&mut self, id: u64, index: usize) -> bool {
        match self.revealed.iter_mut().find(|(k, _)| *k == id) {
            Some((_, i)) => {
                let changed = *i != index;
                *i = index;
                changed
            }
            None => {
                self.revealed.push((id, index));
                true
            }
        }
    }

    /// Whether `id`'s contents are expanded, defaulting to `default` the first
    /// time that id is seen.
    pub(crate) fn is_open(&self, id: u64, default: bool) -> bool {
        self.open
            .iter()
            .find(|(k, _)| *k == id)
            .map_or(default, |(_, v)| *v)
    }

    /// Flip `id`'s expanded state, seeding from `default` if it is new.
    pub(crate) fn toggle_open(&mut self, id: u64, default: bool) {
        match self.open.iter_mut().find(|(k, _)| *k == id) {
            Some((_, v)) => *v = !*v,
            None => self.open.push((id, !default)),
        }
    }

    /// How far `id`'s scroll area is scrolled from its top.
    pub(crate) fn scroll_offset(&self, id: u64) -> f32 {
        self.scroll
            .iter()
            .find(|(k, _)| *k == id)
            .map_or(0.0, |(_, v)| *v)
    }

    /// Set `id`'s scroll offset.
    pub(crate) fn set_scroll_offset(&mut self, id: u64, offset: f32) {
        match self.scroll.iter_mut().find(|(k, _)| *k == id) {
            Some((_, v)) => *v = offset,
            None => self.scroll.push((id, offset)),
        }
    }

    /// Where panel `id` was last frame, if it has ever been laid out.
    pub(crate) fn panel_rect(&self, id: u64) -> Option<Rect> {
        self.panels.iter().find(|(k, _)| *k == id).map(|(_, r)| *r)
    }

    /// Remember where panel `id` ended up this frame.
    pub(crate) fn set_panel_rect(&mut self, id: u64, rect: Rect) {
        match self.panels.iter_mut().find(|(k, _)| *k == id) {
            Some((_, r)) => *r = rect,
            None => self.panels.push((id, rect)),
        }
    }
}

/// Hash a label into a stable id, scoped by its parent.
///
/// FNV-1a over the label, mixed with the enclosing scope's id — and pointedly
/// **not** with the widget's position in the panel.
///
/// That last part is the whole design, and it was learned the hard way. Ids
/// originally mixed in a per-scope sequence number, which meant a widget's
/// identity depended on how many widgets were declared before it. Any
/// conditional row — `if pending { ui.label("rebuilding…") }`, which is an
/// entirely ordinary thing to write — renumbered everything below it the frame
/// it appeared, and a slider being dragged at that moment lost its `active`
/// claim and stopped following the cursor.
///
/// Keying on the label instead means a widget keeps its identity no matter what
/// appears above it. The cost is that two widgets sharing a label in one scope
/// would collide, which [`Ui::next_id`](crate::Ui::next_id) resolves by
/// re-hashing duplicates.
pub(crate) fn hash_id(parent: u64, label: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h = mix(h, parent);
    for b in label.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Fold a whole `u64` into an FNV-1a accumulator, byte by byte.
fn mix(mut h: u64, value: u64) -> u64 {
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
