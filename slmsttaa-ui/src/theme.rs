//! The look: metrics and colors, in one place.
//!
//! A handful of constants rather than a configurable style system (root
//! principle 4 — KISS): one demo with one panel does not justify theming
//! machinery yet. What it *does* justify is putting every number here instead of
//! scattered through the widgets, so the day a `Theme` struct of semantic tokens
//! arrives (UI Slice 4) the widgets change in one place.
//!
//! These are **public** for the same reason `allocate` / `interact` / `painter`
//! are: a widget written by a consumer has to be able to look like the ones that
//! ship here, and it cannot if the metrics and colors are private. That is the
//! unprivileged-widget rule, and this module is part of its cost.
//!
//! All metrics are in **logical points**, so they mean the same thing on a 1×
//! and a 2× display.

use crate::Color;

// --- Metrics ---------------------------------------------------------------

/// Panel left edge, in points from the window's left.
pub const PANEL_X: f32 = 12.0;
/// Panel top edge.
pub const PANEL_Y: f32 = 12.0;
/// Panel width. Fixed — anchored, resizable panels are UI Slice 3.
pub const PANEL_W: f32 = 340.0;
/// Padding between the panel edge and its contents.
pub const PAD: f32 = 10.0;

/// Body text cell size.
pub const TEXT_PX: f32 = 16.0;
/// Panel title cell size.
pub const TITLE_PX: f32 = 20.0;
/// Section heading cell size.
pub const SECTION_PX: f32 = 15.0;

/// The height of one standard widget row.
pub const ROW_H: f32 = 24.0;
/// Slider track thickness.
pub const TRACK_H: f32 = 8.0;
/// Slider knob width.
pub const KNOB_W: f32 = 10.0;

/// The content width available inside the panel's padding.
pub const CONTENT_W: f32 = PANEL_W - 2.0 * PAD;
/// The x every widget's content starts at.
pub const CONTENT_X: f32 = PANEL_X + PAD;

/// Width of the scroll indicator drawn beside an overflowing panel body.
pub const SCROLLBAR_W: f32 = 4.0;
/// How many points one wheel notch scrolls.
pub const SCROLL_SPEED: f32 = 28.0;

// --- Radii and strokes -----------------------------------------------------
//
// A two-step radius scale, which is all a panel of rows needs. The full scale
// arrives with the `Theme` struct in UI Slice 4.

/// Corner radius for panels and other large surfaces.
pub const RADIUS_LG: f32 = 8.0;
/// Corner radius for controls: buttons, checkbox wells, the scroll indicator.
pub const RADIUS: f32 = 4.0;
/// Standard hairline stroke width.
pub const BORDER: f32 = 1.0;
/// Focus ring thickness.
pub const RING: f32 = 2.0;

// --- Colors ----------------------------------------------------------------

/// Panel background (translucent, so the 3D scene reads through).
pub const COL_PANEL: Color = [0.04, 0.06, 0.09, 0.78];
/// Primary text.
pub const COL_TEXT: Color = [0.86, 0.90, 0.95, 1.0];
/// Secondary text (readouts, hints).
pub const COL_MUTED: Color = [0.55, 0.60, 0.68, 1.0];
/// Section headings.
pub const COL_SECTION: Color = [0.45, 0.66, 0.92, 1.0];
/// Accent fill (slider fill, checkbox tick, title rule).
pub const COL_ACCENT: Color = [0.26, 0.59, 0.98, 1.0];
/// Accent fill, hovered or active.
pub const COL_ACCENT_HOT: Color = [0.42, 0.72, 1.0, 1.0];
/// Slider track / checkbox well / separator — a faint light wash.
pub const COL_TRACK: Color = [1.0, 1.0, 1.0, 0.14];
/// Button fill.
pub const COL_BTN: Color = [0.18, 0.32, 0.55, 1.0];
/// Button fill, hovered.
pub const COL_BTN_HOT: Color = [0.26, 0.46, 0.78, 1.0];
/// Panel border — a hairline that separates the panel from a busy 3D scene far
/// more cheaply than a drop shadow would.
pub const COL_BORDER: Color = [1.0, 1.0, 1.0, 0.10];
/// Focus ring, drawn around the focused widget.
pub const COL_RING: Color = [0.42, 0.72, 1.0, 0.85];
