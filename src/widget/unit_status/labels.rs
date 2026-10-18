//! Cached presentation text and monospace alignment for the status panel.
use gettextrs::pgettext;
use glib::Unichar;
use std::{collections::HashMap, sync::LazyLock};

pub(super) const KEY_WIDTH: usize = 15;

struct Label {
    text: String,
    width: usize,
}

// Locale selection is finalized at startup, so neither translation nor CJK
// width calculation belongs in the panel's repeated refresh path.
fn labels() -> &'static HashMap<&'static str, Label> {
    static LABELS: LazyLock<HashMap<&'static str, Label>> = LazyLock::new(|| {
        [
            ("Name:", pgettext("unit status", "Name:")),
            ("Drop in:", pgettext("unit status", "Drop in:")),
            ("Active:", pgettext("unit status", "Active:")),
            ("Duration:", pgettext("unit status", "Duration:")),
            ("Description:", pgettext("unit status", "Description:")),
            ("Loaded:", pgettext("unit status", "Loaded:")),
            ("Follows:", pgettext("unit status", "Follows:")),
            ("Transient:", pgettext("unit status", "Transient:")),
            ("Status:", pgettext("unit status", "Status:")),
            ("Error:", pgettext("unit status", "Error:")),
            ("Device:", pgettext("unit status", "Device:")),
            ("Where:", pgettext("unit status", "Where:")),
            ("What:", pgettext("unit status", "What:")),
            ("Doc:", pgettext("unit status", "Doc:")),
            ("Docs:", pgettext("unit status", "Docs:")),
            ("FD Store:", pgettext("unit status", "FD Store:")),
            ("Memory:", pgettext("unit status", "Memory:")),
            ("Main PID:", pgettext("unit status", "Main PID:")),
            ("CPU:", pgettext("unit status", "CPU:")),
            ("IP:", pgettext("unit status", "IP:")),
            ("Tasks:", pgettext("unit status", "Tasks:")),
            ("Invocation:", pgettext("unit status", "Invocation:")),
            ("Trigger:", pgettext("unit status", "Trigger:")),
            ("Triggers:", pgettext("unit status", "Triggers:")),
            ("TriggeredBy:", pgettext("unit status", "TriggeredBy:")),
            ("Listen:", pgettext("unit status", "Listen:")),
            ("Accepted:", pgettext("unit status", "Accepted:")),
            ("Refused:", pgettext("unit status", "Refused:")),
            ("CGroup:", pgettext("unit status", "CGroup:")),
            ("preset: ", pgettext("unit status", "preset: ")),
            ("min: ", pgettext("unit status", "min: ")),
            ("low: ", pgettext("unit status", "low: ")),
            (
                "low (startup): ",
                pgettext("unit status", "low (startup): "),
            ),
            ("high: ", pgettext("unit status", "high: ")),
            (
                "high (startup): ",
                pgettext("unit status", "high (startup): "),
            ),
            ("max: ", pgettext("unit status", "max: ")),
            (
                "max (startup): ",
                pgettext("unit status", "max (startup): "),
            ),
            ("swap max: ", pgettext("unit status", "swap max: ")),
            (
                "swap max (startup): ",
                pgettext("unit status", "swap max (startup): "),
            ),
            ("zswap max: ", pgettext("unit status", "zswap max: ")),
            (
                "zswap max (startup): ",
                pgettext("unit status", "zswap max (startup): "),
            ),
            ("limit: ", pgettext("unit status", "limit: ")),
            ("available: ", pgettext("unit status", "available: ")),
            ("peak: ", pgettext("unit status", "peak: ")),
            ("swap: ", pgettext("unit status", "swap: ")),
            ("swap peak: ", pgettext("unit status", "swap peak: ")),
            ("zswap: ", pgettext("unit status", "zswap: ")),
            ("yes", pgettext("unit status", "yes")),
            ("no", pgettext("unit status", "no")),
            ("n/a", pgettext("unit status", "n/a")),
        ]
        .into_iter()
        .map(|(key, text)| {
            let width = key_width(&text);
            (key, Label { text, width })
        })
        .collect()
    });
    &LABELS
}

pub(super) fn text(text: &str) -> &str {
    labels()
        .get(text)
        .map(|label| label.text.as_str())
        .unwrap_or(text)
}

pub(super) fn key(text: &str) -> (&str, usize) {
    match labels().get(text) {
        Some(label) => (&label.text, label.width),
        None => (text, key_width(text)),
    }
}

fn key_width(text: &str) -> usize {
    // Rust's format width counts characters; CJK glyphs occupy two cells.
    let (characters, cells) = text.chars().fold((0, 0), |(characters, cells), c| {
        let width = if c.is_zero_width() {
            0
        } else if c.is_wide() {
            2
        } else {
            1
        };
        (characters + 1, cells + width)
    });
    (KEY_WIDTH + characters).saturating_sub(cells)
}

#[cfg(test)]
mod tests {
    use super::key;

    #[test]
    fn status_labels_align_ascii_and_wide_characters() {
        for (text, expected) in [
            ("Name:", "          Name:"),
            ("名称：", "         名称："),
            ("Cafe\u{301}:", "          Cafe\u{301}:"),
            ("🚀:", "            🚀:"),
            ("长标签宽度超过十五列：", "长标签宽度超过十五列："),
        ] {
            let (label, width) = key(text);
            assert_eq!(format!("{label:>width$}"), expected);
        }
    }
}
