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
    assert!(!is_hotkey(&shifted_m, 'm'), "an SGR final byte is never a hotkey");
    let upper_u = KeyPress {
        ch: 'U',
        vk: 0x55,
        ..KeyPress::default()
    };
    assert!(!is_hotkey(&upper_u, 'u'));
    let caps_u = KeyPress {
        caps_lock: true,
        ..upper_u
    };
    assert!(is_hotkey(&caps_u, 'u'), "Caps Lock lifts the case guard");
    assert!(is_hotkey(&KeyPress::named(vk::OEM2), '/'));
    assert!(is_hotkey(&KeyPress::char('.'), '/'), "slash is '.' on both Cyrillic layouts");
    assert!(is_hotkey(&KeyPress::named(0x31), '1'));
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
fn key_up_and_bare_modifiers_are_not_presses() {
    assert_eq!(key_from_record(false, 0x41, 97, 0), None);
    assert_eq!(key_from_record(true, vk::SHIFT, 0, SHIFT_PRESSED), None);
    let k = key_from_record(true, 0x52, 18, 0x0008).expect("ctrl+r");
    assert!(k.is_ctrl(vk::R));
}
