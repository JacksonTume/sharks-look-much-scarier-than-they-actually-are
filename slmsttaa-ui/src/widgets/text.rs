//! Headings, labels, and rules.
//!
//! Only [`Ui::section`] is interactive — it collapses. The rest still return a
//! [`Response`] and still hit-test, because a label's rectangle and hover state
//! are exactly what a consumer needs to hang a tooltip on one later.

use crate::theme::*;
use crate::{Rect, Response, Ui};

impl Ui<'_> {
    /// A bold heading row, underlined with a short accent bar.
    pub fn title(&mut self, text: &str) -> Response {
        let id = self.next_id(text);
        let row = self.allocate([0.0, TITLE_PX + 12.0]);
        let response = self.interact(row, id);

        self.painter.text(row.x, row.y, text, TITLE_PX, COL_TEXT);
        // A short accent rule under the title gives the panel a clear header
        // instead of a flat wall of text.
        let tw = self.painter.text_size(text, TITLE_PX)[0];
        self.painter.fill_rect(
            Rect::new(row.x, row.y + TITLE_PX + 3.0, tw.max(40.0), 2.0),
            1.0,
            COL_ACCENT,
        );

        response
    }

    /// A collapsible section heading. Returns a [`Response`] whose `open` field
    /// says whether the section's contents should be declared:
    ///
    /// ```
    /// # use slmsttaa_ui::{theme, Anchor, RecordingPainter, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # let mut frequency = 1.0_f32;
    /// # ui.panel(Anchor::TopLeft, theme::PANEL_W, |ui| {
    /// if ui.section("Base shape").open {
    ///     ui.slider("frequency", &mut frequency, 0.5, 8.0).show();
    /// }
    /// # });
    /// ```
    ///
    /// Sections start expanded, and clicking the heading toggles it. The state
    /// is keyed by the heading's id and lives in [`UiState`](crate::UiState), so
    /// it survives the frame.
    ///
    /// Because ids are keyed by label rather than by declaration order, a
    /// section keeps its collapsed state when rows appear above it. Two sections
    /// sharing a label in one scope are separated with [`Ui::push_id`].
    pub fn section(&mut self, text: &str) -> Response {
        let id = self.next_id(text);
        let row = self.allocate([0.0, 2.0 + SECTION_PX + 6.0]);
        let mut response = self.interact(row, id);

        if response.clicked {
            self.state.toggle_open(id, true);
        }
        let open = self.state.is_open(id, true);
        response.open = open;

        // A caret rather than a label change: the heading text stays where the
        // eye expects it, and the marker reads at a glance.
        let caret = if open { "-" } else { "+" };
        let color = if response.hovered {
            COL_ACCENT_HOT
        } else {
            COL_SECTION
        };
        self.painter
            .text(row.x, row.y + 2.0, caret, SECTION_PX, color);
        self.painter
            .text(row.x + INDENT, row.y + 2.0, text, SECTION_PX, color);

        response
    }

    /// A plain, full-width text row.
    pub fn label(&mut self, text: &str) -> Response {
        self.text_row(text, COL_TEXT)
    }

    /// A muted text row, for secondary readouts and hints.
    pub fn label_muted(&mut self, text: &str) -> Response {
        self.text_row(text, COL_MUTED)
    }

    /// A row with `label` at the left edge and `value` right-aligned against the
    /// right edge.
    ///
    /// This is the row that retired `format!("{label}: {value}")`. One string
    /// grows until it runs out of panel and gets cut mid-glyph by the clip rect;
    /// two runs anchored to opposite edges use the whole width and put the
    /// numbers in a column the eye can scan. `"area exponent m"` plus `"0.50"`
    /// is 304 points where `"area exponent m: 0.50"` is 336 — the difference
    /// between fitting and not.
    ///
    /// The value is measured with [`Painter::text_size`](crate::Painter::text_size),
    /// so it is only ever as right-aligned as the font's metrics are honest.
    pub fn label_value(&mut self, label: &str, value: &str) -> Response {
        // Seeded with a constant for the same reason `text_row` is: the value
        // side is usually a live number, and hashing it would hand the row a new
        // id every frame.
        let id = self.next_id("label_value");
        let row = self.allocate([0.0, ROW_H]);
        let response = self.interact(row, id);

        let value_w = self.painter.text_size(value, TEXT_PX)[0];
        self.painter.text(row.x, row.y, label, TEXT_PX, COL_TEXT);
        self.painter
            .text(row.max_x() - value_w, row.y, value, TEXT_PX, COL_MUTED);
        response
    }

    /// A thin horizontal divider.
    pub fn separator(&mut self) -> Response {
        let id = self.next_id("separator");
        let row = self.allocate([0.0, 12.0]);
        let response = self.interact(row, id);
        self.painter
            .rect(Rect::new(row.x, row.y + 4.0, row.w, 1.0), COL_TRACK);
        response
    }

    /// Shared body of [`Ui::label`] and [`Ui::label_muted`].
    fn text_row(&mut self, text: &str, color: crate::Color) -> Response {
        // Seeded with a constant, not the text: labels routinely show live
        // numbers ("60 fps"), and hashing those would give the row a new id
        // every frame.
        let id = self.next_id("label");
        let row = self.allocate([0.0, ROW_H]);
        let response = self.interact(row, id);
        self.painter.text(row.x, row.y, text, TEXT_PX, color);
        response
    }
}
