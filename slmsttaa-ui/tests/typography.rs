//! Font metrics: the invariants UI Slice 5 introduced, and the one it exists to
//! protect.
//!
//! Typography looks like the least testable thing in the crate — "does it read
//! well" is a judgement, not an assertion. But the *dangerous* half of it is pure
//! arithmetic, and it is dangerous in the specific way this suite is good at
//! catching: a metrics bug makes text land in the wrong place, which a screenshot
//! shows and a test can pin exactly.

use slmsttaa_ui::{font, RecordingPainter, Theme, Ui, UiInput, UiState, Weight};

/// Every size in the default type scale, so a claim proved here is proved for
/// everything the toolkit actually draws.
fn scale() -> Vec<(f32, Weight)> {
    let t = Theme::dark();
    [t.text.small, t.text.body, t.text.section, t.text.title]
        .iter()
        .map(|s| s.parts())
        .collect()
}

#[test]
fn advances_are_proportional() {
    // The point of the whole slice. Under the bitmap font `i` was as wide as `W`,
    // which is the single loudest thing about a debug HUD.
    for (px, weight) in scale() {
        let wide = font::advance('W', px, weight);
        let narrow = font::advance('i', px, weight);
        assert!(
            narrow < wide * 0.6,
            "at {px}pt {weight:?}, 'i' ({narrow}) should be far narrower than 'W' ({wide})"
        );
    }
}

#[test]
fn digits_are_tabular() {
    // The reason a slider readout keeps still while you drag it. Inter's
    // proportional '1' is 37% narrower than its '0', so without this the value's
    // left edge shuffles sideways every time a digit changes — right-aligning it
    // fixes the right edge and makes the wobble *more* obvious, not less.
    for (px, weight) in scale() {
        let widths: Vec<f32> = "0123456789"
            .chars()
            .map(|c| font::advance(c, px, weight))
            .collect();
        for (digit, w) in "0123456789".chars().zip(&widths) {
            assert_eq!(
                *w, widths[0],
                "at {px}pt {weight:?}, '{digit}' is {w} but '0' is {}",
                widths[0]
            );
        }
    }

    // The consequence, stated at the level a caller cares about: two readouts
    // with the same digit count are the same width.
    let (px, weight) = Theme::dark().text.body.parts();
    assert_eq!(
        font::text_width("0.50", px, weight),
        font::text_width("1.11", px, weight)
    );
    assert_eq!(
        font::text_width("-12.00", px, weight),
        font::text_width("-98.76", px, weight)
    );
}

#[test]
fn metrics_scale_linearly_with_size() {
    // One bake serves the whole type scale, which is only true if everything is
    // in em units. A metric that had a constant baked into it would break here.
    let text = "area exponent m";
    let single = font::text_width(text, 1.0, Weight::Regular);
    for px in [8.0, 15.0, 19.0, 24.0, 64.0] {
        let scaled = font::text_width(text, px, Weight::Regular);
        assert!(
            (scaled - single * px).abs() < 0.001,
            "{text} at {px}pt is {scaled}, expected {}",
            single * px
        );
        assert!((font::line_height(px) - font::line_height(1.0) * px).abs() < 0.001);
    }
}

#[test]
fn semibold_is_wider_than_regular() {
    // Cheap, but it is the only assertion that would catch the two weight tables
    // being emitted from the same TTF — a bake bug that looks perfect until you
    // notice headings aren't actually heavier.
    let heading = "Fluvial erosion";
    let regular = font::text_width(heading, 19.0, Weight::Regular);
    let semibold = font::text_width(heading, 19.0, Weight::SemiBold);
    assert!(
        semibold > regular,
        "semibold ({semibold}) should be wider than regular ({regular})"
    );

    // And the default scale actually uses both, or the second bake is dead weight
    // in every wasm bundle.
    let t = Theme::dark();
    assert_eq!(t.text.body.weight, Weight::Regular);
    assert_eq!(t.text.title.weight, Weight::SemiBold);
    assert_eq!(t.text.section.weight, Weight::SemiBold);
}

#[test]
fn a_space_advances_without_ink() {
    let space = font::glyph(' ', Weight::Regular);
    assert!(!space.has_ink(), "a space should have nothing to draw");
    assert!(space.advance > 0.0, "but it must still move the pen");

    // So a run's width counts its spaces.
    let (px, weight) = (19.0, Weight::Regular);
    assert!(font::text_width("a b", px, weight) > font::text_width("ab", px, weight));
}

#[test]
fn an_unbaked_character_draws_tofu_rather_than_vanishing() {
    // A silently-skipped glyph is a gap in a string that nobody notices until a
    // user types one; a visible box is a bug report. The charset is printable
    // ASCII plus a named handful, so CJK is the obvious out-of-range case.
    let tofu = font::glyph('漢', Weight::Regular);
    assert!(tofu.has_ink(), "an unbaked character must still draw");
    assert_eq!(tofu, font::glyph('□', Weight::Regular));

    // It occupies space, so a run containing one is measured honestly.
    assert!(font::text_width("漢", 19.0, Weight::Regular) > 0.0);
}

#[test]
fn the_named_extras_are_all_baked() {
    // `fit_text` will truncate with '…' when it lands, and the panel reaches for
    // the rest. Each of these is in the atlas by name, so this fails loudly if a
    // re-bake drops one rather than quietly substituting tofu.
    for ch in ['…', '°', '±', '×', '·', '→', '←', '✓', '▶', '■', '□'] {
        let glyph = font::glyph(ch, Weight::Regular);
        assert!(glyph.has_ink(), "{ch:?} should be baked, got tofu");
        if ch != '□' {
            assert_ne!(
                glyph,
                font::glyph('□', Weight::Regular),
                "{ch:?} fell back to tofu"
            );
        }
    }
}

#[test]
fn the_antialiasing_band_narrows_as_text_gets_bigger() {
    // The band is the field distance one screen pixel spans, so a bigger render
    // size needs a narrower one. Getting this inverted gives crisp titles and
    // blurred body text — or the reverse — which is the failure mode a distance
    // field is most prone to.
    let small = font::aa_band(15.0);
    let large = font::aa_band(48.0);
    assert!(
        large < small,
        "48px text ({large}) should need a narrower band than 15px ({small})"
    );

    // Clamped at both ends: no degenerate smoothstep, no fully-washed-out glyph.
    for px in [0.0, 0.001, 1.0, 1000.0, 100_000.0] {
        let band = font::aa_band(px);
        assert!(band > 0.0 && band <= 0.5, "band at {px}px was {band}");
    }
}

#[test]
fn widgets_measure_what_the_painter_draws() {
    // The structural claim of the slice, exercised end to end.
    //
    // `label_value` right-aligns its value by measuring it, and the painter then
    // draws it glyph by glyph from the same table. Through Slice 4 those were two
    // separate implementations of `text_size` and this assertion could not have
    // failed — because the test and the screen were asking the same wrong
    // function. Now the recorded position is measured one way and the ink another.
    let mut painter = RecordingPainter::default();
    let mut state = UiState::default();
    let theme = Theme::dark();
    {
        let mut ui = Ui::new(&mut painter, UiInput::default(), &mut state);
        ui.set_theme(theme);
        ui.panel(slmsttaa_ui::Anchor::TopLeft, theme.panel_w, |ui| {
            ui.label_value("area exponent m", "0.50");
        });
    }

    let runs: Vec<(f32, &str, f32, Weight)> = painter
        .cmds
        .iter()
        .filter_map(|c| match c {
            slmsttaa_ui::DrawCmd::Text {
                x,
                text,
                px,
                weight,
                ..
            } => Some((*x, text.as_str(), *px, *weight)),
            _ => None,
        })
        .collect();

    let (value_x, value, px, weight) = runs[1];
    assert_eq!(value, "0.50");
    // Right edge = left edge + the width the font says, and that has to land on
    // the panel's content edge.
    let right = value_x + font::text_width(value, px, weight);
    let content_edge = theme.panel_w + theme.space.margin - theme.space.pad;
    assert!(
        (right - content_edge).abs() < 0.001,
        "value ends at {right}, content edge is {content_edge}"
    );
}
