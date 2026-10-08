use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn spinner(message: impl Into<String>) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();

    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .expect("valid spinner template")
            .tick_strings(&[
                "⠋", "⠙", "⠹", "⠸", "⠼",
                "⠴", "⠦", "⠧", "⠇", "⠏",
            ]),
    );

    spinner.set_message(message.into());
    spinner.enable_steady_tick(Duration::from_millis(80));

    spinner
}
