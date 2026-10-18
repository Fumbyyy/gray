use super::*;

struct ScriptedWaits {
    answers: std::sync::Mutex<Vec<Option<()>>>,
    pending: std::sync::atomic::AtomicBool,
    waits: std::sync::atomic::AtomicUsize,
}

impl gray_core::agent::ToolExecutor for ScriptedWaits {
    fn wait_for_notification(
        &self,
        _ctx: &gray_core::agent::ToolContext,
        _timeout: std::time::Duration,
    ) -> futures::future::BoxFuture<'static, Option<()>> {
        self.waits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = self.answers.lock().unwrap().remove(0);
        Box::pin(std::future::ready(answer))
    }

    fn has_pending_background(&self, _ctx: &gray_core::agent::ToolContext) -> bool {
        self.pending.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn execute(
        &self,
        _ctx: &gray_core::agent::ToolContext,
        _name: &str,
        _args: serde_json::Value,
    ) -> futures::future::BoxFuture<'static, gray_core::agent::ToolOutput> {
        unreachable!("the waker never runs tools")
    }
}

fn scripted(answers: Vec<Option<()>>, pending: bool) -> std::sync::Arc<ScriptedWaits> {
    std::sync::Arc::new(ScriptedWaits {
        answers: std::sync::Mutex::new(answers),
        pending: std::sync::atomic::AtomicBool::new(pending),
        waits: std::sync::atomic::AtomicUsize::new(0),
    })
}

#[tokio::test]
async fn a_job_longer_than_one_wait_slice_still_wakes_the_prompt() {
    let exec = scripted(vec![None, None, Some(())], true);
    wait_then_wake(
        exec.clone(),
        gray_core::agent::ToolContext::default(),
        std::time::Duration::from_millis(1),
    )
    .await;
    assert_eq!(exec.waits.load(std::sync::atomic::Ordering::SeqCst), 3);
    assert!(crate::host::wake_requested());
}

#[tokio::test]
async fn the_waiter_stops_once_nothing_is_pending() {
    let exec = scripted(vec![None, Some(())], false);
    wait_then_wake(
        exec.clone(),
        gray_core::agent::ToolContext::default(),
        std::time::Duration::from_millis(1),
    )
    .await;
    assert_eq!(
        exec.waits.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "a timed-out wait with no jobs left ends the waiter"
    );
}

fn card_text(line: &ratatui::text::Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn cron_card_header_names_job_status_and_time() {
    let card = crate::session_mailbox::CronCard {
        name: "sandbox-probe".into(),
        body: "Done \u{2014} wrote `~/.gray/sandbox-probe.txt`.".into(),
        failed: false,
        reminder: false,
        elapsed_ms: 18_400,
    };
    let (header, body) = cron_card_lines(&card, 80);
    assert_eq!(
        card_text(&header),
        "\u{2b22} Cron sandbox-probe \u{b7} done \u{b7} 18.4s"
    );
    let joined: String = body.iter().map(card_text).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("sandbox-probe.txt"), "{joined}");
    assert!(!joined.contains("[tool:"), "{joined}");
    assert!(
        body.iter().all(|l| card_text(l).starts_with("  ")),
        "{joined}"
    );
}

#[test]
fn cron_card_reminder_and_failure_headers() {
    let reminder = crate::session_mailbox::CronCard {
        name: "clean-my-room".into(),
        body: "clean my room".into(),
        failed: false,
        reminder: true,
        elapsed_ms: 0,
    };
    let (header, body) = cron_card_lines(&reminder, 80);
    assert_eq!(card_text(&header), "\u{2b22} Reminder");
    assert_eq!(card_text(&body[0]), "  clean my room");

    let failed = crate::session_mailbox::CronCard {
        name: "nightly".into(),
        body: "provider timed out".into(),
        failed: true,
        reminder: false,
        elapsed_ms: 2_000,
    };
    let (header, _) = cron_card_lines(&failed, 80);
    assert_eq!(
        card_text(&header),
        "\u{2b22} Cron nightly \u{b7} failed \u{b7} 2s"
    );
}

#[test]
fn job_notice_becomes_a_card_without_the_model_hint() {
    let notice = "Background job bwrap-5 finished (exit 0) after 4m26s \u{b7} log /tmp/x/bash-9.log\nRead its output with `tail` (or `grep`) on that log.";
    let job = parse_job_notice(notice).expect("parses");
    assert_eq!(job.id, "bwrap-5");
    assert_eq!(job.outcome, "exit 0");
    assert_eq!(job.elapsed, "4m26s");
    assert_eq!(job.log, "/tmp/x/bash-9.log");
    let (header, body) = job_card_lines(&job);
    assert_eq!(
        card_text(&header),
        "\u{2b22} Background job bwrap-5 \u{b7} exit 0 \u{b7} 4m26s"
    );
    assert_eq!(body.len(), 1);
    assert_eq!(card_text(&body[0]), "  log /tmp/x/bash-9.log");
    assert!(parse_job_notice("something else").is_none());
}

#[test]
fn plain_format_has_no_frame_id_dashes_or_path() {
    let s = format_delivery_plain("nightly", "all clear", false, false);
    assert_eq!(s, "nightly\n\nall clear");
    let f = format_delivery_plain("nightly", "boom", false, true);
    assert_eq!(f, "nightly failed\n\nboom");
    for bad in [
        "job_id",
        "-----",
        "Cronjob Response",
        "Full output",
        "To stop or manage",
    ] {
        assert!(!s.contains(bad) && !f.contains(bad), "leaked {bad}");
    }
}

#[test]
fn reminder_plain_text_is_byte_for_byte() {
    let text = "  clean my roo \n\n\n:done: https://x.y ";
    assert_eq!(format_delivery_plain("n", text, true, false), text);
}
