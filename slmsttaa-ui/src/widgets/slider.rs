//! The draggable float slider — the widget the terrain demo actually runs on.
//!
//! It is the widget that most needs [`Ui::interact`]'s `active` id: once you
//! grab the knob the drag has to follow the cursor even when it leaves the
//! track, and "which widget owns the pointer right now" is the only way to know
//! that the motion belongs to this slider and not to the camera behind it.
//!
//! It was also the **first** widget with a builder. Every other one took its
//! arguments and drew; this one has a value to format and a row to arrange, and
//! those are exactly the two things a consumer wants to override. Rather than
//! grow a `slider_fmt`, then a `slider_fmt_compact`, then a variant that takes a
//! closure, it got [`Slider`] in Slice 3 — a rehearsal for the
//! [`Button`](crate::Button) builder Slice 4 generalized it into.

use crate::{font, Rect, Response, Ui};

/// How a [`Slider`] arranges its label, value, and track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SliderLayout {
    /// Label and value on one line, full-width track underneath. Two rows tall,
    /// and the default: it gives the track the whole panel to work with, which
    /// is what makes a value pickable.
    #[default]
    Stacked,
    /// Label, track, and value all on one row. Half the height, at the cost of
    /// a track only as wide as the label and value leave it.
    Compact,
}

/// A draggable float slider over `[min, max]`, configured then shown.
///
/// Built by [`Ui::slider`]. Nothing is drawn until [`Slider::show`] is called,
/// which is what the `must_use` is guarding.
///
/// ```
/// # use slmsttaa_ui::{Anchor, RecordingPainter, SliderLayout, Theme, Ui, UiInput, UiState};
/// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
/// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
/// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
/// # let (mut m, mut erodibility) = (0.5_f32, 3.0e-3_f32);
/// // The plain case.
/// ui.slider("area exponent m", &mut m, 0.2, 1.0).show();
///
/// // Overridden: a value that needs scientific notation, on one row.
/// ui.slider("erodibility", &mut erodibility, 1.0e-5, 6.0e-3)
///     .value_fmt(|v| format!("{v:.1e}"))
///     .layout(SliderLayout::Compact)
///     .show();
/// # });
/// ```
#[must_use = "a slider draws nothing until `.show()` is called"]
pub struct Slider<'u, 'a, 'v> {
    ui: &'u mut Ui<'a>,
    label: &'u str,
    value: &'v mut f32,
    min: f32,
    max: f32,
    decimals: usize,
    layout: SliderLayout,
    fmt: Option<Box<dyn Fn(f32) -> String + 'u>>,
}

impl<'a> Ui<'a> {
    /// Begin a draggable float slider over `[min, max]`, editing `value` in
    /// place.
    ///
    /// Returns a [`Slider`] to configure; call [`Slider::show`] to draw it and
    /// get the [`Response`], whose `changed` says whether the value moved.
    pub fn slider<'u, 'v>(
        &'u mut self,
        label: &'u str,
        value: &'v mut f32,
        min: f32,
        max: f32,
    ) -> Slider<'u, 'a, 'v> {
        Slider {
            ui: self,
            label,
            value,
            min,
            max,
            decimals: 2,
            layout: SliderLayout::default(),
            fmt: None,
        }
    }
}

impl<'u, 'a, 'v> Slider<'u, 'a, 'v> {
    /// How many decimal places the readout shows. Defaults to 2, and is ignored
    /// once [`Slider::value_fmt`] is set.
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals;
        self
    }

    /// Format the readout yourself, for a value that decimals can't describe —
    /// scientific notation, a unit suffix, an enum name.
    pub fn value_fmt(mut self, format: impl Fn(f32) -> String + 'u) -> Self {
        self.fmt = Some(Box::new(format));
        self
    }

    /// Arrange the row differently. See [`SliderLayout`].
    pub fn layout(mut self, layout: SliderLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Lay the slider out, draw it, and report what the pointer did.
    pub fn show(self) -> Response {
        let Slider {
            ui,
            label,
            value,
            min,
            max,
            decimals,
            layout,
            fmt,
        } = self;

        let theme = *ui.theme();
        let (px, weight) = theme.text.body.parts();
        let (row_h, track_h) = (theme.control.row_h, theme.control.track_h);

        let text = match &fmt {
            Some(format) => format(*value),
            None => format!("{value:.decimals$}"),
        };
        let id = ui.next_id(label);

        let (track, mut response) = match layout {
            SliderLayout::Stacked => {
                // A text line, the track, and a trailing gap. Sized from the line
                // box rather than from `px`: the two were the same number under
                // the bitmap font and are not under a real face.
                let text_h = font::line_height(px);
                let row = ui.allocate([0.0, text_h + track_h + 9.0]);
                let value_w = font::text_width(&text, px, weight);

                // Label left, value hard against the right edge: two runs that
                // share the width instead of one that outgrows it.
                let painter = ui.painter();
                painter.text(row.x, row.y, label, px, weight, theme.color.foreground);
                painter.text(
                    row.max_x() - value_w,
                    row.y,
                    &text,
                    px,
                    weight,
                    theme.color.muted,
                );

                let track = Rect::new(row.x, row.y + text_h + 2.0, row.w, track_h);
                // The hit band is taller than the visible track so it's easy to grab.
                let band = Rect::new(track.x, track.y - 6.0, track.w, track_h + 12.0);
                (track, ui.interact(band, id))
            }
            SliderLayout::Compact => {
                let row = ui.allocate([0.0, row_h]);
                // The row owns a 4-point trailing gap, as a button's does.
                let face_h = row_h - 4.0;
                let gap = theme.space.gap;
                let label_w = font::text_width(label, px, weight);
                let value_w = font::text_width(&text, px, weight);

                let text_y = font::centered_top(row.y, face_h, px);
                let painter = ui.painter();
                painter.text(row.x, text_y, label, px, weight, theme.color.foreground);
                painter.text(
                    row.max_x() - value_w,
                    text_y,
                    &text,
                    px,
                    weight,
                    theme.color.muted,
                );

                let track_x = row.x + label_w + gap;
                let track_w = (row.max_x() - value_w - gap - track_x).max(0.0);
                let track = Rect::new(track_x, row.y + (face_h - track_h) * 0.5, track_w, track_h);
                let band = Rect::new(track.x, row.y, track.w, face_h);
                (track, ui.interact(band, id))
            }
        };

        let span = (max - min).max(f32::EPSILON);
        if response.held && track.w > 0.0 {
            // Named `cursor_x`, not `px`: that used to shadow the font size, which
            // is a trap now that `px` genuinely means something else.
            if let Some((cursor_x, _)) = ui.input().cursor {
                let t = ((cursor_x - track.x) / track.w).clamp(0.0, 1.0);
                let new_val = min + t * span;
                if (new_val - *value).abs() > f32::EPSILON {
                    *value = new_val;
                    response.changed = true;
                    ui.mark_changed();
                }
            }
        }

        let t = ((*value - min) / span).clamp(0.0, 1.0);
        draw_track(ui, track, t, &response);
        response
    }
}

/// Track, filled portion, and knob — all capsules (a radius at half the shorter
/// side rounds the ends off completely).
///
/// Split out because both layouts draw the identical control, and because it is
/// written against nothing but [`Ui::painter`] — a consumer's own slider can
/// call the same sequence and match.
fn draw_track(ui: &mut Ui, track: Rect, t: f32, response: &Response) {
    let theme = *ui.theme();
    let (track_h, knob_w) = (theme.control.track_h, theme.control.knob_w);

    let cap = track_h * 0.5;
    let knob_x = (track.x + track.w * t - knob_w * 0.5)
        .clamp(track.x, (track.max_x() - knob_w).max(track.x));
    let knob = Rect::new(knob_x, track.y - 4.0, knob_w, track_h + 8.0);
    let knob_col = if response.held || response.hovered {
        theme.color.accent_hover
    } else {
        theme.color.foreground
    };

    let painter = ui.painter();
    painter.fill_rect(track, cap, theme.color.surface);
    painter.fill_rect(
        Rect::new(track.x, track.y, track.w * t, track_h),
        cap,
        theme.color.accent,
    );
    painter.fill_rect(knob, knob_w * 0.5, knob_col);
    if response.focused {
        painter.stroke_rect(knob, knob_w * 0.5, theme.control.border, theme.color.ring);
    }
}
