use super::*;
use crate::maintenance::FetchFailed;
use crate::self_update::tests::{API, EXE_URL, Stub, release_json, tmp};

const NOW: i64 = 1_800_000_000_000;
const HOUR: i64 = 60 * 60_000;

#[test]
fn version_takes_plain_releases_only() {
    assert_eq!(Version::parse("v1.2.3"), Some(Version([1, 2, 3])));
    assert_eq!(Version::parse("0.10.0"), Some(Version([0, 10, 0])));
    for bad in ["1.2", "1.2.3.4", "1.2.3-beta.1", "1.2.3+build", "01.2.3", "v", "x.y.z"] {
        assert_eq!(Version::parse(bad), None, "{bad}");
    }
    assert!(Version([0, 10, 0]) > Version([0, 9, 9]), "numeric, not text");
}

#[test]
fn newer_never_offers_a_downgrade_or_the_same_version() {
    let rel = |v: [u64; 3]| {
        Some(Release {
            version: Version(v),
            exe_url: String::new(),
            sha_url: String::new(),
        })
    };
    let cur = Some(Version([0, 9, 0]));
    assert!(newer(rel([0, 9, 1]), cur).is_some());
    assert!(newer(rel([0, 9, 0]), cur).is_none());
    assert!(newer(rel([0, 8, 9]), cur).is_none());
    assert!(newer(rel([1, 0, 0]), None).is_none(), "a pre-release build is offered nothing");
}

#[test]
fn a_release_is_refused_for_draft_prerelease_foreign_or_missing_assets() {
    let repo = Repo::default();
    assert_eq!(
        parse_release(&release_json("v99.0.0"), &repo).map(|r| r.version),
        Some(Version([99, 0, 0]))
    );
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v: Value = serde_json::from_slice(&release_json("v99.0.0")).unwrap();
        f(&mut v);
        parse_release(v.to_string().as_bytes(), &repo)
    };
    assert_eq!(edit(&|v| v["prerelease"] = true.into()), None, "pre-release");
    assert_eq!(edit(&|v| v["draft"] = true.into()), None, "draft");
    assert_eq!(edit(&|v| v["tag_name"] = "v99.0.0-rc.1".into()), None, "pre-release tag");
    assert_eq!(
        edit(&|v| v["assets"][0]["browser_download_url"] = EXE_URL.replace("https://", "http://").into()),
        None,
        "http"
    );
    assert_eq!(
        edit(&|v| v["assets"][0]["browser_download_url"] = "https://evil.example/claude-auto.exe".into()),
        None,
        "another host"
    );
    assert_eq!(
        edit(&|v| v["assets"][0]["browser_download_url"] = EXE_URL.replace("vitaliy-shatskiy", "someone-else").into()),
        None,
        "another repo"
    );
    assert_eq!(
        edit(&|v| v["assets"][1]["name"] = "claude-auto.zip".into()),
        None,
        "no checksum asset"
    );
    assert_eq!(parse_release(b"<html>rate limited</html>", &repo), None, "malformed");
}

#[test]
fn a_check_is_cached_for_a_day() {
    let t = tmp("cache");
    let state = t.0.join("state.json");
    let stub = Stub::new(&[(API, Ok(release_json("v99.0.0")))]);
    let repo = Repo::default();
    assert!(latest(&state, &repo, NOW, &stub).is_some());
    assert!(latest(&state, &repo, NOW + 23 * HOUR, &stub).is_some(), "answered from the file");
    assert_eq!(stub.calls().len(), 1);
    let fork = Repo::parse("me/fork").unwrap();
    assert_eq!(latest(&state, &fork, NOW + 23 * HOUR, &stub), None, "another repo asks again");
    assert_eq!(stub.calls().len(), 2);
}

#[test]
fn offline_rate_limited_or_malformed_is_silent_and_retried_after_a_day() {
    let repo = Repo::default();
    let answers = [
        Err(FetchFailed::Offline),
        Err(FetchFailed::Reason("the release channel answered HTTP 403".into())),
        Err(FetchFailed::Reason("the release channel answered HTTP 429".into())),
        Ok(b"{\"tag_name\": 5}".to_vec()),
    ];
    for a in answers {
        let t = tmp("fail");
        let state = t.0.join("state.json");
        let failing = Stub::new(&[(API, a.clone())]);
        assert_eq!(latest(&state, &repo, NOW, &failing), None, "{a:?}");
        assert_eq!(latest(&state, &repo, NOW + 23 * HOUR, &failing), None);
        assert_eq!(failing.calls().len(), 1, "{a:?}: no request again within the day");
        let ok = Stub::new(&[(API, Ok(release_json("v99.0.0")))]);
        assert!(latest(&state, &repo, NOW + 25 * HOUR, &ok).is_some(), "{a:?}: retried the next day");
    }
}
