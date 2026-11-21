//! Keys, clicks, pastes and the confirm over a scripted terminal.

use super::*;

#[test]
fn ctrl_c_and_escape_leave() {
    let ctrl_c = Event::Key(KeyPress {
        vk: vk::C,
        ctrl: true,
        ch: '\u{3}',
        ..KeyPress::default()
    });
    assert_eq!(go(vec![ctrl_c], &[], vec![], 0, (100, 24)).3, Ok(None));
    assert_eq!(go(vec![esc()], &[], vec![], 0, (100, 24)).3, Ok(None));
}

#[test]
fn clicking_esc_back_leaves_and_one_column_past_it_does_not() {
    let m = map_at((100, 24), &reindex());
    let e = footer_span(&m, "Escape");
    let y = m.footer_y.unwrap() + i32::try_from(e.line).unwrap();
    assert_eq!(go(vec![click(e.start, y, false)], &reindex(), vec![], 0, (100, 24)).3, Ok(None));
    assert_eq!(
        go(vec![click(e.end + 1, y, false)], &reindex(), vec![], 0, (100, 24)).3,
        Err(UiStop::KeysExhausted)
    );
}

#[test]
fn hovering_buttons_paints_three_distinct_frames() {
    let m = map_at((100, 24), &reindex());
    let y = m.footer_y.unwrap();
    let mv = |s: &crate::frame::FooterSpan| {
        Event::Mouse(MouseEvent {
            x: s.start,
            y: y + i32::try_from(s.line).unwrap(),
            is_move: true,
            ..MouseEvent::default()
        })
    };
    let (_, _, t, _) = go(
        vec![mv(&footer_span(&m, "Enter")), mv(&footer_span(&m, "Escape")), esc()],
        &reindex(),
        vec![],
        0,
        (100, 24),
    );
    assert_eq!(t.frames.len(), 3);
    assert_ne!(t.frames[0], t.frames[1]);
    assert_ne!(t.frames[1], t.frames[2]);
    assert!(
        t.frames[1].contains(crate::theme::ACCENT_BG),
        "the hovered button gets the accent cap"
    );
    assert!(!t.frames[0].contains(crate::theme::ACCENT_BG));
}

#[test]
fn debug_mode_draws_the_maintenance_screen_exactly_as_off() {
    let ev = || vec![key('i'), key('i'), esc()];
    let on = crate::timing::Timings::on(std::time::Instant::now());
    let (_, _, t_on, _) = go_timed(ev(), &reindex(), vec![], 0, (100, 24), on);
    let (_, _, t_off, _) = go(ev(), &reindex(), vec![], 0, (100, 24));
    assert!(t_off.frames.len() > 1, "the screen painted before and after the command");
    assert_eq!(t_on.frames, t_off.frames);
}

#[test]
fn one_press_only_warns_the_second_runs_once() {
    let (log, status, _, _) = go(vec![key('i'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 0);
    assert!(status.contains("confirm: press i"));
    let (log, status, t, _) = go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 1);
    assert!(status.contains("full reindex") && status.contains("finished green"), "{status}");
    assert_eq!(t.drains, 1, "the input queue is drained after a command that owned the screen");
    assert!(
        t.frames.iter().any(|f| crate::theme::remove_ansi_color(f).contains("running")),
        "a running frame first"
    );
}

#[test]
fn a_click_selects_a_row_and_enter_runs_it() {
    let m = map_at((100, 24), &[]);
    let d = m.rows.iter().find(|r| r.name == "d").unwrap();
    let enter = Event::Key(KeyPress::named(vk::ENTER));
    let (log, status, ..) = go(vec![click(4, d.y, false), enter, esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1, "{status}");
    let (log, ..) = go(vec![click(4, d.y, false), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0, "a click alone runs nothing");
}

#[test]
fn beside_the_rows_a_wide_page_shows_what_the_selected_action_does_and_how_to_run_it() {
    let mut ops = FakeOps {
        log: Rc::new(RefCell::new(Log::default())),
        exit_code: 0,
    };
    let actions = reindex();
    let mut m = MaintScreen::new(
        &mut ops,
        &actions,
        Style {
            color: false,
            ascii: false,
        },
    );
    let text = |m: &mut MaintScreen<'_>, at: i32| {
        *m.cursor() = Cursor::at(at);
        m.before();
        m.draw((211, 52)).lines.join("\n")
    };
    let first = text(&mut m, 0);
    assert!(
        first.contains("─ update ─") && first.contains("downloads the newest Claude Code"),
        "{first}"
    );
    assert!(first.contains("enter  or  u  runs it"), "{first}");
    let doctor = m.page.entries.iter().position(|e| e.key == 'd').unwrap();
    let d = text(&mut m, i32::try_from(doctor).unwrap());
    assert!(d.contains("runs claude doctor") && d.contains("enter  or  d  runs it"), "{d}");
    let narrow = {
        *m.cursor() = Cursor::at(0);
        m.before();
        m.draw((100, 30)).lines.join("\n")
    };
    assert!(!narrow.contains("runs it"), "stacked, the details stay off the page: {narrow}");
}

#[test]
fn a_second_click_on_the_selected_row_runs_it_and_a_click_on_another_only_selects() {
    let m = map_at((100, 24), &[]);
    let d = m.rows.iter().find(|r| r.name == "d").unwrap();
    let other = m.rows.iter().find(|r| r.name != "d").unwrap();
    let (log, status, ..) = go(vec![click(4, d.y, false), click(4, d.y, false), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1, "{status}");
    let (log, ..) = go(
        vec![click(4, other.y, false), click(4, d.y, false), esc()],
        &[],
        vec![],
        0,
        (100, 24),
    );
    assert_eq!(log.doctor, 0, "the first click on a row only selects it");
}

#[test]
fn w_and_s_move_the_selection_when_no_row_owns_them() {
    let enter = || Event::Key(KeyPress::named(vk::ENTER));
    // Rows: u, r, p, d - three moves down land on doctor.
    let (log, ..) = go(vec![key('s'), key('s'), key('s'), enter(), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1);
    let four_down_one_up = vec![key('s'), key('s'), key('s'), key('s'), key('w'), enter(), esc()];
    assert_eq!(go(four_down_one_up, &[], vec![], 0, (100, 24)).0.doctor, 1);
}

#[test]
fn exit_codes_reach_the_status() {
    assert!(
        go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 2, (100, 24))
            .1
            .contains("COULD NOT RUN")
    );
    assert!(
        go(vec![key('i'), key('i'), esc()], &reindex(), vec![], 1, (100, 24))
            .1
            .contains("FAILED")
    );
}

#[test]
fn uppercase_and_sgr_terminators_never_press_a_key() {
    let upper = Event::Key(KeyPress::char('I'));
    assert_eq!(go(vec![upper, upper, esc()], &reindex(), vec![], 0, (100, 24)).0.runs.len(), 0);
    let sgr_m = Event::Key(KeyPress {
        ch: 'M',
        vk: 0x4D,
        shift: true,
        ..KeyPress::default()
    });
    let (log, ..) = go(vec![sgr_m, sgr_m, esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0, "two SGR terminators run nothing");
}

#[test]
fn the_cyrillic_letter_on_the_i_key_runs_the_action() {
    let ru_i = Event::Key(KeyPress {
        ch: '\u{0448}',
        vk: 0x49,
        ..KeyPress::default()
    });
    assert_eq!(go(vec![ru_i, ru_i, esc()], &reindex(), vec![], 0, (100, 24)).0.runs.len(), 1);
}

#[test]
fn actions_come_from_the_config() {
    let (_, status, ..) = go(vec![key('i'), esc()], &[], vec![], 0, (100, 24));
    assert!(!status.contains("confirm"), "a key with no action does nothing");
    let mut one_shot = reindex();
    one_shot[0].key = "x".into();
    one_shot[0].label = "quick".into();
    one_shot[0].confirm_twice = false;
    assert!(
        go(vec![key('x'), esc()], &one_shot, vec![], 0, (100, 24))
            .1
            .contains("quick finished green")
    );
}

#[test]
fn the_confirm_is_state_not_a_parse_of_the_status() {
    let (log, status, ..) = go(vec![key('z'), esc()], &reindex(), vec![], 0, (100, 24));
    assert!(log.runs.is_empty() && !status.contains("confirm"));
    let (log, status, ..) = go(vec![key('i'), key('z'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 0);
    assert!(
        !status.contains("confirm"),
        "the stale confirm text is cleared by the unrelated key"
    );
    let (log, status, ..) = go(vec![key('i'), key('p'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!((log.prunes, log.runs.len()), (0, 0), "one confirm never arms another key");
    assert!(status.contains("confirm: press p again"));
    let mut two = reindex();
    two.push(MaintenanceAction {
        key: "x".into(),
        label: "quick".into(),
        script: two[0].script.clone(),
        confirm_twice: true,
    });
    let (log, status, ..) = go(vec![key('i'), key('x'), key('x'), esc()], &two, vec![], 0, (100, 24));
    assert_eq!(log.runs.len(), 1);
    assert!(status.contains("quick finished green"));
}

fn clock(step: u64, n: usize) -> Vec<Option<u64>> {
    (1..=n as u64).map(|i| Some(1000 + i * step)).collect()
}

#[test]
fn a_paste_is_not_two_presses() {
    let (log, status, ..) = go(vec![key('p'), key('p'), esc()], &reindex(), clock(10, 3), 0, (100, 24));
    assert_eq!(log.prunes, 0, "two p 10 ms apart are a paste");
    assert!(status.contains("confirm: press p again"));
    assert_eq!(
        go(vec![key('p'), key('p'), esc()], &reindex(), clock(400, 3), 0, (100, 24))
            .0
            .prunes,
        1
    );
    assert_eq!(
        go(vec![key('i'), key('i'), esc()], &reindex(), clock(10, 3), 0, (100, 24))
            .0
            .runs
            .len(),
        0
    );
    assert_eq!(
        go(vec![key('i'), key('i'), esc()], &reindex(), clock(400, 3), 0, (100, 24))
            .0
            .runs
            .len(),
        1
    );
    let four = vec![key('p'), key('p'), key('p'), key('p'), esc()];
    assert_eq!(
        go(four, &reindex(), clock(10, 5), 0, (100, 24)).0.prunes,
        0,
        "a long paste only re-arms"
    );
    assert_eq!(
        go(vec![key('p'), key('p'), esc()], &reindex(), vec![], 0, (100, 24)).0.prunes,
        1,
        "no stamp leaves the gate inert"
    );
}

#[test]
fn d_is_doctor_and_w_s_are_not_arrows() {
    let (log, status, ..) = go(vec![key('d'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 1);
    assert!(status.contains("doctor report"));
    let (log, status, ..) = go(vec![key('w'), key('s'), esc()], &reindex(), vec![], 0, (100, 24));
    assert_eq!(log.doctor, 0);
    assert!(!status.contains("doctor report"));
    let mut ws = reindex();
    ws[0] = MaintenanceAction {
        key: "s".into(),
        label: "sweep".into(),
        script: ws[0].script.clone(),
        confirm_twice: false,
    };
    ws.push(MaintenanceAction {
        key: "w".into(),
        label: "warm".into(),
        script: ws[0].script.clone(),
        confirm_twice: false,
    });
    assert_eq!(go(vec![key('s'), key('w'), esc()], &ws, vec![], 0, (100, 24)).0.runs.len(), 2);
}

#[test]
fn update_draws_a_running_frame_and_drains() {
    let (log, status, t, _) = go(vec![key('u'), esc()], &[], vec![], 0, (100, 24));
    assert_eq!(log.updates, 1);
    assert!(status.contains("updated"));
    assert!(t.frames.iter().any(|f| crate::theme::remove_ansi_color(f).contains(" u  update")));
    assert!(
        t.frames
            .iter()
            .any(|f| crate::theme::remove_ansi_color(f).contains("updating \u{b7} downloading 45 %")),
        "every line the updater prints reaches the step line"
    );
    assert_eq!(t.drains, 1);
}
