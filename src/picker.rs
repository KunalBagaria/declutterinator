//! Prompt styling.
//!
//! dialoguer's stock multi-select marks the focused row only by recoloring its
//! text, which disappears as soon as a row carries colors of its own (ours all
//! do). So the pickers here draw an explicit cursor, plain checkboxes, and put
//! the focused row in bold.

use crate::ui;
use console::{strip_ansi_codes, style};
use dialoguer::theme::{ColorfulTheme, Theme};
use std::fmt;

pub struct PickerTheme {
    base: ColorfulTheme,
}

impl PickerTheme {
    pub fn new() -> Self {
        PickerTheme {
            base: ColorfulTheme {
                prompt_prefix: style("⚡".to_string()).for_stderr().color256(ui::YELLOW),
                success_prefix: style("✓".to_string()).for_stderr().color256(ui::GREEN),
                ..ColorfulTheme::default()
            },
        }
    }
}

/// The key help shown above every picker.
pub const KEYS: &str = "↑↓ move   space check/uncheck   a all/none   enter go   esc cancel";

impl Theme for PickerTheme {
    fn format_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.base.format_prompt(f, prompt)
    }

    fn format_error(&self, f: &mut dyn fmt::Write, err: &str) -> fmt::Result {
        self.base.format_error(f, err)
    }

    fn format_confirm_prompt(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        self.base.format_confirm_prompt(f, prompt, default)
    }

    fn format_confirm_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        self.base
            .format_confirm_prompt_selection(f, prompt, selection)
    }

    fn format_multi_select_prompt(&self, f: &mut dyn fmt::Write, prompt: &str) -> fmt::Result {
        self.base.format_multi_select_prompt(f, prompt)
    }

    fn format_multi_select_prompt_selection(
        &self,
        f: &mut dyn fmt::Write,
        prompt: &str,
        selections: &[&str],
    ) -> fmt::Result {
        self.base
            .format_multi_select_prompt_selection(f, prompt, selections)
    }

    fn format_multi_select_prompt_item(
        &self,
        f: &mut dyn fmt::Write,
        text: &str,
        checked: bool,
        active: bool,
    ) -> fmt::Result {
        write!(f, "{}", picker_row(text, checked, active))
    }
}

/// `❯ [✓] text` for the focused row, `  [ ] text` for the rest.
pub fn picker_row(text: &str, checked: bool, active: bool) -> String {
    let cursor = if active {
        style("❯")
            .for_stderr()
            .color256(ui::PINK)
            .bold()
            .to_string()
    } else {
        " ".to_string()
    };
    let checkbox = if checked {
        style("[✓]")
            .for_stderr()
            .color256(ui::GREEN)
            .bold()
            .to_string()
    } else {
        style("[ ]").for_stderr().color256(ui::GREY).to_string()
    };
    let text = if active {
        // the row's own colors would cancel the highlight, so drop them while focused
        style(strip_ansi_codes(text).into_owned())
            .for_stderr()
            .bold()
            .to_string()
    } else {
        text.to_string()
    };
    format!("{cursor} {checkbox} {text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_and_checked_rows_are_distinguishable_without_color() {
        console::set_colors_enabled_stderr(false);
        let label = "\u{1b}[38;5;48mSAFE\u{1b}[0m  Application caches   9.6 GB";
        assert_eq!(
            picker_row(label, true, true),
            "❯ [✓] SAFE  Application caches   9.6 GB"
        );
        assert_eq!(
            picker_row(label, false, true),
            "❯ [ ] SAFE  Application caches   9.6 GB"
        );
        assert!(picker_row(label, true, false).starts_with("  [✓] "));
        assert!(picker_row(label, false, false).starts_with("  [ ] "));
    }
}
