//! Static widgets: headings, labels, and rules. Nothing here reads input.

use crate::theme::*;
use crate::Ui;

impl Ui<'_> {
    /// A bold heading row, underlined with a short accent bar.
    pub fn title(&mut self, text: &str) {
        let y = self.layout.y();
        self.painter.text(CONTENT_X, y, text, TITLE_PX, COL_TEXT);

        // A short accent rule under the title gives the panel a clear header
        // instead of a flat wall of text.
        let tw = self.painter.text_size(text, TITLE_PX)[0];
        self.painter
            .rect(CONTENT_X, y + TITLE_PX + 3.0, tw.max(40.0), 2.0, COL_ACCENT);

        self.layout.advance(TITLE_PX + 12.0);
    }

    /// A section sub-heading: smaller than [`Ui::title`] and accent-colored, for
    /// grouping related widgets within a panel.
    pub fn section(&mut self, text: &str) {
        self.layout.advance(2.0);
        self.painter
            .text(CONTENT_X, self.layout.y(), text, SECTION_PX, COL_SECTION);
        self.layout.advance(SECTION_PX + 6.0);
    }

    /// A plain, full-width text row.
    pub fn label(&mut self, text: &str) {
        self.painter
            .text(CONTENT_X, self.layout.y(), text, TEXT_PX, COL_TEXT);
        self.layout.advance(ROW_H);
    }

    /// A muted text row, for secondary readouts and hints.
    pub fn label_muted(&mut self, text: &str) {
        self.painter
            .text(CONTENT_X, self.layout.y(), text, TEXT_PX, COL_MUTED);
        self.layout.advance(ROW_H);
    }

    /// A thin horizontal divider.
    pub fn separator(&mut self) {
        self.layout.advance(4.0);
        let rule = self.row(1.0);
        self.painter.rect(rule.x, rule.y, rule.w, rule.h, COL_TRACK);
        self.layout.advance(8.0);
    }
}
