use super::*;

#[test]
fn prefix_arms_and_resolves_a_binding() {
    let live = live();
    let mut p = Prefix::default();
    let ctrl_b = press(Key::Char('b'), Mods::CTRL);
    assert_held(p.feed(&ctrl_b, &live));
    assert!(p.armed());
    // `prefix+w` is the reference WorkspacePicker binding.
    assert_action(
        p.feed(&press(Key::Char('w'), Mods::NONE), &live),
        KeybindAction::WorkspacePicker,
    );
    assert!(!p.armed());
}
#[test]
fn double_prefix_forwards_a_literal_ctrl_b() {
    let live = live();
    let mut p = Prefix::default();
    let ctrl_b = press(Key::Char('b'), Mods::CTRL);
    p.feed(&ctrl_b, &live);
    assert_term(p.feed(&ctrl_b, &live), &ctrl_b);
}
#[test]
fn ordinary_keys_pass_untouched() {
    let live = live();
    let mut p = Prefix::default();
    for input in [
        press(Key::Tab, Mods::NONE),
        press(Key::Char('c'), Mods::CTRL),
        press(Key::ArrowUp, Mods::NONE),
        press(Key::Char('x'), Mods::ALT),
    ] {
        assert_term(p.feed(&input, &live), &input);
    }
}
#[test]
fn unknown_prefix_key_disarms_silently() {
    let live = live();
    let mut p = Prefix::default();
    p.feed(&press(Key::Char('b'), Mods::CTRL), &live);
    assert_held(p.feed(&press(Key::Char('~'), Mods::NONE), &live));
    assert!(!p.armed());
    // The next plain key is not swallowed.
    let a = press(Key::Char('a'), Mods::NONE);
    assert_term(p.feed(&a, &live), &a);
}
#[test]
fn navigate_consumes_navigation_and_cancel() {
    let live = live();
    assert_eq!(
        navigate_key(&press(Key::ArrowDown, Mods::NONE), &live),
        Some(Navigate::Move(1))
    );
    assert_eq!(
        navigate_key(&press(Key::ArrowUp, Mods::NONE), &live),
        Some(Navigate::Move(-1))
    );
    assert_eq!(
        navigate_key(&press(Key::Enter, Mods::NONE), &live),
        Some(Navigate::Accept)
    );
    assert_eq!(
        navigate_key(&press(Key::Escape, Mods::NONE), &live),
        Some(Navigate::Cancel)
    );
    assert_eq!(
        navigate_key(&press(Key::Char('3'), Mods::NONE), &live),
        Some(Navigate::Select(2))
    );
    // Plain Tab is never a global switch key — ignored, not eaten.
    assert_eq!(navigate_key(&press(Key::Tab, Mods::NONE), &live), None);
    assert_eq!(
        navigate_key(&press(Key::Char('x'), Mods::NONE), &live),
        None
    );
}
#[test]
fn armed_survives_releases_and_swallows_the_action_tail() {
    let live = live();
    let mut p = Prefix::default();
    let b = Key::Char('b');
    let w = Key::Char('w');
    // ^B press arms; ^B repeat (held) and release keep it armed.
    p.feed(&at(b, Mods::CTRL, Action::Press), &live);
    assert_held(p.feed(&at(b, Mods::CTRL, Action::Repeat), &live));
    assert_held(p.feed(&at(b, Mods::CTRL, Action::Release), &live));
    assert!(p.armed(), "release/repeat disarmed the prefix");
    // 'w' press resolves; its release is swallowed, not forwarded.
    assert_action(
        p.feed(&at(w, Mods::NONE, Action::Press), &live),
        KeybindAction::WorkspacePicker,
    );
    assert_held(p.feed(&at(w, Mods::NONE, Action::Release), &live));
}
#[test]
fn forwarded_press_release_pairs_stay_intact() {
    let live = live();
    let mut p = Prefix::default();
    let x = at(Key::Char('x'), Mods::NONE, Action::Press);
    assert_term(p.feed(&x, &live), &x);
    // Prefix opens while x is physically held; its release forwards.
    p.feed(&at(Key::Char('b'), Mods::CTRL, Action::Press), &live);
    let rel = at(Key::Char('x'), Mods::NONE, Action::Release);
    assert_term(p.feed(&rel, &live), &rel);
    // The armed prefix still resolves on the next press.
    assert_action(
        p.feed(&at(Key::Char('q'), Mods::NONE, Action::Press), &live),
        KeybindAction::Detach,
    );
    assert_held(p.feed(&at(Key::Char('q'), Mods::NONE, Action::Release), &live));
}
#[test]
fn double_prefix_release_pair_and_recovery() {
    let live = live();
    let mut p = Prefix::default();
    let b = Key::Char('b');
    p.feed(&at(b, Mods::CTRL, Action::Press), &live);
    let fwd = at(b, Mods::CTRL, Action::Press);
    assert_term(p.feed(&fwd, &live), &fwd);
    let rel = at(b, Mods::CTRL, Action::Release);
    assert_term(p.feed(&rel, &live), &rel);
    // A fresh ^B still arms after the sequence.
    assert_held(p.feed(&at(b, Mods::CTRL, Action::Press), &live));
    assert!(p.armed());
}
#[test]
fn swallowed_selector_release_pops_once() {
    let live = live();
    let mut p = Prefix::default();
    p.swallow(&at(Key::ArrowDown, Mods::NONE, Action::Press));
    assert_held(p.feed(&at(Key::ArrowDown, Mods::NONE, Action::Release), &live));
    // A second release (no press was consumed) forwards.
    let again = at(Key::ArrowDown, Mods::NONE, Action::Release);
    assert_term(p.feed(&again, &live), &again);
}
