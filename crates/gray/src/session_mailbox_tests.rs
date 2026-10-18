use super::*;

fn cron_entry(name: &str, body: &str) -> String {
    serde_json::json!({
        "card": format!("cron: {name}\n\n{body}"),
        "prompt": format!("[Cron delivery: {name}]\n{body}"),
        "name": name,
        "body": body,
        "status": "ok",
        "reminder": false,
        "elapsed_ms": 0,
    })
    .to_string()
}

#[test]
fn a_session_origin_entry_lands_in_that_sessions_inbox_once() {
    let home = tempfile::tempdir().unwrap();
    let dir = session_inbox(home.path(), "sess-1").unwrap();
    write_entry(&dir, &cron_entry("s3", "toilets: a history")).unwrap();

    assert!(session_inbox_pending(home.path(), "sess-1"));
    assert!(!session_inbox_pending(home.path(), "other-session"));
    let got = drain_session_inbox(home.path(), "sess-1");
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        got[0].card.contains("toilets: a history"),
        "card: {}",
        got[0].card
    );
    assert_eq!(got[0].prompt, "[Cron delivery: s3]\ntoilets: a history");
    let card = got[0].cron.as_ref().expect("structured card");
    assert_eq!(card.name, "s3");
    assert_eq!(card.body, "toilets: a history");
    assert!(!card.failed && !card.reminder);
    assert!(
        drain_session_inbox(home.path(), "sess-1").is_empty(),
        "delivered once"
    );
    assert!(!session_inbox_pending(home.path(), "sess-1"));

    assert!(!session_inbox_pending(home.path(), "../sess-1"));
    assert!(session_inbox(home.path(), "../sess-1").is_none());
}

#[test]
fn drain_leaves_send_entries_for_the_send_claimer() {
    let home = tempfile::tempdir().unwrap();
    let dir = session_inbox(home.path(), "sess-1").unwrap();
    let send = serde_json::json!({
        "kind": crate::session_send::KIND,
        "id": "abc-123",
        "from": "cli",
        "text": "hi",
        "attachments": [],
        "created_ms": 1,
    })
    .to_string();
    write_entry(&dir, &send).unwrap();
    write_entry(&dir, &cron_entry("nightly", "all clear")).unwrap();

    let got = drain_session_inbox(home.path(), "sess-1");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(
        got[0].cron.as_ref().map(|c| c.name.as_str()),
        Some("nightly")
    );
    let claimed = crate::session_send::claim_all(home.path(), "sess-1");
    assert_eq!(claimed.len(), 1, "{claimed:?}");
    assert_eq!(claimed[0].text, "hi");
}

#[test]
fn pending_ignores_held_sends_and_dotfiles() {
    let home = tempfile::tempdir().unwrap();
    let dir = session_inbox(home.path(), "sess-1").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".tmp"), "x").unwrap();
    let held = serde_json::json!({
        "kind": crate::session_send::KIND,
        "id": "abc-124",
        "from": "cli",
        "text": "later",
        "attachments": [],
        "created_ms": 1,
        "not_before_ms": u64::MAX,
    })
    .to_string();
    write_entry(&dir, &held).unwrap();
    assert!(!session_inbox_pending(home.path(), "sess-1"));

    write_entry(&dir, &cron_entry("n", "b")).unwrap();
    assert!(session_inbox_pending(home.path(), "sess-1"));
}

#[test]
fn drain_drops_unreadable_entries() {
    let home = tempfile::tempdir().unwrap();
    let dir = session_inbox(home.path(), "sess-1").unwrap();
    write_entry(&dir, "not json").unwrap();
    write_entry(&dir, &serde_json::json!({"prompt": "p"}).to_string()).unwrap();
    assert!(drain_session_inbox(home.path(), "sess-1").is_empty());
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "unreadable entries were removed"
    );
}
