//! The draggable float slider — the widget the terrain demo actually runs on.
//!
//! It is the one widget here that needs state across frames: once you grab the
//! knob the drag has to follow the cursor even when it leaves the track, so the
//! slider claims [`UiState::active`](crate::UiState) until the button comes up.

use crate::theme::*;
use crate::{Rect, Ui};

impl Ui<'_> {
    /// A labeled, draggable float slider over `[min, max]`. Edits `value` in
    /// place and returns `true` if it changed this frame.
    ///
    /// Renders as a `label: value` line over a track with a draggable knob.
    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> bool {
        self.slider_fmt(label, value, min, max, 2)
    }

    /// [`Ui::slider`] with control over how many decimals the value shows.
    pub fn slider_fmt(
        &mut self,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        decimals: usize,
    ) -> bool {
        let id = self.next_id(label);
        let y = self.layout.y();

        // Header line: "label: value".
        let header = format!("{label}: {value:.decimals$}");
        self.painter.text(CONTENT_X, y, &header, TEXT_PX, COL_TEXT);

        let track_y = y + TEXT_PX + 5.0;
        // The hit band is taller than the visible track so it's easy to grab.
        let band = Rect::new(CONTENT_X, track_y - 6.0, CONTENT_W, TRACK_H + 12.0);
        let hovered = self.hovered(band);

        // Capture the drag on press, release it when the button comes up —
        // wherever the cursor has wandered to by then.
        if hovered && self.input.primary_pressed {
            self.state.active = Some(id);
        }
        if self.state.active == Some(id) && !self.input.primary_held {
            self.state.active = None;
        }

        let span = (max - min).max(f32::EPSILON);
        let mut changed = false;
        if self.state.active == Some(id) {
            if let Some((px, _)) = self.input.cursor {
                let t = ((px - CONTENT_X) / CONTENT_W).clamp(0.0, 1.0);
                let new_val = min + t * span;
                if (new_val - *value).abs() > f32::EPSILON {
                    *value = new_val;
                    changed = true;
                    self.changed = true;
                }
            }
        }

        // Track, filled portion, and knob.
        let t = ((*value - min) / span).clamp(0.0, 1.0);
        self.painter
            .rect(CONTENT_X, track_y, CONTENT_W, TRACK_H, COL_TRACK);
        self.painter
            .rect(CONTENT_X, track_y, CONTENT_W * t, TRACK_H, COL_ACCENT);

        let knob_x = (CONTENT_X + CONTENT_W * t - KNOB_W * 0.5)
            .clamp(CONTENT_X, CONTENT_X + CONTENT_W - KNOB_W);
        let knob_col = if self.state.active == Some(id) || hovered {
            COL_ACCENT_HOT
        } else {
            COL_TEXT
        };
        self.painter
            .rect(knob_x, track_y - 4.0, KNOB_W, TRACK_H + 8.0, knob_col);

        self.layout.advance(ROW_H + TEXT_PX);
        changed
    }
}
