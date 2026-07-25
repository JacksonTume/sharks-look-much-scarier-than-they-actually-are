//! Headings, labels, and rules.
//!
//! Only [`Ui::section`] is interactive — it collapses. The rest still return a
//! [`Response`] and still hit-test, because a label's rectangle and hover state
//! are exactly what a consumer needs to hang a tooltip on one later.

use crate::theme::*;
use crate::{Response, Ui};

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
        self.painter
            .rect(row.x, row.y + TITLE_PX + 3.0, tw.max(40.0), 2.0, COL_ACCENT);

        response
    }

    /// A collapsible section heading. Returns a [`Response`] whose `open` field
    /// says whether the section's contents should be declared:
    ///
    /// ```
    /// # use slmsttaa_ui::{RecordingPainter, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # let mut frequency = 1.0_f32;
    /// if ui.section("Base shape").open {
    ///     ui.slider("frequency", &mut frequency, 0.5, 8.0);
    /// }
    /// ```
    ///
    /// Sections start expanded, and clicking the heading toggles it. The state
    /// is keyed by the heading's id and lives in [`UiState`](crate::UiState), so
    /// it survives the frame.
    ///
    /// That id comes from [`Ui::stable_id`] rather than [`Ui::next_id`]: keyed
    /// by declaration order, a section would forget it was collapsed the moment
    /// a row was added above it. Two sections sharing a label in one scope
    /// therefore share their state — [`Ui::push_id`] separates them.
    pub fn section(&mut self, text: &str) -> Response {
        let id = self.stable_id(text);
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
            .text(row.x + SECTION_PX, row.y + 2.0, text, SECTION_PX, color);

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

    /// A thin horizontal divider.
    pub fn separator(&mut self) -> Response {
        let id = self.next_id("separator");
        let row = self.allocate([0.0, 12.0]);
        let response = self.interact(row, id);
        self.painter.rect(row.x, row.y + 4.0, row.w, 1.0, COL_TRACK);
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
