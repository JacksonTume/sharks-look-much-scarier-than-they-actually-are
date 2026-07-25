//! Click widgets: a push button and a toggle.
//!
//! Both activate on the press *edge* rather than the held state, so holding the
//! mouse down doesn't fire every frame. True press-inside-release-inside click
//! semantics need the hot/active tracking that arrives in UI Slice 1.

use crate::theme::*;
use crate::Ui;

impl Ui<'_> {
    /// A clickable button. Returns `true` on the frame it is clicked.
    pub fn button(&mut self, label: &str) -> bool {
        // Consumed but unused: it keeps the id sequence in step with the widget
        // order, so ids stay stable when a button is added above a slider. UI
        // Slice 1 gives it a real use (hot/active state).
        let _id = self.next_id(label);

        let rect = self.row(ROW_H - 4.0);
        let hovered = self.hovered(rect);
        let clicked = hovered && self.input.primary_pressed;

        let bg = if hovered { COL_BTN_HOT } else { COL_BTN };
        self.painter.rect(rect.x, rect.y, rect.w, rect.h, bg);

        // Center the label within the button.
        let tw = self.painter.text_size(label, TEXT_PX)[0];
        let tx = rect.x + (rect.w - tw) * 0.5;
        let ty = rect.y + (rect.h - TEXT_PX) * 0.5;
        self.painter.text(tx, ty, label, TEXT_PX, COL_TEXT);

        self.layout.advance(ROW_H);
        clicked
    }

    /// A labeled toggle. Edits `value` in place; returns `true` if it changed.
    ///
    /// The whole row is the hit target, not just the box — a 16px square is a
    /// mean thing to ask anyone to hit.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> bool {
        let _id = self.next_id(label);

        let row = self.row(ROW_H);
        let hovered = self.hovered(row);
        let box_sz = TEXT_PX;

        let mut changed = false;
        if hovered && self.input.primary_pressed {
            *value = !*value;
            changed = true;
            self.changed = true;
        }

        // The well, then a fill if checked.
        self.painter.rect(row.x, row.y, box_sz, box_sz, COL_TRACK);
        if *value {
            let inset = 3.0;
            self.painter.rect(
                row.x + inset,
                row.y + inset,
                box_sz - 2.0 * inset,
                box_sz - 2.0 * inset,
                if hovered { COL_ACCENT_HOT } else { COL_ACCENT },
            );
        }
        self.painter
            .text(row.x + box_sz + 8.0, row.y, label, TEXT_PX, COL_TEXT);

        self.layout.advance(ROW_H);
        changed
    }
}
