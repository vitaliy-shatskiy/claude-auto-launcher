use super::*;
use crate::maintenance::FetchFailed;
use crate::self_update::Mode;
use crate::self_update::check::Version;
use crate::self_update::tests::{EXE_URL, SHA_URL, Stub, local, sha_file, tmp};

fn release() -> Release {
    Release {
        version: Version([99, 0, 0]),
        exe_url: EXE_URL.into(),
        sha_url: SHA_URL.into(),
    }
}

/// Fails the rename whose source file name ends with `fail_from`.
struct FailOn(&'static str);

impl Files for FailOn {
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if from.to_string_lossy().ends_with(self.0) {
            return Err(io::Error::other("injected"));
        }
        std::fs::rename(from, to)
    }
}

#[test]
fn a_matching_download_replaces_the_exe_and_keeps_the_old_one() {
    let t = tmp("good");
    let l = local(&t.0, Mode::Notify);
    let net = Stub::new(&[(SHA_URL, Ok(sha_file(&t.0, b"new build"))), (EXE_URL, Ok(b"new build".to_vec()))]);
    let new = download(&l.exe, &release(), &net).unwrap();
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build", "nothing swapped before the swap");
    swap_in(&l.exe, &new, &RealFiles).unwrap();
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"new build");
    assert_eq!(std::fs::read(sibling(&l.exe, ".old")).unwrap(), b"current build");
    assert!(!new.exists());
}

#[test]
fn a_hash_mismatch_deletes_the_download_and_leaves_the_exe() {
    let t = tmp("bad");
    let l = local(&t.0, Mode::Notify);
    let net = Stub::new(&[
        (SHA_URL, Ok(sha_file(&t.0, b"new build"))),
        (EXE_URL, Ok(b"tampered build".to_vec())),
    ]);
    let e = download(&l.exe, &release(), &net).unwrap_err();
    assert!(e.contains("does not match its published SHA-256"), "{e}");
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build");
    assert!(!sibling(&l.exe, ".new").exists(), "the bad download is gone");
    assert!(!sibling(&l.exe, ".old").exists());
}

#[test]
fn a_checksum_that_is_no_sha256_or_fails_to_download_stops_before_the_exe_is_fetched() {
    let t = tmp("nosha");
    let l = local(&t.0, Mode::Notify);
    for sums in [Ok(b"not a hash\n".to_vec()), Err(FetchFailed::Offline)] {
        let net = Stub::new(&[(SHA_URL, sums), (EXE_URL, Ok(b"new build".to_vec()))]);
        assert!(download(&l.exe, &release(), &net).is_err());
        assert_eq!(net.calls(), vec![SHA_URL.to_owned()]);
    }
}

#[test]
fn a_failed_move_in_restores_the_original() {
    let t = tmp("move");
    let l = local(&t.0, Mode::Notify);
    let new = sibling(&l.exe, ".new");
    std::fs::write(&new, b"new build").unwrap();
    let e = swap_in(&l.exe, &new, &FailOn(".new")).unwrap_err();
    assert!(e.contains("the current one was put back"), "{e}");
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build");
    assert!(!sibling(&l.exe, ".old").exists());
}

#[test]
fn rollback_brings_the_previous_exe_back_and_keeps_the_current_as_old() {
    let t = tmp("back");
    let l = local(&t.0, Mode::Notify);
    std::fs::write(sibling(&l.exe, ".old"), b"previous build").unwrap();
    roll_back(&l.exe, &RealFiles).unwrap();
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"previous build");
    assert_eq!(std::fs::read(sibling(&l.exe, ".old")).unwrap(), b"current build");
}

#[test]
fn a_failed_rollback_leaves_the_current_exe() {
    let t = tmp("backfail");
    let l = local(&t.0, Mode::Notify);
    std::fs::write(sibling(&l.exe, ".old"), b"previous build").unwrap();
    assert!(roll_back(&l.exe, &FailOn(".old")).unwrap_err().contains("nothing changed"));
    assert_eq!(std::fs::read(&l.exe).unwrap(), b"current build");
    assert_eq!(std::fs::read(sibling(&l.exe, ".old")).unwrap(), b"previous build");
    let none = tmp("nothing");
    assert!(roll_back(&local(&none.0, Mode::Notify).exe, &RealFiles).is_err(), "no .old");
}

#[test]
fn a_build_directory_exe_is_refused() {
    assert!(refusal(Path::new(r"C:\src\claude-auto\target\release\claude-auto.exe")).is_some());
    assert!(refusal(Path::new(r"C:\Users\me\bin\claude-auto.exe")).is_none());
    let net = Stub::new(&[]);
    assert!(download(Path::new(r"C:\x\target\debug\claude-auto.exe"), &release(), &net).is_err());
    assert!(net.calls().is_empty(), "refused before any request");
}
