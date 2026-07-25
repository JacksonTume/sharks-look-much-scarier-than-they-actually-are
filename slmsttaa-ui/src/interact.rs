//! Input and the little state an immediate-mode UI cannot avoid keeping.
//!
//! Two types live here, and the split between them is the point: [`UiInput`] is
//! *this frame*, handed in fresh every time; [`UiState`] is the handful of facts
//! that must survive between frames (which slider is being dragged, how tall the
//! panel came out).
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

/// One frame of pointer input, as the UI sees it.
///
/// The host fills this in each frame. Coordinates are physical pixels with the
/// origin at the top-left, matching what a [`Painter`](crate::Painter) draws in.
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
    pub(crate) fn hits(&self, rect: crate::Rect) -> bool {
        self.cursor.is_some_and(|p| rect.contains(p))
    }
}

/// Persistent UI state that survives between frames.
///
/// Immediate-mode UIs need almost no retained state; this is all of it. The host
/// owns one and lends it to each [`Ui`](crate::Ui) frame.
#[derive(Debug, Default)]
pub struct UiState {
    /// The id of the widget currently capturing the pointer (a slider being
    /// dragged), so the drag continues even if the cursor leaves the track.
    pub(crate) active: Option<u64>,
    /// Last frame's panel height, used to draw the background behind a panel
    /// whose height we only know after laying out its contents.
    pub(crate) panel_height: f32,
}

/// Hash a widget label + its call index into a stable id.
///
/// FNV-1a over the label, mixed with the sequence number so duplicate labels in
/// one panel still get distinct ids. Sequence-based ids are order-dependent —
/// the parent-scoped id stack that fixes that is UI Slice 1.
pub(crate) fn hash_id(seq: u64, label: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325 ^ seq.wrapping_mul(0x100000001b3);
    for b in label.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
