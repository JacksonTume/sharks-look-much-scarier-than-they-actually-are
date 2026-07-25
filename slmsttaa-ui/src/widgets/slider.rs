//! The draggable float slider — the widget the terrain demo actually runs on.
//!
//! It is the widget that most needs [`Ui::interact`]'s `active` id: once you
//! grab the knob the drag has to follow the cursor even when it leaves the
//! track, and "which widget owns the pointer right now" is the only way to know
//! that the motion belongs to this slider and not to the camera behind it.

use crate::theme::*;
use crate::{Rect, Response, Ui};

impl Ui<'_> {
    /// A labeled, draggable float slider over `[min, max]`. Edits `value` in
    /// place; read [`Response::changed`].
    ///
    /// Renders as a `label: value` line over a track with a draggable knob.
    pub fn slider(&mut self, label: &str, value: &mut f32, min: f32, max: f32) -> Response {
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
    ) -> Response {
        let id = self.next_id(label);
        let row = self.allocate([0.0, ROW_H + TEXT_PX]);

        // Header line: "label: value".
        let header = format!("{label}: {value:.decimals$}");
        self.painter.text(row.x, row.y, &header, TEXT_PX, COL_TEXT);

        let track_y = row.y + TEXT_PX + 5.0;
        // The hit band is taller than the visible track so it's easy to grab.
        let band = Rect::new(row.x, track_y - 6.0, row.w, TRACK_H + 12.0);
        let mut response = self.interact(band, id);

        let span = (max - min).max(f32::EPSILON);
        if response.held {
            if let Some((px, _)) = self.input.cursor {
                let t = ((px - row.x) / row.w).clamp(0.0, 1.0);
                let new_val = min + t * span;
                if (new_val - *value).abs() > f32::EPSILON {
                    *value = new_val;
                    response.changed = true;
                    self.changed = true;
                }
            }
        }

        // Track, filled portion, and knob.
        let t = ((*value - min) / span).clamp(0.0, 1.0);
        self.painter.rect(row.x, track_y, row.w, TRACK_H, COL_TRACK);
        self.painter
            .rect(row.x, track_y, row.w * t, TRACK_H, COL_ACCENT);

        let knob_x = (row.x + row.w * t - KNOB_W * 0.5).clamp(row.x, row.max_x() - KNOB_W);
        let knob_col = if response.held || response.hovered {
            COL_ACCENT_HOT
        } else {
            COL_TEXT
        };
        self.painter
            .rect(knob_x, track_y - 4.0, KNOB_W, TRACK_H + 8.0, knob_col);

        response
    }
}
