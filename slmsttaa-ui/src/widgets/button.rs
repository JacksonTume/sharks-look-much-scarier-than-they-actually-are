//! Click widgets: a push button and a toggle.
//!
//! Both fire on the press *edge* rather than the held state, so holding the
//! mouse down doesn't retrigger every frame. True press-inside-release-inside
//! semantics would use the `active` id [`Ui::interact`] maintains; the button
//! doesn't yet, because nothing has asked to cancel a click by dragging off it.
//!
//! The button is the second widget to become a **builder**, after the slider —
//! and the first to do it for the reason UI Slice 4 exists. A destructive
//! "reset" is not a different widget from an ordinary button; it is the same
//! widget at a different emphasis, and that is what [`Variant`] says. Bolting on
//! a `button_destructive` would have been two surfaces for one control, which is
//! the trade the slider's builder already refused.

use crate::theme::{Size, Variant};
use crate::{Rect, Response, Ui};

/// A clickable button, configured then shown.
///
/// Built by [`Ui::button`]. Nothing is drawn until [`Button::show`] is called,
/// which is what the `must_use` is guarding.
///
/// ```
/// # use slmsttaa_ui::{Anchor, RecordingPainter, Size, Theme, Ui, UiInput, UiState, Variant};
/// # let (mut p, mut s) = (RecordingPainter::default(), UiState::default());
/// # let mut ui = Ui::new(&mut p, UiInput::default(), &mut s);
/// # ui.panel(Anchor::TopLeft, Theme::default().panel_w, |ui| {
/// // The plain case.
/// if ui.button("new seed").show().clicked { /* reseed */ }
///
/// // One that throws work away, so it is colored to be noticed.
/// if ui.button("reset")
///     .variant(Variant::Destructive)
///     .size(Size::Sm)
///     .show()
///     .clicked
/// { /* back to defaults */ }
/// # });
/// ```
#[must_use = "a button draws nothing until `.show()` is called"]
pub struct Button<'u, 'a> {
    ui: &'u mut Ui<'a>,
    label: &'u str,
    variant: Variant,
    size: Size,
}

impl<'a> Ui<'a> {
    /// Begin a clickable button.
    ///
    /// Returns a [`Button`] to configure; call [`Button::show`] to draw it and
    /// get the [`Response`], whose `clicked` says whether it was pressed.
    pub fn button<'u>(&'u mut self, label: &'u str) -> Button<'u, 'a> {
        Button {
            ui: self,
            label,
            variant: Variant::default(),
            size: Size::default(),
        }
    }
}

impl<'u, 'a> Button<'u, 'a> {
    /// How much emphasis the button carries. Defaults to [`Variant::Primary`].
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    /// How large the button is drawn. Defaults to [`Size::Md`].
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Lay the button out, draw it, and report what the pointer did.
    pub fn show(self) -> Response {
        let Button {
            ui,
            label,
            variant,
            size,
        } = self;
        let theme = *ui.theme();

        let id = ui.next_id(label);
        // The row owns a 4-point trailing gap; the button face is the top of it.
        let face_h = size.face_height(&theme);
        let row = ui.allocate([0.0, face_h + 4.0]);
        let face = Rect::new(row.x, row.y, row.w, face_h);
        let response = ui.interact(face, id);

        let fill = theme.fill(variant, response.hovered);
        let painter = ui.painter();
        painter.fill_rect(face, theme.radius.md, fill);
        // Pressed is a scrim rather than a fourth color per variant: `surface` is
        // a light wash on a dark theme and a dark one on a light theme, so one
        // token darkens or brightens whichever way the theme runs.
        if response.held {
            painter.fill_rect(face, theme.radius.md, theme.color.surface);
        }
        if response.focused {
            painter.stroke_rect(face, theme.radius.md, theme.control.ring, theme.color.ring);
        }

        // Center the label within the button.
        let px = size.text_px(&theme);
        let tw = ui.painter().text_size(label, px)[0];
        let tx = face.x + (face.w - tw) * 0.5;
        let ty = face.y + (face.h - px) * 0.5;
        ui.painter().text(tx, ty, label, px, theme.on_fill(variant));

        response
    }
}

impl Ui<'_> {
    /// A labeled toggle. Edits `value` in place; read [`Response::changed`].
    ///
    /// The whole row is the hit target, not just the box — a 16-point square is
    /// a mean thing to ask anyone to hit.
    pub fn checkbox(&mut self, label: &str, value: &mut bool) -> Response {
        let theme = self.theme;
        let px = theme.text.body;

        let id = self.next_id(label);
        let row = self.allocate([0.0, theme.control.row_h]);
        let mut response = self.interact(row, id);
        let well = Rect::new(row.x, row.y, px, px);

        if response.clicked {
            *value = !*value;
            response.changed = true;
            self.changed = true;
        }

        self.painter
            .fill_rect(well, theme.radius.md, theme.color.surface);
        if *value {
            let tick = if response.hovered {
                theme.color.accent_hover
            } else {
                theme.color.accent
            };
            self.painter
                .fill_rect(well.shrink(3.0), theme.radius.sm, tick);
        }
        if response.focused {
            self.painter.stroke_rect(
                well,
                theme.radius.md,
                theme.control.border,
                theme.color.ring,
            );
        }
        self.painter.text(
            well.max_x() + theme.space.gap,
            row.y,
            label,
            px,
            theme.color.foreground,
        );

        response
    }
}
