use super::*;
use crate::theme::{TONE_USER, add_tone_color};

/// The marker-free text of `body_lines`.
fn shown(text: &str, width: usize) -> Vec<String> {
    body_lines(text, width, 40, false, None)
        .iter()
        .map(|l| add_tone_color(l, false).replace([DIM_OPEN, DIM_CLOSE], ""))
        .collect()
}

const RICH: &str = "## Plan\n\nFirst paragraph with **bold words** and `cargo test` inline, long enough to wrap across the pane at a narrow width.\nSecond line of it.\n\n- **r rename swap.** The update moves the exe aside first\n- glob src/**/*.rs and __init__.py stay\n  - nested item\n\n1. first step\n2. second step that is quite a bit longer than the first one\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\n| a | b |\n|---|---|\n\nDone.";

#[test]
fn a_rich_message_keeps_its_structure_at_140_and_80_columns() {
    for (width, name) in [(140, "chat-markdown-140"), (80, "chat-markdown-80")] {
        let got = shown(RICH, width).join("\n");
        let path = format!("{}/tests/fixtures/wide-frames/{name}.txt", env!("CARGO_MANIFEST_DIR"));
        if std::env::var_os("CLAUDE_AUTO_BLESS").is_some() {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        assert_eq!(want, got, "{name}");
    }
}

#[test]
fn markers_never_print_and_lookalikes_stay() {
    let all = shown(RICH, 80).join("\n");
    assert!(
        !all.contains("**b") && !all.contains("**r") && !all.contains('`') && !all.contains("##"),
        "{all}"
    );
    assert!(all.contains("src/**/*.rs") && all.contains("__init__.py"), "{all}");
    assert!(!all.contains("```"), "fence lines dropped: {all}");
}

#[test]
fn emphasis_pairs_drop_their_markers_and_lookalikes_stay() {
    let flat = |t: &str| inline(t).into_iter().map(|(s, _)| s).collect::<String>();
    let cases = [
        ("**Закрито:** CS-1561", "Закрито: CS-1561"),
        ("a __bold__ word", "a bold word"),
        ("an *em* and _em_ pair", "an em and em pair"),
        ("**a** **b**", "a b"),
        ("run `cargo test` now", "run cargo test now"),
        ("see `a_b_` and `*.rs`", "see a_b_ and *.rs"),
        ("read [the docs](https://x.y/z) first", "read the docs first"),
        ("a * b", "a * b"),
        ("*", "*"),
        ("a_b_c and my_var_name", "a_b_c and my_var_name"),
        ("glob *.rs and *.toml", "glob *.rs and *.toml"),
        (r"C:\Users\dev\src\_x\file.rs", r"C:\Users\dev\src\_x\file.rs"),
        ("2*3 = 6 and 2 * 3 * 4", "2*3 = 6 and 2 * 3 * 4"),
        ("src/**/*.rs and **/*.toml", "src/**/*.rs and **/*.toml"),
        ("edit __init__.py now", "edit __init__.py now"),
        (r"C:\proj\__pycache__\x.pyc", r"C:\proj\__pycache__\x.pyc"),
        (r"C:\proj\_tmp_ dir", r"C:\proj\_tmp_ dir"),
    ];
    for (input, want) in cases {
        assert_eq!(flat(input), want, "input: {input}");
    }
}

#[test]
fn list_items_get_a_bullet_and_a_hanging_indent() {
    let lines = shown("- one two three four five six seven", 14);
    assert_eq!(lines, ["\u{2022} one two", "  three four", "  five six", "  seven"]);
    assert_eq!(shown("12. a b c d e f", 8), ["12. a b", "    c d", "    e f"]);
    assert_eq!(body_lines("* x", 10, 9, true, None), ["- x"]);
}

#[test]
fn paragraphs_are_one_blank_apart_and_headings_lose_their_hashes() {
    assert_eq!(shown("a\n\n\n\nb\n", 20), ["a", "", "b"]);
    let heading = body_lines("# Title", 20, 9, false, None);
    assert_eq!(heading, [format!("{TONE_BOLD}Title{TONE_CLOSE}")]);
}

#[test]
fn bold_and_code_runs_take_their_tones_and_the_owners_text_keeps_his() {
    let l = body_lines("say **hi** to `x`", 40, 9, false, Some(TONE_USER));
    assert_eq!(
        l,
        [format!(
            "{TONE_USER}say {TONE_CLOSE}{TONE_BOLD}hi{TONE_CLOSE}{TONE_USER} to {TONE_CLOSE}{TONE_CODE}x{TONE_CLOSE}"
        )]
    );
}

#[test]
fn a_long_message_shows_its_start_and_counts_the_rest() {
    let text = (1..=20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let lines = body_lines(&text, 40, 5, false, None);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[0], "line 1");
    assert_eq!(lines[5], format!("{DIM_OPEN}\u{2026} 15 more lines{DIM_CLOSE}"));
}

#[test]
fn a_word_wider_than_the_pane_is_cut_by_cells() {
    assert_eq!(shown("abcdefghij", 4), ["abcd", "efgh", "ij"]);
}
