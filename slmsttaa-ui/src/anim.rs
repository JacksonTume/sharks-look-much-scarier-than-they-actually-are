//! Values that ease toward a target instead of jumping to it.
//!
//! An immediate-mode UI has no widget objects to hang a transition off — the
//! button that was hovered last frame is not an object that still exists, it is
//! a rectangle that will be re-declared in a moment. What persists is the **id**,
//! and that turns out to be enough: [`UiState`](crate::UiState) keeps one float
//! per `(id, property)` pair, and [`Ui::animate`](crate::Ui::animate) nudges it a
//! frame's worth closer to whatever the widget asks for this time.
//!
//! ## Why exponential, and why it takes `dt`
//!
//! The tempting one-liner is `value += (target - value) * 0.2`. It is wrong in a
//! way that hides: it converges in a fixed number of *frames*, so the same fade
//! takes 130 ms at 144 Hz and 330 ms at 60 Hz, and on a machine that stutters it
//! visibly changes character. [`approach`] instead integrates exponential decay
//! over the real elapsed time — `1 - e^(-rate·dt)` — which gives the same curve
//! in the same wall-clock milliseconds at any frame rate, and behaves correctly
//! when one frame takes ten times as long as the last.
//!
//! That is what pulls `dt` across the seam. [`UiInput`](crate::UiInput) gained
//! one `f32` field for this slice and the host fills it in, exactly as it already
//! does for the cursor and the viewport — the toolkit still owns no clock and
//! knows nothing about frames.
//!
//! ## Rate, and turning motion off
//!
//! `rate` is in **e-folds per second**: a value covers about 63% of the remaining
//! distance in `1/rate` seconds and is done (see [`SNAP`]) after roughly
//! `4/rate`. The two rates the widgets use are theme tokens —
//! [`Motion`](crate::theme::Motion) — so a consumer retunes the feel of the whole
//! toolkit the same way it retunes a color.
//!
//! An infinite rate snaps instantly, which is deliberately how motion is
//! *disabled*: there is no `animate: bool` anywhere, because a flag would have to
//! be checked at every call site and one of them would eventually forget.
//!
//! ```
//! # use slmsttaa_ui::{theme::Motion, Theme};
//! // Respect a "reduce motion" preference: every widget stops easing, and not
//! // one of them has a branch for it.
//! let mut theme = Theme::dark();
//! theme.motion = Motion::none();
//! ```

use crate::Color;

/// How close to its target a value has to get before it is snapped exactly onto
/// it, relative to the target's own magnitude.
///
/// Exponential decay never actually *arrives*, and "never arrives" is not a
/// harmless property here. A section is fully open at `t == 1.0` and takes a fast
/// path that skips clipping entirely; at `t == 0.99999` it would clip forever,
/// and a value that is asymptotically approaching zero keeps a scroll area
/// re-laying-out for the rest of the program. Snapping ends the animation for
/// real.
pub const SNAP: f32 = 1.0e-4;

/// Move `current` a `dt`-sized step toward `target`, at `rate` e-folds per
/// second.
///
/// Frame-rate independent: the curve is a function of elapsed time, not of how
/// many times this was called.
///
/// `dt <= 0.0` **snaps to the target** rather than freezing at the current
/// value, and the difference matters more than it looks. A host that never fills
/// [`UiInput::dt`](crate::UiInput::dt) — one written before this slice existed,
/// or a test — would otherwise get a UI whose sections could never finish
/// collapsing. Snapping instead makes `dt` genuinely optional at the seam: leave
/// it at zero and every widget behaves exactly as it did before anything here
/// eased. Freezing would be the more literal reading of "no time passed", and it
/// would be a trap.
///
/// ```
/// # use slmsttaa_ui::anim::approach;
/// // Two 8 ms frames land in the same place as one 16 ms frame.
/// let stepped = approach(approach(0.0, 1.0, 20.0, 0.008), 1.0, 20.0, 0.008);
/// let single = approach(0.0, 1.0, 20.0, 0.016);
/// assert!((stepped - single).abs() < 1.0e-6);
/// ```
pub fn approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    if current == target {
        return current;
    }
    if dt <= 0.0 {
        return target;
    }
    // `rate` of infinity means "no motion", and `-inf * dt` exponentiates to
    // zero, so the step below is a straight assignment. Guarding `dt` first is
    // what keeps `inf * 0.0` from producing a NaN.
    let step = 1.0 - (-rate * dt).exp();
    let next = current + (target - current) * step;
    if (target - next).abs() <= SNAP * target.abs().max(1.0) {
        target
    } else {
        next
    }
}

/// Mix two colors, `t` of the way from `a` to `b`, alpha included.
///
/// At `t == 0.0` and `t == 1.0` this returns `a` and `b` *bit for bit*, which is
/// load-bearing rather than incidental: `tests/theme.rs` asserts that every color
/// reaching the painter is one the theme supplied, and a fade that only
/// approximately reproduced its endpoints would fail it — correctly, since a
/// widget resting on a color the theme never named is the exact bug that test
/// exists to catch.
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    if t <= 0.0 {
        return a;
    }
    if t >= 1.0 {
        return b;
    }
    let mut out = a;
    for i in 0..4 {
        out[i] = a[i] + (b[i] - a[i]) * t;
    }
    out
}

/// Scale `color`'s alpha by `t`, for something that fades in from nothing rather
/// than between two colors — a focus ring, a pressed scrim.
///
/// Returns `color` unchanged at `t == 1.0`, for the reason [`lerp`] does.
///
/// A caller should skip drawing entirely at `t == 0.0` rather than paint a fully
/// transparent rectangle: it saves a primitive, and it keeps the frame's recorded
/// color list free of alpha-zero ghosts that no theme token accounts for.
pub fn fade(color: Color, t: f32) -> Color {
    if t >= 1.0 {
        return color;
    }
    [color[0], color[1], color[2], color[3] * t.max(0.0)]
}
