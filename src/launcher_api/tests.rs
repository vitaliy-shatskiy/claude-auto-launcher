use serde_json::json;

use super::*;
use crate::prefs::parse_prefs;
use crate::types::LimitRecord;

fn account(key: &str, canonical: bool, hidden: bool) -> Account {
    Account {
        key: key.to_owned(),
        root: format!(r"C:\h\.{key}"),
        label: format!("{key} label"),
        tint: if canonical { "Green" } else { "Magenta" }.to_owned(),
        hidden,
        canonical,
    }
}

fn accounts() -> Vec<Account> {
    vec![
        account("main", true, false),
        account("second", false, false),
        account("old", false, true),
    ]
}

fn roster() -> Roster {
    Roster::new(&accounts(), true, Some("main"))
}

fn parse(line: &str) -> Result<RunRequest, String> {
    parse_run_args(&[line.to_owned()])
}

fn request() -> RunRequest {
    RunRequest {
        account: "main".into(),
        project: std::env::temp_dir().to_string_lossy().into_owned(),
        action: "new".into(),
        model: Some("opus1m".into()),
        effort: Some("high".into()),
        advisor: Some("off".into()),
        permission: Some("auto".into()),
        mode: Some("safe".into()),
        resume_id: None,
        fork: false,
        plugin_rows: BTreeMap::new(),
    }
}

fn limits_without_fable(account: &str) -> Limits {
    let record = LimitRecord {
        five_hour: Some(16),
        available_models: Some(vec!["opus".to_owned()]),
        ..LimitRecord::default()
    };
    [(account.to_owned(), record)].into()
}

fn resolve(req: &RunRequest) -> Result<LaunchState, String> {
    resolve_run(req, &roster(), &Limits::new(), &Prefs::default())
}

#[test]
fn a_json_object_becomes_a_request() {
    let req = parse(
        r#"{"account":"main","project":"C:\\p","action":"resume","model":"opus1m","effort":"high",
            "advisor":"off","permission":"auto","mode":"safe","resumeId":"abc","fork":true}"#,
    )
    .unwrap();
    assert_eq!(req.account, "main");
    assert_eq!(req.project, r"C:\p");
    assert_eq!(req.action, "resume");
    assert_eq!(req.model.as_deref(), Some("opus1m"));
    assert_eq!(req.mode.as_deref(), Some("safe"));
    assert_eq!(req.resume_id.as_deref(), Some("abc"));
    assert!(req.fork);
}

#[test]
fn anything_but_one_well_formed_object_is_refused() {
    for (why, line) in [
        ("not JSON", "--model haiku"),
        ("an array", "[]"),
        ("no account", r#"{"project":"C:\\p","action":"new"}"#),
        ("no project", r#"{"account":"main","action":"new"}"#),
        ("no action", r#"{"account":"main","project":"C:\\p"}"#),
        (
            "an unknown key",
            r#"{"account":"main","project":"C:\\p","action":"new","dangerouslySkip":true}"#,
        ),
        (
            "a wrong type",
            r#"{"account":"main","project":"C:\\p","action":"new","fork":"yes"}"#,
        ),
    ] {
        assert!(parse(line).is_err(), "{why}");
    }
    assert!(parse_run_args(&[]).is_err(), "no argument");
    assert!(
        parse_run_args(&["{}".to_owned(), "{}".to_owned()]).is_err(),
        "a second argument would be a guess"
    );
}

#[test]
fn every_row_of_the_screen_is_required_so_a_dropped_one_cannot_launch_on_a_silent_default() {
    for (key, drop) in [
        ("model", (|r| r.model = None) as fn(&mut RunRequest)),
        ("effort", |r| r.effort = None),
        ("advisor", |r| r.advisor = None),
        ("permission", |r| r.permission = None),
        ("mode", |r| r.mode = None),
    ] {
        let mut req = request();
        drop(&mut req);
        let err = resolve(&req).unwrap_err();
        assert!(err.starts_with(key), "{key}: {err}");
    }
}

#[test]
fn a_row_the_screen_does_not_show_is_refused_and_keeps_its_pin() {
    let mut r = roster();
    r.apply_rows(&[RowName::Account, RowName::Remote, RowName::Model, RowName::Effort]);
    let mut req = request();
    let err = resolve_run(&req, &r, &Limits::new(), &Prefs::default()).unwrap_err();
    assert!(err.starts_with("advisor"), "{err}");
    (req.advisor, req.permission, req.mode) = (None, None, None);
    let state = resolve_run(&req, &r, &Limits::new(), &Prefs::default()).unwrap();
    assert_eq!(state.advisor, "default");
}

#[test]
fn an_unknown_account_stops_the_run_before_claude_starts_on_the_wrong_profile() {
    let mut req = request();
    req.account = "nobody".into();
    let err = resolve(&req).unwrap_err();
    assert!(err.contains("nobody"), "{err}");
}

#[test]
fn an_archived_account_is_off_the_screen_so_it_is_off_the_api() {
    let mut req = request();
    req.account = "old".into();
    assert!(resolve(&req).is_err());
}

#[test]
fn the_project_action_is_refused_because_the_client_already_chose_the_project() {
    let mut req = request();
    req.action = "project".into();
    let err = resolve(&req).unwrap_err();
    assert!(err.starts_with("action"), "{err}");
}

#[test]
fn a_value_outside_its_row_names_the_row() {
    for (key, set) in [
        ("model", (|r, v| r.model = Some(v)) as fn(&mut RunRequest, String)),
        ("effort", |r, v| r.effort = Some(v)),
        ("advisor", |r, v| r.advisor = Some(v)),
        ("permission", |r, v| r.permission = Some(v)),
        ("mode", |r, v| r.mode = Some(v)),
        ("action", |r, v| r.action = v),
    ] {
        let mut req = request();
        set(&mut req, "nonsense".to_owned());
        let err = resolve(&req).unwrap_err();
        assert!(err.starts_with(key), "{key}: {err}");
    }
}

#[test]
fn a_family_the_account_lacks_is_refused_because_the_screen_would_not_offer_it() {
    let mut req = request();
    req.model = Some("fable".into());
    let lacking = limits_without_fable("main");
    let err = resolve_run(&req, &roster(), &lacking, &Prefs::default()).unwrap_err();
    assert!(err.starts_with("model"), "{err}");
    req.model = Some("opus1m".into());
    req.advisor = Some("fable".into());
    let err = resolve_run(&req, &roster(), &lacking, &Prefs::default()).unwrap_err();
    assert!(err.starts_with("advisor"), "{err}");
}

#[test]
fn another_accounts_availability_does_not_hide_a_family_here() {
    let mut req = request();
    req.model = Some("fable".into());
    let state = resolve_run(&req, &roster(), &limits_without_fable("second"), &Prefs::default()).unwrap();
    assert_eq!(state.model, "fable");
}

#[test]
fn a_missing_project_directory_is_refused_because_claude_would_open_in_the_wrong_folder() {
    let mut req = request();
    req.project = std::env::temp_dir()
        .join("claude-auto-no-such-dir-024")
        .to_string_lossy()
        .into_owned();
    let err = resolve(&req).unwrap_err();
    assert!(err.starts_with("project"), "{err}");
}

#[test]
fn resume_needs_a_session_id_and_only_resume_takes_one() {
    let mut req = request();
    req.action = "RESUME".into();
    let err = resolve(&req).unwrap_err();
    assert!(err.starts_with("resumeId"), "{err}");
    req.resume_id = Some("abc-123".into());
    assert_eq!(resolve(&req).unwrap().action, "resume", "the row's own casing");
    req.action = "new".into();
    assert!(resolve(&req).unwrap_err().starts_with("resumeId"), "an id on a new session");
    req.resume_id = None;
    req.fork = true;
    assert!(resolve(&req).unwrap_err().starts_with("fork"), "fork without a session");
}

#[test]
fn a_session_id_that_reads_as_an_option_never_reaches_claude() {
    let mut req = request();
    req.action = "resume".into();
    for bad in ["--dangerously-skip-permissions", "", "a b", "x;y"] {
        req.resume_id = Some(bad.into());
        assert!(resolve(&req).unwrap_err().starts_with("resumeId"), "{bad:?}");
    }
}

#[test]
fn values_take_the_rows_own_casing_because_set_profile_and_launch_args_compare_exactly() {
    let mut req = request();
    req.account = "MAIN".into();
    req.permission = Some("ACCEPTEDITS".into());
    let state = resolve(&req).unwrap();
    assert_eq!(state.account, "main");
    assert_eq!(state.permission, "acceptEdits");
}

#[test]
fn the_state_carries_the_request_and_the_remembered_remote() {
    let prefs = parse_prefs(r#"{"Version":2,"Account":"main","Profiles":{"main":{"Remote":"off","Model":"haiku"}}}"#);
    let req = request();
    let state = resolve_run(&req, &roster(), &Limits::new(), &prefs).unwrap();
    assert_eq!(state.model, "opus1m", "the request wins over the remembered row");
    assert_eq!(state.effort, "high");
    assert_eq!(state.advisor, "off");
    assert_eq!(state.permission, "auto");
    assert_eq!(state.mode, "safe");
    assert_eq!(state.action, "new");
    assert_eq!(state.project, req.project);
    assert_eq!(state.remote, "off", "the remembered row, not the client's opinion of it");
}

struct Options {
    limits: Limits,
    labels: BTreeMap<String, DefaultLabels>,
    prefs: Prefs,
}

impl Options {
    fn new() -> Self {
        Self {
            limits: Limits::new(),
            labels: BTreeMap::new(),
            prefs: Prefs::default(),
        }
    }

    fn json(&self) -> Value {
        options_json(&roster(), &accounts(), &self.limits, &self.labels, &self.prefs, Some("2.1.317"))
    }

    fn main_account(&self) -> Value {
        self.json()["accounts"][0].clone()
    }
}

#[test]
fn the_rows_follow_the_screen_without_account_and_remote_and_the_actions_come_apart() {
    let v = Options::new().json();
    let names: Vec<&str> = v["rows"].as_array().unwrap().iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["model", "effort", "advisor", "permission", "mode"]);
    assert_eq!(v["actions"], json!(["new", "continue", "resume", "worktree"]));
    let mut on_demand = roster();
    on_demand.add_action_row();
    let v = options_json(&on_demand, &accounts(), &Limits::new(), &BTreeMap::new(), &Prefs::default(), None);
    assert_eq!(
        v["rows"][0]["name"], "model",
        "the action row is the actions list, `project` left out"
    );
    assert_eq!(v["actions"], json!(["new", "continue", "resume", "worktree"]));
}

#[test]
fn every_value_on_a_row_carries_the_label_the_screen_draws() {
    let v = Options::new().json();
    let model = &v["rows"][0];
    assert_eq!(model["label"], "model");
    assert_eq!(
        model["values"][0],
        json!({"value": "haiku", "label": "Haiku 5.5"}),
        "no default model value"
    );
    assert_eq!(model["values"][2], json!({"value": "opus1m", "label": "Opus 5.5[1M]"}));
    let permission = &v["rows"][3];
    assert_eq!(permission["values"][3], json!({"value": "acceptEdits", "label": "acceptEdits"}));
}

#[test]
fn the_head_names_the_launcher_and_the_cli_version() {
    let v = Options::new().json();
    assert_eq!(v["launcher"], "claude-auto");
    assert_eq!(v["version"], "2.1.317");
    let none = options_json(&roster(), &accounts(), &Limits::new(), &BTreeMap::new(), &Prefs::default(), None);
    assert_eq!(none["version"], Value::Null, "an unknown version is said, not invented");
}

#[test]
fn accounts_are_those_of_the_screen_with_the_default_marked() {
    let v = Options::new().json();
    let list = v["accounts"].as_array().unwrap();
    assert_eq!(list.len(), 2, "the archived account is not offered");
    let main = &list[0];
    assert_eq!(main["key"], "main");
    assert_eq!(main["label"], "main label");
    assert_eq!(main["tint"], "Green");
    assert_eq!(main["configDir"], r"C:\h\.main");
    assert_eq!(main["default"], true);
    assert_eq!(list[1]["default"], false);
}

#[test]
fn without_data_an_account_has_null_and_empty_objects_rather_than_guesses() {
    let main = Options::new().main_account();
    assert_eq!(main["limitPct"], Value::Null);
    assert_eq!(main["hidden"], json!({}));
    assert_eq!(main["defaultLabels"], json!({}));
    assert_eq!(main["prefs"], json!({}));
}

#[test]
fn the_limit_is_the_five_hour_percent_the_account_tab_shows() {
    let mut o = Options::new();
    o.limits = limits_without_fable("main");
    assert_eq!(o.main_account()["limitPct"], 16);
}

#[test]
fn a_family_the_account_lacks_is_listed_as_hidden_per_row() {
    let mut o = Options::new();
    o.limits = limits_without_fable("main");
    let v = o.json();
    assert_eq!(v["accounts"][0]["hidden"], json!({"model": ["fable"], "advisor": ["fable"]}));
    assert_eq!(v["accounts"][1]["hidden"], json!({}), "the other account answered nothing");
}

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, DefaultLabels> {
    let inner: DefaultLabels = pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    [("main".to_owned(), inner)].into()
}

#[test]
fn default_labels_say_what_default_resolves_to_and_leave_out_the_uninformative() {
    let mut o = Options::new();
    o.labels = labels(&[
        ("Model", "Opus 5.5"),
        ("Effort", "high"),
        ("Advisor", "default"),
        ("Permission", "default"),
    ]);
    assert_eq!(
        o.main_account()["defaultLabels"],
        json!({"effort": "default (high)"}),
        "the model row has no default value to label"
    );
}

#[test]
fn a_default_of_a_lacking_family_is_labelled_by_the_pin_the_launch_will_pass() {
    let mut o = Options::new();
    o.limits = limits_without_fable("main");
    o.labels = labels(&[("Model", "Fable 5.1"), ("Advisor", "fable")]);
    assert_eq!(o.main_account()["defaultLabels"], json!({"advisor": "default (opus)"}));
}

const SAVED: &str = r#"{"Version":2,"Account":"main","Profiles":{"main":{"Model":"fable","Effort":"medium","Advisor":"OFF","Remote":"off","Project":"C:\\p"}}}"#;

#[test]
fn prefs_are_the_rows_the_screen_would_restore_and_never_the_remote() {
    let mut o = Options::new();
    o.prefs = parse_prefs(SAVED);
    assert_eq!(
        o.main_account()["prefs"],
        json!({"model": "fable", "effort": "medium", "advisor": "off", "permission": "default", "mode": "normal"})
    );
}

#[test]
fn a_remembered_family_the_account_lost_is_no_pick_in_prefs() {
    let mut o = Options::new();
    o.prefs = parse_prefs(SAVED);
    o.limits = limits_without_fable("main");
    o.labels = labels(&[("ModelId", "sonnet")]);
    assert_eq!(
        o.main_account()["prefs"]["model"],
        "default",
        "a snapped model is never a remembered one"
    );
}

#[test]
fn default_model_names_what_a_launch_without_a_pick_runs() {
    let mut o = Options::new();
    assert_eq!(o.main_account()["defaultModel"], "opus1m", "no settings model: the pin");
    o.labels = labels(&[("ModelId", "sonnet")]);
    assert_eq!(o.main_account()["defaultModel"], "sonnet1m");
    o.labels = labels(&[("ModelId", "opusplan")]);
    assert_eq!(o.main_account()["defaultModel"], "opusplan", "no family alias: verbatim");
}

#[test]
fn a_run_request_may_ask_for_the_default_model() {
    let mut req = request();
    req.model = Some("default".into());
    assert_eq!(resolve(&req).map(|s| s.model), Ok("default".to_owned()));
}

#[test]
fn a_profile_holding_only_the_remote_has_no_prefs_to_offer() {
    let mut o = Options::new();
    o.prefs = parse_prefs(r#"{"Version":2,"Profiles":{"main":{"Remote":"off"}}}"#);
    assert_eq!(o.main_account()["prefs"], json!({}));
}
