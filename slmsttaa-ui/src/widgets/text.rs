//! Headings, labels, and rules.
//!
//! Only [`Ui::section`] is interactive — it collapses. The rest still return a
//! [`Response`] and still hit-test, because a label's rectangle and hover state
//! are exactly what a consumer needs to hang a tooltip on one later.

use crate::theme::TypeStep;
use crate::{font, Color, Rect, Response, Ui};

impl Ui<'_> {
    /// A heading row in the title step — larger and heavier than body text —
    /// underlined with a short accent bar.
    pub fn title(&mut self, text: &str) -> Response {
        let theme = self.theme;
        let (px, weight) = theme.text.title.parts();

        let id = self.next_id(text);
        // The line box plus room for the rule beneath it.
        let row = self.allocate([0.0, font::line_height(px) + 8.0]);
        let response = self.interact(row, id);

        self.painter
            .text(row.x, row.y, text, px, weight, theme.color.foreground);
        // A short accent rule under the title gives the panel a clear header
        // instead of a flat wall of text. It sits under the *baseline* rather
        // than under the line box: descender space below the baseline is empty
        // for a title in caps, and a rule that clears it looks detached.
        let tw = font::text_width(text, px, weight);
        let rule_y = row.y + font::ascent(px) + 4.0;
        self.painter.fill_rect(
            Rect::new(row.x, rule_y, tw.max(40.0), 2.0),
            1.0,
            theme.color.accent,
        );

        response
    }

    /// A collapsible section heading. Returns a [`Response`] whose `open` field
    /// says whether the section's contents should be declared:
    ///
    /// ```
    /// # use slmsttaa_ui::{Anchor, RecordingPainter, Theme, Ui, UiInput, UiState};
    /// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
    /// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
    /// # let mut frequency = 1.0_f32;
    /// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
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
        let theme = self.theme;
        let (px, weight) = theme.text.section.parts();

        let id = self.next_id(text);
        let row = self.allocate([0.0, font::line_height(px) + 6.0]);
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
            theme.color.accent_hover
        } else {
            theme.color.heading
        };
        let y = font::centered_top(row.y, row.h, px);
        self.painter.text(row.x, y, caret, px, weight, color);
        self.painter
            .text(row.x + theme.space.indent, y, text, px, weight, color);

        response
    }

    /// A plain, full-width text row.
    pub fn label(&mut self, text: &str) -> Response {
        let color = self.theme.color.foreground;
        self.text_row(text, color)
    }

    /// A muted text row, for secondary readouts and hints.
    pub fn label_muted(&mut self, text: &str) -> Response {
        let color = self.theme.color.muted;
        self.text_row(text, color)
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
    /// The value is measured with [`font::text_width`], the same function that
    /// laid it out — so the alignment is exact rather than as honest as two
    /// separate metric tables happen to be.
    ///
    /// Digits are **tabular**: every digit has the widest digit's advance, so a
    /// live readout keeps still instead of shuffling sideways as `1`s and `0`s
    /// trade places. Inter's proportional `1` is 37% narrower than its `0`, which
    /// is very visible on a slider being dragged.
    pub fn label_value(&mut self, label: &str, value: &str) -> Response {
        let theme = self.theme;
        let step = theme.text.body;

        // Seeded with a constant for the same reason `text_row` is: the value
        // side is usually a live number, and hashing it would hand the row a new
        // id every frame.
        let id = self.next_id("label_value");
        let row = self.allocate([0.0, theme.control.row_h]);
        let response = self.interact(row, id);

        self.draw_run(row, row.x, label, step, theme.color.foreground);
        let value_x = row.max_x() - step.width(value);
        self.draw_run(row, value_x, value, step, theme.color.muted);
        response
    }

    /// A thin horizontal divider.
    pub fn separator(&mut self) -> Response {
        let color = self.theme.color.surface;
        let id = self.next_id("separator");
        let row = self.allocate([0.0, 12.0]);
        let response = self.interact(row, id);
        self.painter
            .rect(Rect::new(row.x, row.y + 4.0, row.w, 1.0), color);
        response
    }

    /// Shared body of [`Ui::label`] and [`Ui::label_muted`].
    fn text_row(&mut self, text: &str, color: Color) -> Response {
        let step = self.theme.text.body;
        // Seeded with a constant, not the text: labels routinely show live
        // numbers ("60 fps"), and hashing those would give the row a new id
        // every frame.
        let id = self.next_id("label");
        let row = self.allocate([0.0, self.theme.control.row_h]);
        let response = self.interact(row, id);
        self.draw_run(row, row.x, text, step, color);
        response
    }

    /// Draw one run at `x`, vertically centred in `row`.
    ///
    /// Every text row wants exactly this, and having it in one place is what
    /// stopped each widget from carrying its own vertical fudge factor.
    fn draw_run(&mut self, row: Rect, x: f32, text: &str, step: TypeStep, color: Color) {
        let (px, weight) = step.parts();
        let y = font::centered_top(row.y, row.h, px);
        self.painter.text(x, y, text, px, weight, color);
    }
}
