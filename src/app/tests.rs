use super::*;

#[test]
fn prefixes_are_shortest_unique() {
    let p = account_prefixes(&["work", "main", "mamoru", "ma"]);
    assert_eq!(p.get("work").map(String::as_str), Some("w"));
    assert_eq!(p.get("main").map(String::as_str), Some("mai"));
    assert_eq!(p.get("mamoru").map(String::as_str), Some("mam"));
    assert_eq!(
        p.get("ma").map(String::as_str),
        Some("ma"),
        "a key that prefixes another keeps itself"
    );
}

#[test]
fn answers_resolve_by_full_key_or_longest_prefix() {
    let p = account_prefixes(&["work", "personal"]);
    assert_eq!(resolve_account_answer(" Personal ", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("p", &p, "work"), "personal");
    assert_eq!(resolve_account_answer("", &p, "work"), "work");
    assert_eq!(resolve_account_answer("x", &p, "work"), "work");
}
