//! Background-task visibility (Codex `/ps` parity, gray style).
//!
//! Pure formatters over [`TaskInfo`] snapshots — no registry access here.
//! Both consumers (the `⬡ Working…` status suffix in `composer/draw` and the
//! `/tasks` transcript card in `repl/status`) share these so the two views
//! can never disagree on counts or wording.

use super::contract::{TaskInfo, TaskState};

/// Max command chars per `/tasks` row (Codex truncates at 80 graphemes).
pub const MAX_COMMAND_CHARS: usize = 80;

/// Live (still-`Running`) tasks, preserving registry order.
pub fn running<'a>(tasks: &'a [TaskInfo]) -> Vec<&'a TaskInfo> {
    tasks
        .iter()
        .filter(|t| matches!(t.state, TaskState::Running))
        .collect()
}

/// Status-row suffix for the `⬡ Working…` pill / idle footer.
///
/// - 0 live → `None` (row and footer render exactly as today — no shift).
/// - 1 live → `"t1 running"`.
/// - n live → `"N bg tasks"`. The caller appends the `/tasks to view` hint
///   when width allows; kept separate so truncation drops the hint first.
pub fn status_suffix(tasks: &[TaskInfo]) -> Option<String> {
    let live = running(tasks);
    match live.len() {
        0 => None,
        1 => Some(format!("{} running", live[0].id)),
        n => Some(format!("{n} bg tasks")),
    }
}

/// Compact elapsed (`8s`, `2m 05s`, `1h 02m 03s`) for `/tasks` rows.
pub fn fmt_task_elapsed(elapsed_secs: u64) -> String {
    if elapsed_secs < 60 {
        return format!("{elapsed_secs}s");
    }
    if elapsed_secs < 3600 {
        return format!("{}m {:02}s", elapsed_secs / 60, elapsed_secs % 60);
    }
    format!(
        "{}h {:02}m {:02}s",
        elapsed_secs / 3600,
        (elapsed_secs % 3600) / 60,
        elapsed_secs % 60
    )
}

/// Compact byte count (`512`, `12.4k`, `3.1M`) for `/tasks` rows.
pub fn fmt_task_bytes(bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        return format!("{bytes}");
    }
    if b < K * K {
        return format!("{:.1}k", b / K);
    }
    format!("{:.1}M", b / (K * K))
}

/// One `/tasks` detail row:
/// `• t1 · pid 13941 · 8m · 12.4k logged · while true; do echo…`
pub fn task_row(task: &TaskInfo) -> String {
    let elapsed = fmt_task_elapsed(task.started.elapsed().as_secs());
    let logged = fmt_task_bytes(task.bytes);
    let cmd: String = task.command.chars().take(MAX_COMMAND_CHARS).collect();
    let cmd = if task.command.chars().count() > MAX_COMMAND_CHARS {
        format!("{cmd}…")
    } else {
        cmd
    };
    format!(
        "• {} · pid {} · {elapsed} · {logged} logged · {cmd}",
        task.id, task.pid
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Instant;

    fn task(id: u32, command: &str) -> TaskInfo {
        TaskInfo {
            id: super::super::contract::TaskId(id),
            pid: 1000 + id,
            pgid: 0,
            command: command.to_string(),
            started: Instant::now(),
            log_path: PathBuf::from("/tmp/t.log"),
            bytes: 0,
            state: TaskState::Running,
        }
    }

    #[test]
    fn suffix_empty_when_no_live_tasks() {
        assert_eq!(status_suffix(&[]), None);
        let mut exited = task(1, "sleep 1");
        exited.state = TaskState::Exited {
            report: super::super::contract::ExitReport {
                effective: 0,
                label: "exit 0".to_string(),
                note: None,
            },
            at: Instant::now(),
        };
        assert_eq!(status_suffix(&[exited]), None);
    }

    #[test]
    fn suffix_single_names_the_task() {
        assert_eq!(status_suffix(&[task(1, "sleep 60")]), Some("t1 running".to_string()));
    }

    #[test]
    fn suffix_many_counts() {
        let tasks = vec![task(1, "a"), task(2, "b"), task(3, "c")];
        assert_eq!(status_suffix(&tasks), Some("3 bg tasks".to_string()));
    }

    #[test]
    fn elapsed_formats() {
        assert_eq!(fmt_task_elapsed(8), "8s");
        assert_eq!(fmt_task_elapsed(125), "2m 05s");
        assert_eq!(fmt_task_elapsed(3723), "1h 02m 03s");
    }

    #[test]
    fn bytes_format() {
        assert_eq!(fmt_task_bytes(512), "512");
        assert_eq!(fmt_task_bytes(12_646), "12.3k");
        assert_eq!(fmt_task_bytes(3_200_000), "3.1M");
    }

    #[test]
    fn row_truncates_long_commands() {
        let long = "x".repeat(200);
        let row = task_row(&task(1, &long));
        assert!(row.ends_with("…"), "long command truncates: {row}");
        let short = task_row(&task(1, "sleep 60"));
        assert!(short.ends_with("sleep 60"), "short command intact: {short}");
    }
}
