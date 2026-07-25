//! The look: metrics and colors, in one place.
//!
//! A handful of constants rather than a configurable style system (root
//! principle 4 — KISS): one demo with one panel does not justify theming
//! machinery yet. What it *does* justify is putting every number here instead of
//! scattered through the widgets, so the day a `Theme` struct of semantic tokens
//! arrives (UI Slice 4) the widgets change in one place.

use crate::Color;

// --- Metrics ---------------------------------------------------------------

/// Panel left edge, in physical pixels from the window's left.
pub(crate) const PANEL_X: f32 = 12.0;
/// Panel top edge.
pub(crate) const PANEL_Y: f32 = 12.0;
/// Panel width. Fixed — anchored, resizable panels are UI Slice 3.
pub(crate) const PANEL_W: f32 = 340.0;
/// Padding between the panel edge and its contents.
pub(crate) const PAD: f32 = 10.0;

/// Body text cell size.
pub(crate) const TEXT_PX: f32 = 16.0;
/// Panel title cell size.
pub(crate) const TITLE_PX: f32 = 20.0;
/// Section heading cell size.
pub(crate) const SECTION_PX: f32 = 15.0;

/// The height of one standard widget row.
pub(crate) const ROW_H: f32 = 24.0;
/// Slider track thickness.
pub(crate) const TRACK_H: f32 = 8.0;
/// Slider knob width.
pub(crate) const KNOB_W: f32 = 10.0;

/// The content width available inside the panel's padding.
pub(crate) const CONTENT_W: f32 = PANEL_W - 2.0 * PAD;
/// The x every widget's content starts at.
pub(crate) const CONTENT_X: f32 = PANEL_X + PAD;

// --- Colors ----------------------------------------------------------------

/// Panel background (translucent, so the 3D scene reads through).
pub(crate) const COL_PANEL: Color = [0.04, 0.06, 0.09, 0.78];
/// Primary text.
pub(crate) const COL_TEXT: Color = [0.86, 0.90, 0.95, 1.0];
/// Secondary text (readouts, hints).
pub(crate) const COL_MUTED: Color = [0.55, 0.60, 0.68, 1.0];
/// Section headings.
pub(crate) const COL_SECTION: Color = [0.45, 0.66, 0.92, 1.0];
/// Accent fill (slider fill, checkbox tick, title rule).
pub(crate) const COL_ACCENT: Color = [0.26, 0.59, 0.98, 1.0];
/// Accent fill, hovered or active.
pub(crate) const COL_ACCENT_HOT: Color = [0.42, 0.72, 1.0, 1.0];
/// Slider track / checkbox well / separator — a faint light wash.
pub(crate) const COL_TRACK: Color = [1.0, 1.0, 1.0, 0.14];
/// Button fill.
pub(crate) const COL_BTN: Color = [0.18, 0.32, 0.55, 1.0];
/// Button fill, hovered.
pub(crate) const COL_BTN_HOT: Color = [0.26, 0.46, 0.78, 1.0];
