//! Markdown markers out of a preview line: the card shows what a reply says, not how it was
//! formatted. Exports `plain`. Works on text `clean_text` already folded onto one line.

use std::sync::LazyLock;

use regex::Regex;

use crate::theme::re;

/// A heading, list or quote marker opening the text. Mid-text ` - ` and `1. ` stay: once the
/// newlines are folded they cannot be told from a dash or a number.
static LEAD: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:#{1,6}|[-*+]|\d{1,3}[.)]|>)\s+"));
/// A folded `## heading`; a single `#` mid-text stays (`issue # 5`).
static HEADING: LazyLock<Regex> = LazyLock::new(|| re(r"(^|\s)#{2,6}\s+"));
/// A fence mark with its language tag.
static FENCE: LazyLock<Regex> = LazyLock::new(|| re(r"```[\w+.#-]* ?"));
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`]+)`"));
static LINK: LazyLock<Regex> = LazyLock::new(|| re(r"!?\[([^\]]+)\]\([^)\s]*\)"));
/// Emphasis needs a non-word neighbour outside and no space inside, so `2*3`, `a_b_c`, `*.rs`
/// and a lone `*` are not markup. Each has the same three groups: before, inside, after.
static EMPHASIS: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        re(r"(^|[^\w*])\*\*([^\s*]|[^\s*].*?\S)\*\*($|[^\w*])"),
        re(r"(^|[^\w_])__([^\s_]|[^\s_].*?\S)__($|[^\w_])"),
        re(r"(^|[^\w*])\*([^\s*]|[^\s*][^*]*?\S)\*($|[^\w*])"),
        re(r"(^|[^\w_])_([^\s_]|[^\s_][^_]*?\S)_($|[^\w_])"),
    ]
});

/// The text with markdown markers removed, whitespace runs folded to one space.
pub(super) fn plain(text: &str) -> String {
    let text = LEAD.replace(text, "");
    let text = HEADING.replace_all(&text, "$1");
    let text = FENCE.replace_all(&text, "");
    let text = LINK.replace_all(&text, "$1");
    // Code spans keep their content verbatim: `*.rs` inside one is not emphasis.
    let mut out = String::new();
    let mut at = 0;
    for c in CODE.captures_iter(&text) {
        let (Some(whole), Some(inner)) = (c.get(0), c.get(1)) else {
            continue;
        };
        out.push_str(&unemphasise(text.get(at..whole.start()).unwrap_or("")));
        out.push_str(inner.as_str());
        at = whole.end();
    }
    out.push_str(&unemphasise(text.get(at..).unwrap_or("")));
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Emphasis markers dropped. A match consumes its outer neighbour, so `**a** **b**` needs a
/// second pass; every pass that changes the text shortens it, so the loop ends.
fn unemphasise(text: &str) -> String {
    let mut s = text.to_owned();
    for r in EMPHASIS.iter() {
        loop {
            let next = r.replace_all(&s, "$1$2$3").into_owned();
            if next == s {
                break;
            }
            s = next;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::plain;

    #[test]
    fn markdown_markers_are_dropped_and_lookalikes_stay() {
        let cases = [
            ("**Закрыто:** CS-1561", "Закрыто: CS-1561"),
            ("a __bold__ word", "a bold word"),
            ("an *em* and _em_ pair", "an em and em pair"),
            ("**a** **b**", "a b"),
            ("run `cargo test` now", "run cargo test now"),
            ("see `a_b_` and `*.rs`", "see a_b_ and *.rs"),
            ("```rust fn main() {} ``` done", "fn main() {} done"),
            ("``` plain ```", "plain"),
            ("# Title here", "Title here"),
            ("### Plan", "Plan"),
            ("Intro. ## Next steps", "Intro. Next steps"),
            ("- **Закрыто:** CS-1561", "Закрыто: CS-1561"),
            ("* item", "item"),
            ("1. first step", "first step"),
            ("read [the docs](https://x.y/z) first", "read the docs first"),
            ("a * b", "a * b"),
            ("*", "*"),
            ("-", "-"),
            ("a_b_c and my_var_name", "a_b_c and my_var_name"),
            ("glob *.rs and *.toml", "glob *.rs and *.toml"),
            (r"C:\Users\dev\src\_x\file.rs", r"C:\Users\dev\src\_x\file.rs"),
            ("2*3 = 6 and 2 * 3 * 4", "2*3 = 6 and 2 * 3 * 4"),
            ("done - fixed - shipped", "done - fixed - shipped"),
            ("C# and #5", "C# and #5"),
        ];
        for (input, want) in cases {
            assert_eq!(plain(input), want, "input: {input}");
        }
    }
}
