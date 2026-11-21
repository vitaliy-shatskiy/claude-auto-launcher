//! `l`: the launch rows editor opened from maintenance, its saves and its errors.

use super::*;

#[test]
fn l_saves_a_hidden_rows_value_and_names_it_in_the_status() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Effort]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    // Rows: account, effort, then the hidden ones in default order (remote, model, ...).
    let keys = crate::ui::parse_preview_keys("l,s,s,s,d,d,d,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert_eq!(log.borrow().hidden.get(&RowName::Model).map(String::as_str), Some("opus1m"));
    assert!(s.page.status.contains("hidden model -> opus1m"), "{}", s.page.status);
}

#[test]
fn l_edits_the_launch_rows_and_saves_them() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Model, RowName::Mode]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    let keys = crate::ui::parse_preview_keys("l,s,s,Spacebar,Enter,l,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert!(s.page.status.contains("launch rows saved: account, model"), "{}", s.page.status);
    let saved = vec![RowName::Account, RowName::Model];
    assert_eq!(log.borrow().rows, [saved.clone(), saved], "the next open starts from the save");
}

#[test]
fn the_rows_editor_opens_on_the_config_as_it_is_now() {
    let log = Rc::new(RefCell::new(Log {
        config: Some(vec![RowName::Account, RowName::Mode]),
        ..Log::default()
    }));
    let mut ops = FakeOps {
        log: Rc::clone(&log),
        exit_code: 0,
    };
    let mut s = MaintScreen::new(&mut ops, &[], plain());
    let keys = crate::ui::parse_preview_keys("l,Enter,Escape").unwrap();
    let mut t = PreviewTerm::new((100, 30), keys, true);
    assert!(run(&mut s, &mut t).unwrap().is_none());
    assert_eq!(
        log.borrow().rows,
        [vec![RowName::Account, RowName::Mode]],
        "another launcher's save is what the editor starts from"
    );
}

#[test]
fn only_a_plain_l_opens_the_rows_editor() {
    let shifted = Event::Key(KeyPress {
        shift: true,
        ..KeyPress::char('L')
    });
    // psmux drops the Caps Lock flag: a Latin letter then arrives as a bare upper-case char.
    let bare_upper = Event::Key(KeyPress::char('L'));
    for stray in [shifted, bare_upper] {
        let (_, status, _, r) = go(vec![stray, esc()], &[], vec![], 0, (100, 24));
        assert_eq!(r, Ok(None), "Escape left the maintenance screen, no editor took it");
        assert!(!status.contains("Last action"), "{status}");
    }
    let caps = Event::Key(KeyPress {
        caps_lock: true,
        ..KeyPress::char('L')
    });
    for press in [key('l'), caps] {
        let (_, status, _, r) = go(vec![press, esc(), esc()], &[], vec![], 0, (100, 24));
        assert_eq!(r, Ok(None));
        assert!(status.contains("launch rows unchanged"), "{status}");
    }
}

#[test]
fn an_error_inside_the_rows_editor_still_ends_the_screen() {
    // The editor's wait fails (keys exhausted, as a lost console would): the screen's own next
    // wait fails the same way, so the error reaches the caller.
    let (.., r) = go(vec![key('l')], &[], vec![], 0, (100, 24));
    assert_eq!(r, Err(UiStop::KeysExhausted));
}
