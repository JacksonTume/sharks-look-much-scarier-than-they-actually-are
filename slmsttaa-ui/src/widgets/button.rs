//! Click widgets: a push button and a toggle.
//!
//! Both fire on the press *edge* rather than the held state, so holding the
//! mouse down doesn't retrigger every frame. True press-inside-release-inside
//! semantics would use the `active` id [`Ui::interact`] maintains; the button
//! doesn't yet, because nothing has asked to cancel a click by dragging off it.

use crate::theme::*;
use crate::{Rect, Response, Ui};

impl Ui<'_> {
    /// A clickable button. Read [`Response::clicked`].
    pub fn button(&mut self, label: &str) -> Response {
        let id = self.next_id(label);
        // The row owns the trailing gap; the button face is the top of it.
        let row = self.allocate([0.0, ROW_H]);
        let face = Rect::new(row.x, row.y, row.w, ROW_H - 4.0);
        let response = self.interact(face, id);

        let bg = if response.held {
            COL_ACCENT
        } else if response.hovered {
            COL_BTN_HOT
        } else {
            COL_BTN
        };
        self.painter.fill_rect(face, RADIUS, bg);
        if response.focused {
            self.painter.stroke_rect(face, RADIUS, RING, COL_RING);
        }

        // Center the label within the button.
        let tw = self.painter.text_size(label, TEXT_PX)[0];
        let tx = face.x + (face.w - tw) * 0.5;
        let ty = face.y + (face.h - TEXT_PX) * 0.5;
        self.painter.text(tx, ty, label, TEXT_PX, COL_TEXT);

        response
    }

    /// A labeled toggle. Edits `value` in place; read [`Response::changed`].
    ///
    /// The whole row is the hit target, not just the box — a 16-point square is
    /// a mean thing to ask anyone to hit.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> Response {
        let id = self.next_id(label);
        let row = self.allocate([0.0, ROW_H]);
        let mut response = self.interact(row, id);
        let well = Rect::new(row.x, row.y, TEXT_PX, TEXT_PX);

        if response.clicked {
            *value = !*value;
            response.changed = true;
            self.changed = true;
        }

        self.painter.fill_rect(well, RADIUS, COL_TRACK);
        if *value {
            let tick = if response.hovered {
                COL_ACCENT_HOT
            } else {
                COL_ACCENT
            };
            self.painter.fill_rect(well.shrink(3.0), RADIUS - 1.0, tick);
        }
        if response.focused {
            self.painter.stroke_rect(well, RADIUS, BORDER, COL_RING);
        }
        self.painter
            .text(well.max_x() + 8.0, row.y, label, TEXT_PX, COL_TEXT);

        response
    }
}
