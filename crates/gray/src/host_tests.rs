use super::*;

#[tokio::test]
async fn background_rejects_headless_and_malformed_requests() {
    let handler = default_handler(PathBuf::from("/tmp"));
    for params in [
        serde_json::json!({}),
        serde_json::json!({"path": 42}),
        serde_json::json!({"path": null, "raw_escape": "bad"}),
    ] {
        let reply = handler("host/background".into(), params).await;
        assert!(reply.get("error").is_some(), "{reply}");
    }
    let reply = handler("host/background".into(), serde_json::json!({"path": null})).await;
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("interactive Gray TUI")
    );
}

fn drain_queued(entries: Vec<SayEntry>) -> Vec<Vec<Line<'static>>> {
    let mut q = SAY_QUEUE.lock().expect("say queue");
    q.clear();
    for e in entries {
        q.push(e);
    }
    drain_say_entries(std::mem::take(&mut *q))
}

fn line_text(lines: &[Line<'static>]) -> String {
    lines[0].spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn plugin_crash_notices_drain_as_one_coalesced_line() {
    let drained = drain_queued(vec![
        SayEntry::Crash(CrashNotice {
            plugin: "gray-gateway".into(),
            status: "exit 1".into(),
            restarted: true,
            error: None,
        }),
        SayEntry::Crash(CrashNotice {
            plugin: "gray-mcp".into(),
            status: "exit 1".into(),
            restarted: true,
            error: None,
        }),
    ]);
    assert_eq!(drained.len(), 1, "{drained:?}");
    assert_eq!(
        line_text(&drained[0]),
        "  ! gateway, mcp crashed (exit 1), restarted · sidecar-stderr.log"
    );
}

#[test]
fn plugin_crash_line_lists_differing_statuses_and_restart_failure() {
    let drained = drain_queued(vec![
        SayEntry::Crash(CrashNotice {
            plugin: "gray-gateway".into(),
            status: "exit 1".into(),
            restarted: true,
            error: None,
        }),
        SayEntry::Crash(CrashNotice {
            plugin: "gray-mcp".into(),
            status: "signal 9".into(),
            restarted: true,
            error: None,
        }),
    ]);
    assert_eq!(
        line_text(&drained[0]),
        "  ! gateway (exit 1), mcp (signal 9) crashed, restarted · sidecar-stderr.log"
    );

    let drained = drain_queued(vec![SayEntry::Crash(CrashNotice {
        plugin: "gray-gateway".into(),
        status: "exit 1".into(),
        restarted: false,
        error: Some("spawn failed".into()),
    })]);
    assert_eq!(
        line_text(&drained[0]),
        "  ! gateway crashed (exit 1), restart failed: spawn failed · sidecar-stderr.log"
    );
}
