use std::path::PathBuf;
use std::sync::Mutex;

use gray_plugin::{HOST_ASK, HOST_RUN, HOST_SAY, HostHandler};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

static TUI: Mutex<std::sync::Weak<std::sync::Mutex<crate::composer::Tui>>> =
    Mutex::new(std::sync::Weak::new());

pub(crate) fn register_tui(tui: &crate::composer::SharedTui) {
    *TUI.lock().expect("background registration") = std::sync::Arc::downgrade(tui);
}

pub(crate) fn registered_tui() -> Option<crate::composer::SharedTui> {
    TUI.lock().ok()?.upgrade()
}

fn background(params: serde_json::Value) -> anyhow::Result<()> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        path: Option<PathBuf>,
    }
    anyhow::ensure!(
        params.get("path").is_some(),
        "missing background path (null clears)"
    );
    let request: Request = serde_json::from_value(params)?;
    let tui = TUI
        .lock()
        .map_err(|_| anyhow::anyhow!("TUI unavailable"))?
        .upgrade()
        .ok_or_else(|| anyhow::anyhow!("background requires an interactive Gray TUI"))?;
    let mut tui = tui.lock().map_err(|_| anyhow::anyhow!("TUI unavailable"))?;
    tui.set_background(request.path.as_deref())
}

const MAX_SAY_QUEUE: usize = 32;

#[derive(Debug)]
pub(crate) struct CrashNotice {
    pub plugin: String,
    pub status: String,
    pub restarted: bool,
    pub error: Option<String>,
}

enum SayEntry {
    Lines(Vec<Line<'static>>),
    Crash(CrashNotice),
}

static SAY_QUEUE: Mutex<Vec<SayEntry>> = Mutex::new(Vec::new());

pub fn take_host_say() -> Vec<Vec<Line<'static>>> {
    let entries = SAY_QUEUE
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    drain_say_entries(entries)
}

fn drain_say_entries(entries: Vec<SayEntry>) -> Vec<Vec<Line<'static>>> {
    let mut out: Vec<Vec<Line<'static>>> = Vec::new();
    let mut crashes: Vec<CrashNotice> = Vec::new();
    for e in entries {
        match e {
            SayEntry::Crash(c) => crashes.push(c),
            SayEntry::Lines(lines) => {
                if !crashes.is_empty() {
                    out.push(vec![crash_notice_line(std::mem::take(&mut crashes))]);
                }
                out.push(lines);
            }
        }
    }
    if !crashes.is_empty() {
        out.push(vec![crash_notice_line(crashes)]);
    }
    out
}

fn plugin_short_name(name: &str) -> &str {
    name.strip_prefix("gray-").unwrap_or(name)
}

fn crash_notice_line(crashes: Vec<CrashNotice>) -> Line<'static> {
    let dim = Style::new().add_modifier(Modifier::DIM);
    let bang = Style::new().fg(crate::theme::theme().error);
    let mut segments: Vec<String> = Vec::new();
    let mut run: Vec<&CrashNotice> = Vec::new();
    let flush_run = |run: &mut Vec<&CrashNotice>, segments: &mut Vec<String>| {
        if run.is_empty() {
            return;
        }
        let status = &run[0].status;
        let seg = if run.iter().all(|c| &c.status == status) {
            let names = run
                .iter()
                .map(|c| plugin_short_name(&c.plugin))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{names} crashed ({status}), restarted")
        } else {
            let items = run
                .iter()
                .map(|c| format!("{} ({})", plugin_short_name(&c.plugin), c.status))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{items} crashed, restarted")
        };
        run.clear();
        segments.push(seg);
    };
    for c in &crashes {
        if c.restarted {
            run.push(c);
            continue;
        }
        flush_run(&mut run, &mut segments);
        let why = c
            .error
            .as_deref()
            .map(|e| format!(": {e}"))
            .unwrap_or_default();
        segments.push(format!(
            "{} crashed ({}), restart failed{why}",
            plugin_short_name(&c.plugin),
            c.status
        ));
    }
    flush_run(&mut run, &mut segments);
    let body = segments.join("; ");
    Line::from(vec![
        Span::styled("  ", dim),
        Span::styled("!", bang),
        Span::styled(format!(" {body} · sidecar-stderr.log"), dim),
    ])
}

fn queue_say_inner(entry: SayEntry) {
    if let Ok(mut q) = SAY_QUEUE.lock() {
        if q.len() >= MAX_SAY_QUEUE {
            q.remove(0);
        }
        q.push(entry);
    }

    request_wake();
}

pub(crate) fn queue_say(text: String) {
    queue_say_inner(SayEntry::Lines(
        text.split('\n')
            .map(|l| {
                Line::from(Span::styled(
                    format!("  {l}"),
                    Style::new().add_modifier(Modifier::DIM),
                ))
            })
            .collect(),
    ));
}

pub(crate) fn queue_crash(notice: CrashNotice) {
    queue_say_inner(SayEntry::Crash(notice));
}

pub(crate) fn queue_say_lines(lines: Vec<Line<'static>>) {
    queue_say_inner(SayEntry::Lines(lines));
}

static WAKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn request_wake() {
    WAKE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn wake_requested() -> bool {
    WAKE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn clear_wake() {
    WAKE.store(false, std::sync::atomic::Ordering::Relaxed);
}

type BackgroundSource = (
    std::sync::Arc<dyn gray_core::agent::ToolExecutor>,
    gray_core::agent::ToolContext,
);
static BACKGROUND_SOURCE: Mutex<Option<BackgroundSource>> = Mutex::new(None);

pub(crate) fn set_background_source(source: Option<BackgroundSource>) {
    *BACKGROUND_SOURCE.lock().unwrap_or_else(|e| e.into_inner()) = source;
}

pub(crate) fn background_source() -> Option<BackgroundSource> {
    BACKGROUND_SOURCE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

static LIVE_SESSION: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn set_live_session(sid: Option<&str>) {
    if let Ok(mut s) = LIVE_SESSION.lock() {
        *s = sid.map(str::to_string);
    }
}

pub(crate) fn live_session() -> Option<String> {
    LIVE_SESSION.lock().ok().and_then(|s| s.clone())
}

pub fn default_handler(cwd: PathBuf) -> HostHandler {
    std::sync::Arc::new(move |method: String, params: serde_json::Value| {
        let cwd = cwd.clone();
        let fut: std::pin::Pin<Box<dyn std::future::Future<Output = serde_json::Value> + Send>> =
            Box::pin(async move {
                match method.as_str() {
                    "host/background" => match background(params) {
                        Ok(()) => serde_json::json!({"ok": true}),
                        Err(e) => serde_json::json!({"error": format!("{e:#}")}),
                    },
                    HOST_SAY => {
                        if params.get("kind").and_then(|k| k.as_str()) == Some("plugin_crash") {
                            queue_crash(CrashNotice {
                                plugin: params
                                    .get("plugin")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("plugin")
                                    .to_string(),
                                status: params
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("exited")
                                    .to_string(),
                                restarted: params
                                    .get("restarted")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(true),
                                error: params
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .map(str::to_string),
                            });
                        } else if let Some(text) = params.get("text").and_then(|t| t.as_str())
                            && !text.trim().is_empty()
                        {
                            queue_say(text.to_string());
                        }
                        serde_json::json!({"ok": true})
                    }
                    HOST_ASK => crate::ask::handle_ask(params).await,
                    HOST_RUN => {
                        let prompt = params
                            .get("prompt")
                            .and_then(|p| p.as_str())
                            .unwrap_or("")
                            .to_string();
                        if prompt.trim().is_empty() {
                            return serde_json::json!({"error": "host/run: missing prompt"});
                        }
                        gray_plugin::host::run_prompt_child(&cwd, &prompt).await
                    }
                    _ => serde_json::json!({"error": format!("unknown host method {method}")}),
                }
            });
        fut
    })
}

#[path = "host_tests.rs"]
#[cfg(test)]
mod tests;
