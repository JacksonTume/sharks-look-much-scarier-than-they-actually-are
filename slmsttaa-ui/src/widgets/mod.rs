//! The widget roster.
//!
//! Each submodule adds an `impl Ui` block, so a widget is one file and adding
//! one touches nothing else. The roster is deliberately short: these are the
//! widgets a real demo hit a wall without, and nothing else. A widget with no
//! roadblock behind it is polish — see the crate ROADMAP's stopping rule.

mod button;
mod slider;
mod text;
