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
//! It is deliberately narrow: one pointer, one button. Right/middle buttons,
//! typed characters, and modifiers arrive when a demo actually needs them —
//! never speculatively (root principle 2).

use crate::Rect;

/// One frame of pointer input, as the UI sees it.
///
/// The host fills this in each frame. Coordinates are **logical points** with
/// the origin at the top-left, matching what a [`Painter`](crate::Painter) draws
/// in — a host on a HiDPI display divides physical cursor coordinates by the
/// scale factor before filling this in.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UiInput {
    /// Where the pointer is, or `None` if it hasn't been seen yet (or has left
    /// the window). Hit-testing against `None` never hits.
    pub cursor: Option<(f32, f32)>,
    /// Whether the primary (left) button is *currently down*. Drives drags.
    pub primary_held: bool,
    /// Whether the primary button went down *this frame* — a press edge, true
    /// for exactly one frame. Drives clicks.
    pub primary_pressed: bool,
}

impl UiInput {
    /// Whether the pointer is inside the given rectangle this frame.
    pub(crate) fn hits(&self, rect: Rect) -> bool {
        self.cursor.is_some_and(|p| rect.contains(p))
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
    /// The primary button went down on it this frame.
    pub clicked: bool,
    /// The widget edited its bound value this frame. Always `false` for widgets
    /// that don't bind one.
    pub changed: bool,
    /// Whether this widget's contents should be shown.
    ///
    /// Only [`Ui::section`](crate::Ui::section) — the one collapsible widget —
    /// ever reports `false`. Everything else reports `true`, so the
    /// `if ui.something(..).open { .. }` shape is always safe to write.
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
/// - **focused** — what would receive the keyboard. Nothing reads it yet: there
///   is no key input in [`UiInput`] until a demo needs typing. It is tracked now
///   because click-to-focus is the half that belongs with this slice, and
///   bolting it on later would mean revisiting every widget.
#[derive(Debug, Default)]
pub struct UiState {
    /// The widget the pointer is over this frame.
    pub(crate) hot: Option<u64>,
    /// The widget currently capturing the pointer (a slider being dragged), so
    /// the drag continues even if the cursor leaves the track.
    pub(crate) active: Option<u64>,
    /// The widget that would receive keyboard input.
    pub(crate) focused: Option<u64>,
    /// Last frame's panel height, so `wants_pointer` still knows the panel's
    /// extent before this frame's widgets have been declared.
    pub(crate) panel_height: f32,
    /// Collapsed/expanded state per section id.
    ///
    /// A `Vec` rather than a `HashMap`: a panel has a handful of sections, a
    /// linear scan over them beats hashing, and it keeps this crate's state
    /// trivially `Debug`-able. Revisit if a consumer ever has hundreds.
    pub(crate) open: Vec<(u64, bool)>,
}

impl UiState {
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
