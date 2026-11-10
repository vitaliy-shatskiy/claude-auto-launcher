use super::*;
use crate::types::DefaultLabels;

pub(crate) fn four() -> Roster {
    let acct = |k: &str, t: &str, hidden: bool| Account {
        key: k.into(),
        root: format!(r"C:\r\{k}"),
        label: format!("{k} account"),
        tint: t.into(),
        hidden,
        canonical: k == "work",
    };
    Roster::new(
        &[
            acct("work", "Green", false),
            acct("personal", "Magenta", false),
            acct("shared", "Cyan", true),
            acct("low", "Blue", false),
        ],
        true,
        None,
    )
}

fn args(r: &Roster, f: impl FnOnce(&mut LaunchState)) -> String {
    let mut s = r.new_state();
    f(&mut s);
    launch_args(r, &s, None, false, None, "", "").join(" ")
}

#[test]
fn row_order_is_account_remote_model_effort_advisor_permission_mode() {
    let names: Vec<&str> = four().rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names.join(","), "Account,Remote,Model,Effort,Advisor,Permission,Mode");
    assert_eq!(
        four().row(RowName::Account).map(|r| r.values.join(",")),
        Some("work,personal,low".into())
    );
}

#[test]
fn hidden_mode_row_launches_normal() {
    let mut r = four();
    r.apply_rows(&[RowName::Account, RowName::Model, RowName::Effort]);
    let names: Vec<&str> = r.rows.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Account", "Model", "Effort"], "listed rows in their order, the rest hidden");
    let mut s = r.new_state();
    let prefs = crate::prefs::Prefs {
        account: Some("work".into()),
        profiles: [(
            "work".to_owned(),
            serde_json::json!({"Permission": "bypass", "Remote": "off", "Effort": "max"}),
        )]
        .into_iter()
        .collect(),
        ..crate::prefs::Prefs::default()
    };
    crate::prefs::merge_prefs(&mut s, &prefs, &r, 0);
    assert_eq!(
        (s.mode.as_str(), s.permission.as_str(), s.remote.as_str(), s.effort.as_str()),
        ("normal", "default", "on", "max"),
        "a hidden row keeps its default, never a remembered pick"
    );
    assert_eq!(launch_args(&r, &s, None, false, None, "", "").join(" "), "--effort max");
    let mut reordered = four();
    reordered.apply_rows(&[RowName::Mode, RowName::Account]);
    let names: Vec<&str> = reordered.rows.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Mode", "Account"]);
}

#[test]
fn the_action_row_follows_account_and_remote_and_project_adds_no_argument() {
    let mut r = four();
    r.add_action_row();
    let names: Vec<&str> = r.rows.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names.join(","), "Account,Remote,Action,Model,Effort,Advisor,Permission,Mode");
    assert_eq!(
        r.row(RowName::Action).unwrap().values.join(" "),
        "new project continue resume worktree"
    );
    assert!(!PROFILE_FIELDS.contains(&RowName::Action), "never remembered");
    assert_eq!(r.new_state().action, "new", "Enter at once launches new");
    let mut s = r.new_state();
    s.action = PROJECT_ACTION.into();
    assert_eq!(launch_args(&r, &s, None, false, None, "", "").join(" "), "");
    let mut only = four();
    only.apply_rows(&[RowName::Model, RowName::Account]);
    only.add_action_row();
    let names: Vec<&str> = only.rows.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, ["Model", "Account", "Action"], "after account wherever rows put it");
}

#[test]
fn default_state_adds_no_arguments() {
    assert_eq!(args(&four(), |_| {}), "");
}

#[test]
fn each_row_maps_to_its_flag() {
    let r = four();
    assert_eq!(args(&r, |s| s.model = "opus1m".into()), "--model opus[1m]");
    assert_eq!(args(&r, |s| s.model = "sonnet1m".into()), "--model sonnet[1m]");
    assert_eq!(args(&r, |s| s.permission = "bypass".into()), "--permission-mode bypassPermissions");
    assert_eq!(args(&r, |s| s.advisor = "off".into()), "", "off is an environment variable");
    assert_eq!(args(&r, |s| s.action = "continue".into()), "-c");
    assert_eq!(args(&r, |s| s.action = "worktree".into()), "-w");
    assert_eq!(args(&r, |s| s.action = "resume".into()), "", "resume without an id adds nothing");
    assert_eq!(
        args(&r, |s| {
            s.model = "opus1m".into();
            s.effort = "ultracode".into();
            s.advisor = "fable".into();
            s.permission = "plan".into();
            s.mode = "safe".into();
        }),
        "--model opus[1m] --effort ultracode --advisor fable --permission-mode plan --safe-mode"
    );
}

#[test]
fn resume_carries_id_and_fork() {
    let r = four();
    let mut s = r.new_state();
    s.action = "resume".into();
    assert_eq!(
        launch_args(&r, &s, Some("abc123"), true, None, "", "").join(" "),
        "--resume abc123 --fork-session"
    );
}

#[test]
fn default_pins_opus_when_account_lacks_fable() {
    let r = four();
    let s = r.new_state();
    let none: Vec<String> = Vec::new();
    assert_eq!(
        launch_args(&r, &s, None, false, Some(&none), "Fable 5.1[1M]", "").join(" "),
        "--model opus[1m]"
    );
    assert_eq!(
        launch_args(&r, &s, None, false, None, "Fable 5.1[1M]", "").join(" "),
        "",
        "no information pins nothing"
    );
    let fable = vec!["fable".to_owned()];
    assert_eq!(launch_args(&r, &s, None, false, Some(&fable), "Fable 5.1[1M]", "").join(" "), "");
}

#[test]
fn a_hidden_model_row_still_pins_the_unavailable_default() {
    let mut r = four();
    r.apply_rows(&[RowName::Account, RowName::Effort]);
    let none: Vec<String> = Vec::new();
    assert_eq!(
        launch_args(&r, &r.new_state(), None, false, Some(&none), "Fable 5.1[1M]", "").join(" "),
        "--model opus[1m]"
    );
}

#[test]
fn step_wraps_and_skips_hidden_fable() {
    let r = four();
    let mut s = r.new_state();
    s.row = 2;
    let none: Vec<String> = Vec::new();
    let labels = DefaultLabels::new();
    let ctx = OptionText::plain(Some(&none), &labels);
    s.model = "opus1m".into();
    step_launch_value(&r, &mut s, 1, &ctx);
    assert_eq!(s.model, "default", "fable is hidden on this account; wraps");
    step_launch_value(&r, &mut s, -1, &ctx);
    assert_eq!(s.model, "opus1m", "wraps backwards over the hidden fable");
}

#[test]
fn folded_twin_takes_the_default_slot_and_stepping_onto_it_stores_the_twin() {
    let r = four();
    let labels: DefaultLabels = [("Effort".to_owned(), "high".to_owned())].into();
    let ctx = OptionText::plain(None, &labels);
    let row = r.row(RowName::Effort).unwrap();
    assert_eq!(
        visible_row_values(&r, row, &ctx).join(" "),
        "low medium default xhigh max ultracode"
    );
    let mut s = r.new_state();
    s.row = 3;
    s.effort = "medium".into();
    step_launch_value(&r, &mut s, 1, &ctx);
    assert_eq!(s.effort, "high");
    assert!(is_hidden_row_value(&r, row, "high", &ctx));
}

#[test]
fn snap_resets_hidden_family_and_its_mark() {
    let mut s = four().new_state();
    s.model = "fable".into();
    s.restored = vec!["Model".into(), "Effort".into()];
    availability_snap(&mut s, Some(&[]));
    assert_eq!(s.model, "default");
    assert_eq!(s.restored, ["Effort"]);
}

#[test]
fn honest_label_names_the_pin() {
    let r = four();
    assert_eq!(
        honest_default_label(&r, "default (Fable 5.1[1M])", Some(&[]), RowName::Model),
        "default (Opus 5.5[1M])"
    );
    assert_eq!(
        honest_default_label(&r, "fable", Some(&[]), RowName::Advisor),
        "opus",
        "CS-1044: the advisor pin, bare like the label came in"
    );
    assert_eq!(
        honest_default_label(&r, "default (fable)", Some(&[]), RowName::Advisor),
        "default (opus)"
    );
}

#[test]
fn family_match_catches_non_canonical_settings_ids() {
    for id in [
        "fable",
        "fable5",
        "fable_1m",
        "claude-fable-5-1[1m]",
        "default (fable5)",
        "Fable 5.1[1M]",
    ] {
        assert!(default_model_unavailable(id, Some(&[])), "{id} names fable (CS-232a)");
    }
    assert!(!default_model_unavailable("opus", Some(&[])));
    assert!(!default_model_unavailable("fable5", Some(&["fable".to_owned()])));
    assert!(!default_model_unavailable("fable5", None), "no information hides nothing");
}

#[test]
fn default_advisor_pins_opus_when_account_lacks_fable() {
    let r = four();
    let s = r.new_state();
    let none: Vec<String> = Vec::new();
    let run = |avail: Option<&[String]>, adv: &str| launch_args(&r, &s, None, false, avail, "", adv).join(" ");
    assert_eq!(run(Some(&none), "fable5"), "--advisor opus");
    assert_eq!(run(None, "fable"), "", "no information pins nothing");
    assert_eq!(run(Some(&["fable".to_owned()]), "fable"), "");
    assert_eq!(run(Some(&none), "opus"), "");
    let mut off = r.new_state();
    off.advisor = "off".into();
    assert_eq!(
        launch_args(&r, &off, None, false, Some(&none), "", "fable").join(" "),
        "",
        "off stays off"
    );
}

#[test]
fn launch_mode_table_is_valid_and_case_insensitive() {
    assert_eq!(
        launch_mode_table_errors(&LAUNCH_MODES, &PROJECT_SCREEN_OWN_KEYS),
        [] as [std::string::String; 0]
    );
    assert_eq!(launch_mode("RESUME").map(|m| m.flag), Some("--resume"));
    let clash = [LaunchMode {
        hotkey: Some('w'),
        ..LAUNCH_MODES[1]
    }];
    assert_eq!(launch_mode_table_errors(&clash, &PROJECT_SCREEN_OWN_KEYS).len(), 1);
}

#[test]
fn the_hide_keys_collide_with_no_project_screen_or_launch_mode_key() {
    for k in ['h', 'v'] {
        assert!(PROJECT_SCREEN_OWN_KEYS.contains(&k), "{k} is reserved on the project screen");
        assert!(LAUNCH_MODES.iter().all(|m| m.hotkey != Some(k)), "{k} is no launch-mode hotkey");
        assert_eq!(PROJECT_SCREEN_OWN_KEYS.iter().filter(|c| **c == k).count(), 1, "{k} is one key");
    }
    let h = [LaunchMode {
        hotkey: Some('h'),
        ..LAUNCH_MODES[1]
    }];
    assert_eq!(
        launch_mode_table_errors(&h, &PROJECT_SCREEN_OWN_KEYS).len(),
        1,
        "a mode on h is refused"
    );
}

#[test]
fn model_labels_from_catalog_keep_sonnet_bracketless() {
    let mut r = four();
    r.set_model_labels(
        &[
            ("opus".to_owned(), "Opus 6".to_owned()),
            ("sonnet".to_owned(), "Sonnet 6".to_owned()),
        ]
        .into(),
    );
    let row = r.row(RowName::Model).unwrap();
    assert_eq!(row.labels.get("opus1m").map(String::as_str), Some("Opus 6[1M]"));
    assert_eq!(row.labels.get("sonnet1m").map(String::as_str), Some("Sonnet 6"));
}
