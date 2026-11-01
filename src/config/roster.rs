//! The account roster (`ConvertTo-LauncherRoster`): one entry per account, archived ones
//! dropped, and any structural failure falls back to the single default account.

use serde_json::Value;

use super::{ALLOWED_TINTS, PathCtx, is_rooted_raw, ps_str, ps_truthy, to_bool, trim_end_sep};

/// One account of the roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub key: String,
    pub root: String,
    pub label: String,
    pub tint: String,
    pub hidden: bool,
    pub canonical: bool,
}

fn canonical_root(ctx: &PathCtx) -> String {
    format!("{}\\.claude", ctx.home.to_string_lossy())
}

pub(super) fn default_account(ctx: &PathCtx) -> Account {
    Account {
        key: "work".into(),
        root: canonical_root(ctx),
        label: "work account".into(),
        tint: "Green".into(),
        hidden: false,
        canonical: true,
    }
}

struct RawAccount {
    account: Account,
    archived: bool,
    rooted: bool,
    raw_root: String,
}

fn parse_account(a: &Value, ctx: &PathCtx, warnings: &mut Vec<String>) -> RawAccount {
    let field = |n: &str| a.as_object().and_then(|o| o.get(n));
    let key = field("key").map(ps_str).unwrap_or_default().trim().to_owned();
    let raw_root = field("root").map(ps_str).unwrap_or_default();
    let rooted = is_rooted_raw(&raw_root);
    let root = ctx.expand(&raw_root);
    let canon = canonical_root(ctx);
    let canonical = trim_end_sep(&root).eq_ignore_ascii_case(trim_end_sep(&canon));
    // Fail-safe: the key present and anything but a false value means archived.
    let has_archived = a.as_object().is_some_and(|o| o.contains_key("archived"));
    let (mut archived, w) = to_bool(field("archived"), has_archived, &format!("account '{key}': archived"));
    warnings.extend(w);
    if archived && canonical {
        warnings.push(format!(
            "account '{key}': archived is refused for the canonical account ({canon}); kept live"
        ));
        archived = false;
    }
    let tint_raw = if ps_truthy(field("tint")) {
        field("tint").map(ps_str).unwrap_or_default()
    } else {
        "Green".into()
    };
    let tint = if let Some(t) = ALLOWED_TINTS.iter().find(|t| t.eq_ignore_ascii_case(&tint_raw)) {
        (*t).to_owned()
    } else {
        if !archived {
            warnings.push(format!(
                "account '{key}': tint '{tint_raw}' is not one of {}; using Green",
                ALLOWED_TINTS.join(", ")
            ));
        }
        "Green".to_owned()
    };
    let (hidden, hw) = to_bool(field("hidden"), false, &format!("account '{key}': hidden"));
    if !archived {
        warnings.extend(hw);
    }
    let label = if ps_truthy(field("label")) {
        field("label").map(ps_str).unwrap_or_default()
    } else {
        format!("{key} account")
    };
    RawAccount {
        account: Account {
            key,
            root,
            label,
            tint,
            hidden,
            canonical,
        },
        archived,
        rooted,
        raw_root,
    }
}

/// `ConvertTo-LauncherRoster`: any structural failure returns the default roster.
pub fn parse_roster(raw: &Value, ctx: &PathCtx) -> (Vec<Account>, Vec<String>) {
    let mut warnings = Vec::new();
    let items: Vec<&Value> = match raw {
        Value::Array(a) => a.iter().collect(),
        other => vec![other],
    };
    let parsed: Vec<RawAccount> = items.iter().map(|a| parse_account(a, ctx, &mut warnings)).collect();
    let live: Vec<&RawAccount> = parsed.iter().filter(|p| !p.archived).collect();
    let mut seen_tints: Vec<String> = Vec::new();
    for p in &live {
        let t = &p.account.tint;
        if seen_tints.iter().any(|s| s.eq_ignore_ascii_case(t)) {
            continue;
        }
        let group: Vec<&str> = live
            .iter()
            .filter(|q| q.account.tint.eq_ignore_ascii_case(t))
            .map(|q| q.account.key.as_str())
            .collect();
        if group.len() > 1 {
            warnings.push(format!("accounts {} share tint '{t}'", group.join(", ")));
        }
        seen_tints.push(t.clone());
    }
    if let Some(fatal) = roster_fatal(&parsed, ctx) {
        warnings.push(format!("accounts: {fatal}; using the default roster"));
        return (vec![default_account(ctx)], warnings);
    }
    (parsed.into_iter().filter(|p| !p.archived).map(|p| p.account).collect(), warnings)
}

fn roster_fatal(parsed: &[RawAccount], ctx: &PathCtx) -> Option<String> {
    if parsed.is_empty() {
        return Some("accounts is empty".into());
    }
    let bad_len: Vec<String> = parsed
        .iter()
        .filter(|p| p.account.key.is_empty() || p.account.key.chars().count() > 8)
        .map(|p| format!("'{}'", p.account.key))
        .collect();
    if !bad_len.is_empty() {
        return Some(format!("account key(s) {} must be 1 to 8 characters", bad_len.join(", ")));
    }
    for p in parsed {
        let n = parsed.iter().filter(|q| q.account.key.eq_ignore_ascii_case(&p.account.key)).count();
        if n > 1 {
            return Some(format!(
                "account key '{}' is used {n} times; account keys must be unique",
                p.account.key
            ));
        }
    }
    let bad_root: Vec<String> = parsed
        .iter()
        .filter(|p| !p.rooted)
        .map(|p| format!("'{}': '{}'", p.account.key, p.raw_root))
        .collect();
    if !bad_root.is_empty() {
        return Some(format!("account root(s) {} must be an absolute path", bad_root.join(", ")));
    }
    if parsed.iter().filter(|p| p.account.canonical).count() != 1 {
        let roots: Vec<String> = parsed.iter().map(|p| format!("'{}': '{}'", p.account.key, p.raw_root)).collect();
        return Some(format!(
            "exactly one account must have root {} (the canonical one); accounts have root(s) {}",
            canonical_root(ctx),
            roots.join(", ")
        ));
    }
    None
}
