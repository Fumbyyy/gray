use super::*;

#[test]
fn fresh_conversation_resets_history_to_welcome_and_home() {
    let mut tui = Tui::test_tui(80, 30);
    tui.history_entries = vec![TranscriptEntry::Welcome];

    tui.push_user_prompt("hello", &[], true);
    tui.push_action("some work", None);
    tui.left_home = true;
    assert!(!tui.is_home());

    tui.start_fresh_conversation();

    assert!(
        matches!(tui.history_entries.as_slice(), [TranscriptEntry::Welcome]),
        "history must be exactly [Welcome]: {:?}",
        tui.history_entries
    );
    assert!(tui.is_home());
}
