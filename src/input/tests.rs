use super::*;

#[test]
fn sgr_press_release_and_motion() {
    let VtOutcome::Mouse(m) = decode_csi("[<0;10;5M") else {
        panic!("not a mouse report")
    };
    assert_eq!((m.x, m.y, m.left, m.is_move), (9, 4, true, false));
    let VtOutcome::Mouse(m) = decode_csi("[<35;100;20M") else {
        panic!("not a mouse report")
    };
    assert!(m.is_move, "bit 32 is motion");
    assert_eq!((m.x, m.y), (99, 19));
    let VtOutcome::Mouse(m) = decode_csi("[<0;1;1m") else {
        panic!("not a mouse report")
    };
    assert!(!m.left, "a release is no press");
    let VtOutcome::Mouse(m) = decode_csi("[<65;3;3M") else {
        panic!("not a mouse report")
    };
    assert!(m.wheel_down());
}

#[test]
fn malformed_sgr_is_swallowed_not_a_key() {
    assert_eq!(decode_csi("[<0;1M"), VtOutcome::Swallow);
    assert_eq!(decode_csi("[<a;1;1M"), VtOutcome::Swallow);
    assert_eq!(decode_csi("[I"), VtOutcome::Swallow, "focus in");
}

#[test]
fn vt_cursor_keys_map() {
    assert_eq!(decode_csi("[A"), VtOutcome::Key(KeyPress::named(vk::UP)));
    assert_eq!(vt_key("OD"), Some(KeyPress::named(vk::LEFT)));
    assert_eq!(vt_key("[4~"), Some(KeyPress::named(vk::END)));
    assert_eq!(vt_key("Ou"), None);
}

#[test]
fn x10_report_decodes_column_bytes() {
    let m = decode_x10(&[u32::from('M'), 32, 32 + 85, 32 + 3]).expect("decodes");
    assert_eq!((m.x, m.y, m.left), (84, 2, true));
    assert_eq!(decode_x10(&[77, 32]), None);
}

#[test]
fn wheel_sign_from_high_word() {
    let up = mouse_from_record(0, 0, 128 << 16, 0x4);
    assert!(up.wheel_up());
    let down = mouse_from_record(0, 0, 0xFF80_0000, 0x4);
    assert!(down.wheel_down());
}

#[test]
fn console_and_sgr_motion_decode_to_the_same_hover() {
    let rec = mouse_from_record(57, 6, 0, MOUSE_MOVED);
    assert!(rec.is_move && !rec.left, "MOUSE_MOVED with no button is a hover");
    let VtOutcome::Mouse(sgr) = decode_csi("[<35;58;7M") else {
        panic!("not a mouse report")
    };
    assert_eq!((sgr.x, sgr.y, sgr.is_move, sgr.left), (rec.x, rec.y, true, false));
}

#[test]
fn hotkey_on_cyrillic_layout_and_guards() {
    let cyr = KeyPress::char('\u{0433}');
    assert!(is_hotkey(&cyr, 'u'));
    let by_vk = KeyPress {
        ch: 'г',
        vk: 0x55,
        ..KeyPress::default()
    };
    assert!(is_hotkey(&by_vk, 'u'));
    let shifted_m = KeyPress {
        ch: 'M',
        vk: 0x4D,
        shift: true,
        ..KeyPress::default()
    };
    assert!(is_hotkey(&shifted_m, 'm'), "case never matters");
    let caps_u = KeyPress {
        ch: 'U',
        vk: 0x55,
        caps_lock: true,
        ..KeyPress::default()
    };
    assert!(is_hotkey(&caps_u, 'u'));
    let ctrl_r = KeyPress {
        ch: '\u{12}',
        vk: 0x52,
        ctrl: true,
        ..KeyPress::default()
    };
    assert!(!is_hotkey(&ctrl_r, 'r'), "Ctrl is never a hotkey");
    assert!(is_hotkey(&KeyPress::named(vk::OEM2), '/'));
    assert!(is_hotkey(&KeyPress::char('.'), '/'), "slash is '.' on both Cyrillic layouts");
    assert!(is_hotkey(&KeyPress::named(0x31), '1'));
}

/// What psmux delivers with Caps Lock on: the flag is gone.
#[test]
fn caps_lock_through_psmux_is_still_the_hotkey() {
    assert!(is_hotkey(&KeyPress::char('R'), 'r'), "Latin: vk 0, 'R', no flags");
    let cyrillic = KeyPress {
        ch: '\u{041A}',
        vk: 0x52,
        shift: true,
        ..KeyPress::default()
    };
    assert!(is_hotkey(&cyrillic, 'r'), "Cyrillic: Shift + K on the R key");
    assert!(is_hotkey(&KeyPress::char('\u{041A}'), 'r'), "Cyrillic, vk 0");
}

#[test]
fn every_layout_letter_is_cyrillic_and_unique() {
    for (k, c) in HOTKEY_LAYOUT {
        assert!(*k == '/' || ('\u{0430}'..='\u{044F}').contains(c), "{k}");
        assert_eq!(HOTKEY_LAYOUT.iter().filter(|(_, x)| x == c).count(), 1);
    }
}

#[test]
fn key_names_round_trip() {
    assert_eq!(parse_key_name("downarrow"), Some(vk::DOWN));
    assert_eq!(parse_key_name("D7"), Some(0x37));
    assert_eq!(parse_key_name("F5"), Some(116));
    assert_eq!(key_name(vk::ENTER), "Enter");
    assert_eq!(key_name(0x52), "R");
    assert_eq!(scripted_key("/"), Some(KeyPress::char('/')));
    assert_eq!(scripted_key("Nope"), None);
}

#[test]
fn a_char_only_control_key_gets_its_virtual_key() {
    // psmux, batching a key with SGR mouse text, writes the key-down as vk 0 + the char.
    for (ch, code) in [(13, vk::ENTER), (9, vk::TAB), (8, vk::BACKSPACE), (27, vk::ESCAPE)] {
        let k = key_from_record(true, 0, ch, 0).expect("a press");
        assert!(k.is(code), "char {ch} -> vk {code}");
    }
    let report_byte = key_from_record(true, 0, u16::from(b'M'), 0).expect("a char");
    assert_eq!((report_byte.vk, report_byte.ch), (0, 'M'), "mouse report text stays text");
    let ctrl_m = key_from_record(true, 0x4D, 13, 0x0008).expect("ctrl+m");
    assert_eq!(ctrl_m.vk, 0x4D, "a real vk is never rewritten");
}

/// Key-downs that arrive at virtual milliseconds after the ESC; a peek or take waits up to its
/// timeout on that clock.
struct Timeline {
    keys: std::collections::VecDeque<(u16, u64)>,
    now: u64,
    reports: usize,
}

impl Timeline {
    fn new(chunks: &[(&str, u64)]) -> Self {
        let keys = chunks
            .iter()
            .flat_map(|(text, at)| text.encode_utf16().map(move |c| (c, *at)))
            .collect();
        Self { keys, now: 0, reports: 0 }
    }

    fn left(&self) -> Vec<u16> {
        self.keys.iter().map(|(c, _)| *c).collect()
    }
}

impl EscSource for Timeline {
    fn peek_down(&mut self, timeout_ms: u64) -> Option<u16> {
        match self.keys.front() {
            Some(&(c, at)) if at <= self.now + timeout_ms => {
                self.now = self.now.max(at);
                Some(c)
            }
            _ => {
                self.now += timeout_ms;
                None
            }
        }
    }

    fn take_down(&mut self, timeout_ms: u32) -> Option<u16> {
        let c = self.peek_down(u64::from(timeout_ms))?;
        self.keys.pop_front();
        Some(c)
    }

    fn mouse_report(&mut self) {
        self.reports += 1;
    }
}

#[test]
fn a_report_split_after_its_esc_is_mouse_not_escape_in_text_mouse_mode() {
    let mut tl = Timeline::new(&[("[<35;10;5M", 20)]);
    let AfterEsc::Sequence(VtOutcome::Mouse(m)) = after_esc(&mut tl, esc_wait_ms(true)) else {
        panic!("a split report became Escape")
    };
    assert_eq!((m.x, m.y, m.is_move), (9, 4, true));
    assert!(tl.left().is_empty(), "no tail char leaks as a key");
    assert_eq!(tl.reports, 1);
}

#[test]
fn a_report_split_after_its_bracket_is_mouse_not_typed_keys_in_text_mouse_mode() {
    let mut tl = Timeline::new(&[("[", 0), ("<35;10;5M", 20)]);
    let AfterEsc::Sequence(VtOutcome::Mouse(m)) = after_esc(&mut tl, esc_wait_ms(true)) else {
        panic!("a report split after its [ was not a mouse event")
    };
    assert_eq!((m.x, m.y, m.is_move), (9, 4, true));
    assert!(tl.left().is_empty(), "no tail char leaks as a key");
}

#[test]
fn esc_o_a_is_up() {
    let mut tl = Timeline::new(&[("OA", 0)]);
    assert_eq!(
        after_esc(&mut tl, esc_wait_ms(true)),
        AfterEsc::Sequence(VtOutcome::Key(KeyPress::named(vk::UP)))
    );
    assert_eq!(tl.left(), Vec::<u16>::new(), "the final byte is consumed");
}

#[test]
fn an_x10_report_after_esc_is_mouse() {
    // `ESC [ M` then button, column, row bytes, each 32 + value.
    let mut tl = Timeline::new(&[("[M u#", 0)]);
    let AfterEsc::Sequence(VtOutcome::Mouse(m)) = after_esc(&mut tl, esc_wait_ms(false)) else {
        panic!("an X10 report was not a mouse event")
    };
    assert_eq!((m.x, m.y, m.left), (84, 2, true));
    assert_eq!(tl.left(), Vec::<u16>::new(), "all four report bytes are consumed");
    assert_eq!(tl.reports, 1);
}

#[test]
fn a_lone_esc_is_escape_and_waits_less_than_a_person_notices() {
    for text_mouse in [false, true] {
        assert_eq!(after_esc(&mut Timeline::new(&[]), esc_wait_ms(text_mouse)), AfterEsc::Alone);
        assert!(esc_wait_ms(text_mouse) < 50);
    }
    assert_eq!(after_esc(&mut Timeline::new(&[("x", 200)]), esc_wait_ms(true)), AfterEsc::Alone);
}

#[test]
fn esc_then_another_key_is_escape() {
    let mut tl = Timeline::new(&[("\u{1b}[<35;1;1M", 1)]);
    assert_eq!(after_esc(&mut tl, esc_wait_ms(true)), AfterEsc::Followed);
    assert_eq!(tl.left().len(), 10, "the next report stays queued");
}

#[test]
fn esc_bracket_a_is_up_in_both_modes() {
    for text_mouse in [false, true] {
        let mut tl = Timeline::new(&[("[A", 0)]);
        assert_eq!(
            after_esc(&mut tl, esc_wait_ms(text_mouse)),
            AfterEsc::Sequence(VtOutcome::Key(KeyPress::named(vk::UP)))
        );
    }
}

#[test]
fn an_enter_right_after_a_report_stays_queued_for_its_own_read() {
    let mut tl = Timeline::new(&[("[<35;10;5M\r", 0)]);
    assert!(matches!(
        after_esc(&mut tl, esc_wait_ms(true)),
        AfterEsc::Sequence(VtOutcome::Mouse(_))
    ));
    assert_eq!(tl.left(), vec![13]);
    assert!(key_from_record(true, 0, 13, 0).expect("a press").is(vk::ENTER));
}

#[test]
fn key_up_and_bare_modifiers_are_not_presses() {
    assert_eq!(key_from_record(false, 0x41, 97, 0), None);
    assert_eq!(key_from_record(true, vk::SHIFT, 0, SHIFT_PRESSED), None);
    let k = key_from_record(true, 0x52, 18, 0x0008).expect("ctrl+r");
    assert!(k.is_ctrl(vk::R));
}
