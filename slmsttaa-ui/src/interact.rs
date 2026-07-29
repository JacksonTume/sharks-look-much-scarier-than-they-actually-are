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

/// One frame of host state, as the UI sees it: where the pointer is, and how
/// big the window is.
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
    /// It holds focus — it was the last thing clicked. Widgets draw a focus ring
    /// from this; keyboard input will route by it once there is any.
    pub focused: bool,
    /// The widget edited its bound value this frame. Always `false` for widgets
    /// that don't bind one.
    pub changed: bool,
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
    /// One eased float per `(widget, property)` pair — see [`anim`](crate::anim).
    ///
    /// Swept every frame (see [`UiState::begin_frame`]), which is the difference
    /// between this and the maps above. Those are keyed by containers, of which a
    /// panel has a handful; this one gets an entry per *animated property of
    /// every widget declared*, so a consumer that generates rows from changing
    /// labels would grow it without bound if nothing pruned it.
    pub(crate) anim: Vec<Animated>,
}

/// One eased float, and whether anything asked for it this frame.
#[derive(Debug)]
pub(crate) struct Animated {
    /// `hash(widget id, property name)`.
    pub(crate) key: u64,
    /// Where the value is right now.
    pub(crate) value: f32,
    /// Set by [`UiState::animate`], cleared by [`UiState::begin_frame`]. An entry
    /// that survives a whole frame without being asked for belonged to a widget
    /// that is no longer declared, and is dropped.
    pub(crate) alive: bool,
}

impl UiState {
    /// Retire animated values nothing asked for last frame, and re-arm the
    /// survivors to be asked for again.
    ///
    /// Called once per frame by [`Ui::new`](crate::Ui::new). A widget inside a
    /// collapsed section stops being declared and so loses its slots — which is
    /// correct rather than merely acceptable: it is invisible, and when it comes
    /// back it should come back settled, not mid-fade from a hover the user has
    /// long since forgotten.
    pub(crate) fn begin_frame(&mut self) {
        self.anim.retain(|slot| slot.alive);
        for slot in &mut self.anim {
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
        match self.anim.iter_mut().find(|slot| slot.key == key) {
            Some(slot) => {
                slot.alive = true;
                slot.value = crate::anim::approach(slot.value, target, rate, dt);
                slot.value
            }
            None => {
                self.anim.push(Animated {
                    key,
                    value: target,
                    alive: true,
                });
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
