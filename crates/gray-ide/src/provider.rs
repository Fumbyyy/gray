use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum ProviderEvent {
    Text(String),
    Replace(String),
    Activity {
        kind: String,
        label: String,
        detail: String,
    },
    Session(String),
    Approval {
        label: String,
    },
    Usage {
        input: u64,
        output: u64,
        total: u64,
        cost_micros: Option<u64>,
        requests: Option<u64>,
    },
    Done,
    Error(String),
}

pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub models: &'static [&'static str],
}

pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "gray",
        label: "Gray",
        models: &["default"],
    },
    ProviderInfo {
        id: "claude",
        label: "Claude",
        models: &["sonnet", "opus", "haiku"],
    },
    ProviderInfo {
        id: "codex",
        label: "Codex",
        models: &["gpt-5-codex", "gpt-5"],
    },
    ProviderInfo {
        id: "fake",
        label: "Fake",
        models: &["fake-1"],
    },
];

pub const MODES: &[(&str, &str)] = &[
    ("full", "Full access"),
    ("edits", "Approve edits"),
    ("readonly", "Read only"),
];

pub fn provider_label(id: &str) -> &'static str {
    PROVIDERS
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.label)
        .unwrap_or("Unknown")
}

pub fn mode_label(id: &str) -> &'static str {
    MODES
        .iter()
        .find(|m| m.0 == id)
        .map(|m| m.1)
        .unwrap_or("Full access")
}

pub fn default_model(provider: &str) -> &'static str {
    PROVIDERS
        .iter()
        .find(|p| p.id == provider)
        .and_then(|p| p.models.first().copied())
        .unwrap_or("default")
}

pub struct Turn {
    pub rx: Receiver<ProviderEvent>,
    pub pending: Arc<Mutex<Option<Sender<bool>>>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    child: Arc<Mutex<Option<Child>>>,
    cancelled: Arc<AtomicBool>,
}

impl Turn {
    /// Steer a mid-turn message into the running child, when the provider
    /// was spawned with a steerable stdin (gray's print-mode
    /// `{"type":"steer"}` line protocol; see `GRAY_STDIN_STEER`).
    /// Err when the child has no writable stdin or the write fails.
    pub fn steer(&self, text: &str) -> Result<(), String> {
        let line = serde_json::json!({ "type": "steer", "text": text });
        let mut guard = self.stdin.lock().map_err(|e| e.to_string())?;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| "provider stdin is not steerable".to_string())?;
        writeln!(stdin, "{line}").map_err(|e| e.to_string())
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(mut guard) = self.child.lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.kill();
        }
    }
}

pub struct TurnRequest {
    pub provider: String,
    pub model: String,
    pub mode: String,
    pub prompt: String,
    pub cwd: PathBuf,
    pub session: Option<String>,
    pub effort: String,
}

pub fn start(req: TurnRequest) -> Turn {
    let (tx, rx) = channel();
    let child = Arc::new(Mutex::new(None));
    let slot = child.clone();
    let stdin = Arc::new(Mutex::new(None));
    let stdin_slot = stdin.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    let pending = Arc::new(Mutex::new(None::<Sender<bool>>));
    let pend = pending.clone();
    thread::spawn(move || {
        let result = if flag.load(Ordering::SeqCst) {
            Ok(())
        } else {
            match req.provider.as_str() {
                "fake" => run_fake(&req, &tx, &flag, &pend),
                "claude" => {
                    run_process(claude_command(&req), &tx, &slot, &flag, parse_claude, None)
                }
                "codex" => run_process(codex_command(&req), &tx, &slot, &flag, parse_codex, None),
                _ => run_process(
                    gray_command(&req),
                    &tx,
                    &slot,
                    &flag,
                    parse_gray,
                    Some(stdin_slot),
                ),
            }
        };
        if let Ok(mut p) = pend.lock() {
            *p = None;
        }
        match result {
            Ok(()) => {
                let _ = tx.send(ProviderEvent::Done);
            }
            Err(e) => {
                let _ = tx.send(ProviderEvent::Error(e));
            }
        }
    });
    Turn {
        rx,
        pending,
        stdin,
        child,
        cancelled,
    }
}

fn bin(env: &str, default: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| default.to_string())
}

fn gray_command(req: &TurnRequest) -> Command {
    let mut cmd = Command::new(bin("GRAY_IDE_GRAY_BIN", "gray"));
    cmd.arg("-p").arg(&req.prompt).arg("--json");
    // Enable the `{"type":"steer"}` stdin protocol so `Turn::steer` can
    // deliver mid-turn input; the child is spawned with a piped stdin.
    cmd.env("GRAY_STDIN_STEER", "1");
    if !req.model.is_empty() && req.model != "default" {
        cmd.arg("--model").arg(&req.model);
    }
    if let Some(sid) = &req.session {
        cmd.arg("--session").arg(sid);
    }
    if !req.effort.is_empty() {
        cmd.env("GRAY_THINKING_EFFORT", &req.effort);
    }
    cmd.current_dir(&req.cwd);
    cmd
}

fn claude_command(req: &TurnRequest) -> Command {
    let mode = match req.mode.as_str() {
        "readonly" => "plan",
        "edits" => "acceptEdits",
        _ => "bypassPermissions",
    };
    let mut cmd = Command::new(bin("GRAY_IDE_CLAUDE_BIN", "claude"));
    cmd.arg("-p")
        .arg(&req.prompt)
        .args(["--output-format", "stream-json", "--verbose"])
        .args(["--model", &req.model])
        .args(["--permission-mode", mode]);
    if let Some(sid) = &req.session {
        cmd.arg("--resume").arg(sid);
    }
    cmd.current_dir(&req.cwd);
    cmd
}

fn codex_command(req: &TurnRequest) -> Command {
    let sandbox = match req.mode.as_str() {
        "readonly" => "read-only",
        "edits" => "workspace-write",
        _ => "danger-full-access",
    };
    let mut cmd = Command::new(bin("GRAY_IDE_CODEX_BIN", "codex"));
    cmd.args(["exec", "--json", "--skip-git-repo-check"])
        .args(["--model", &req.model])
        .args(["--sandbox", sandbox]);
    if !req.effort.is_empty() {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort=\"{}\"", req.effort));
    }
    if let Some(sid) = &req.session {
        cmd.arg("resume").arg(sid);
    }
    cmd.arg(&req.prompt);
    cmd.current_dir(&req.cwd);
    cmd
}

fn run_process(
    mut cmd: Command,
    tx: &Sender<ProviderEvent>,
    slot: &Arc<Mutex<Option<Child>>>,
    cancelled: &Arc<AtomicBool>,
    parse: fn(&serde_json::Value, &mut ParseState) -> Vec<ProviderEvent>,
    steer_stdin: Option<Arc<Mutex<Option<ChildStdin>>>>,
) -> Result<(), String> {
    cmd.stdin(if steer_stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {:?}: {e}", cmd.get_program()))?;
    if let Some(steer) = &steer_stdin
        && let Ok(mut g) = steer.lock()
    {
        *g = child.stdin.take();
    }
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take();
    if cancelled.load(Ordering::SeqCst) {
        let _ = child.kill();
        return Ok(());
    }
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(child);
        if cancelled.load(Ordering::SeqCst)
            && let Some(c) = guard.as_mut()
        {
            let _ = c.kill();
            return Ok(());
        }
    }
    let err_buf = Arc::new(Mutex::new(String::new()));
    let err_clone = err_buf.clone();
    let err_thread = stderr.map(|s| {
        thread::spawn(move || {
            for line in BufReader::new(s).lines().map_while(Result::ok) {
                if let Ok(mut b) = err_clone.lock() {
                    b.push_str(&line);
                    b.push('\n');
                }
            }
        })
    });
    let mut state = ParseState::default();
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(v) => {
                // gray's demuxed ask protocol (`GRAY_STDIN_STEER` also
                // enables it): the child emits a
                // `{"phase":"ask","ask_id":N}` row and parks the turn until
                // an `{"ask_id":N,"answers":{...}}` reply arrives on
                // stdin. The IDE has no ask UI, so dismiss with empty
                // answers — same result as the pre-demux stdin-EOF path,
                // instead of stalling for the 300s timeout.
                if let Some(ask_id) = demux_ask_id(&v) {
                    let question = v
                        .pointer("/questions/0/question")
                        .and_then(|q| q.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(steer) = &steer_stdin
                        && let Ok(mut g) = steer.lock()
                        && let Some(stdin) = g.as_mut()
                    {
                        let reply =
                            serde_json::json!({"ask_id": ask_id, "answers": {}});
                        let _ = writeln!(stdin, "{reply}");
                    }
                    let _ = tx.send(ProviderEvent::Activity {
                        kind: "approval".into(),
                        label: "Plugin question dismissed".into(),
                        detail: question,
                    });
                    continue;
                }
                for ev in parse(&v, &mut state) {
                    let _ = tx.send(ev);
                }
            }
            Err(_) => {
                let _ = tx.send(ProviderEvent::Text(format!("{trimmed}\n")));
            }
        }
    }
    if let Some(t) = err_thread {
        let _ = t.join();
    }
    let status = slot
        .lock()
        .ok()
        .and_then(|mut g| g.as_mut().and_then(|c| c.wait().ok()));
    if let Some(err) = state.error.take() {
        return Err(err);
    }
    match status {
        Some(s) if !s.success() => {
            let stderr = err_buf.lock().map(|b| b.clone()).unwrap_or_default();
            let tail: String = stderr
                .lines()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            Err(if tail.is_empty() {
                format!("provider exited with {s}")
            } else {
                tail
            })
        }
        _ => Ok(()),
    }
}

#[derive(Default)]
pub struct ParseState {
    pub got_text: bool,
    pub error: Option<String>,
    pub session: Option<String>,
}

impl ParseState {
    fn session(&mut self, id: String) -> Option<ProviderEvent> {
        if id.is_empty() || self.session.as_deref() == Some(id.as_str()) {
            return None;
        }
        self.session = Some(id.clone());
        Some(ProviderEvent::Session(id))
    }
}

fn s(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn u64_at(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|x| x.as_u64())
}

/// If `v` is a gray demuxed ask row
/// (`{"type":"progress","phase":"ask","ask_id":N}`), return the ask id
/// that must be answered on the child's stdin.
fn demux_ask_id(v: &serde_json::Value) -> Option<u64> {
    if v.get("type").and_then(|t| t.as_str()) == Some("progress")
        && v.get("phase").and_then(|p| p.as_str()) == Some("ask")
    {
        v.get("ask_id").and_then(|a| a.as_u64())
    } else {
        None
    }
}

pub fn parse_gray(v: &serde_json::Value, st: &mut ParseState) -> Vec<ProviderEvent> {
    let mut out: Vec<ProviderEvent> = st.session(s(v, "session_id")).into_iter().collect();
    out.extend(parse_gray_row(v, st));
    out
}

fn parse_gray_row(v: &serde_json::Value, st: &mut ParseState) -> Vec<ProviderEvent> {
    match v.get("type").and_then(|t| t.as_str()) {
        Some("progress") => match v.get("phase").and_then(|p| p.as_str()) {
            Some("text") => {
                let delta = s(v, "delta");
                if delta.is_empty() {
                    return vec![];
                }
                st.got_text = true;
                let mut d = delta;
                if v.get("done").and_then(|x| x.as_bool()) == Some(true) && !d.ends_with('\n') {
                    d.push_str("\n\n");
                }
                vec![ProviderEvent::Text(d)]
            }
            Some("tool_started") => vec![],
            Some("tool_ran") => vec![ProviderEvent::Activity {
                kind: "tool".into(),
                label: s(v, "label"),
                detail: s(v, "detail"),
            }],
            Some("tool_finished") => {
                let output = s(v, "output");
                if output.is_empty() {
                    vec![]
                } else {
                    vec![ProviderEvent::Activity {
                        kind: if v.get("error").and_then(|e| e.as_bool()) == Some(true) {
                            "error".into()
                        } else {
                            "output".into()
                        },
                        label: format!("{} output", s(v, "label")),
                        detail: output,
                    }]
                }
            }
            Some("thinking") => vec![ProviderEvent::Activity {
                kind: "thinking".into(),
                label: "Thinking".into(),
                detail: s(v, "detail"),
            }],
            _ => vec![],
        },
        Some("result") => {
            let mut out = Vec::new();
            if v.get("usage").is_some() || v.get("accounting").is_some() {
                let usage = v.get("usage").cloned().unwrap_or_default();
                let accounting = v.get("accounting").cloned().unwrap_or_default();
                let input = u64_at(&usage, "input_tokens").unwrap_or(0);
                let output = u64_at(&usage, "output_tokens").unwrap_or(0);
                let total = u64_at(&usage, "total_tokens").unwrap_or(input + output);
                out.push(ProviderEvent::Usage {
                    input,
                    output,
                    total,
                    cost_micros: u64_at(&accounting, "cost_micros"),
                    requests: u64_at(&accounting, "requests"),
                });
            }
            if !st.got_text {
                out.push(ProviderEvent::Replace(s(v, "text")));
            }
            out
        }
        Some("error") => {
            st.error = Some(s(v, "message").to_string())
                .filter(|m| !m.is_empty())
                .or(Some(v.to_string()));
            vec![]
        }
        _ => vec![],
    }
}

pub fn parse_claude(v: &serde_json::Value, st: &mut ParseState) -> Vec<ProviderEvent> {
    let mut out: Vec<ProviderEvent> = st.session(s(v, "session_id")).into_iter().collect();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) {
                for block in content {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            st.got_text = true;
                            out.push(ProviderEvent::Text(format!("{}\n\n", s(block, "text"))));
                        }
                        Some("tool_use") => out.push(ProviderEvent::Activity {
                            kind: "tool".into(),
                            label: s(block, "name"),
                            detail: block
                                .get("input")
                                .map(|i| i.to_string())
                                .unwrap_or_default(),
                        }),
                        Some("thinking") => out.push(ProviderEvent::Activity {
                            kind: "thinking".into(),
                            label: "Thinking".into(),
                            detail: s(block, "thinking"),
                        }),
                        _ => {}
                    }
                }
            }
        }
        Some("result") => {
            if let Some(usage) = v.get("usage") {
                let input = u64_at(usage, "input_tokens").unwrap_or(0);
                let output = u64_at(usage, "output_tokens").unwrap_or(0);
                let total = u64_at(usage, "total_tokens").unwrap_or(input + output);
                let cost_micros = v
                    .get("total_cost_usd")
                    .and_then(|c| c.as_f64())
                    .map(|c| (c * 1e6) as u64);
                out.push(ProviderEvent::Usage {
                    input,
                    output,
                    total,
                    cost_micros,
                    requests: None,
                });
            }
            if v.get("is_error").and_then(|e| e.as_bool()) == Some(true) {
                st.error = Some(s(v, "result"));
            } else if !st.got_text {
                out.push(ProviderEvent::Replace(s(v, "result")));
            }
        }
        _ => {}
    }
    out
}

pub fn parse_codex(v: &serde_json::Value, st: &mut ParseState) -> Vec<ProviderEvent> {
    let mut out = Vec::new();
    match v.get("type").and_then(|t| t.as_str()) {
        Some("thread.started") => out.extend(st.session(s(v, "thread_id"))),
        Some("item.completed") => {
            let item = v.get("item").cloned().unwrap_or_default();
            match item.get("type").and_then(|t| t.as_str()) {
                Some("agent_message") => {
                    st.got_text = true;
                    out.push(ProviderEvent::Text(format!("{}\n\n", s(&item, "text"))));
                }
                Some("reasoning") => out.push(ProviderEvent::Activity {
                    kind: "thinking".into(),
                    label: "Reasoning".into(),
                    detail: s(&item, "text"),
                }),
                Some("command_execution") => out.push(ProviderEvent::Activity {
                    kind: "command".into(),
                    label: s(&item, "command"),
                    detail: s(&item, "aggregated_output"),
                }),
                Some("file_change") => out.push(ProviderEvent::Activity {
                    kind: "edit".into(),
                    label: "Edited files".into(),
                    detail: item
                        .get("changes")
                        .map(|c| c.to_string())
                        .unwrap_or_default(),
                }),
                _ => {}
            }
        }
        Some("turn.completed") => {
            if let Some(usage) = v.get("usage") {
                let input = u64_at(usage, "input_tokens").unwrap_or(0);
                let output = u64_at(usage, "output_tokens").unwrap_or(0);
                let total = u64_at(usage, "total_tokens").unwrap_or(input + output);
                out.push(ProviderEvent::Usage {
                    input,
                    output,
                    total,
                    cost_micros: None,
                    requests: None,
                });
            }
        }
        Some("turn.failed") | Some("error") => {
            st.error = Some(
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| s(v, "message")),
            );
        }
        _ => {}
    }
    out
}

fn ask_approval(
    tx: &Sender<ProviderEvent>,
    pending: &Arc<Mutex<Option<Sender<bool>>>>,
    label: String,
) -> bool {
    let (atx, arx) = channel();
    if let Ok(mut p) = pending.lock() {
        *p = Some(atx);
    }
    let _ = tx.send(ProviderEvent::Approval { label });
    arx.recv().unwrap_or(false)
}

fn run_fake(
    req: &TurnRequest,
    tx: &Sender<ProviderEvent>,
    cancelled: &Arc<AtomicBool>,
    pending: &Arc<Mutex<Option<Sender<bool>>>>,
) -> Result<(), String> {
    let delay = Duration::from_millis(
        std::env::var("GRAY_IDE_FAKE_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(25),
    );
    let prompt = req.prompt.trim();
    let session = req
        .session
        .clone()
        .unwrap_or_else(|| format!("fake-{}", &uuid::Uuid::new_v4().to_string()[..8]));
    let effort = req.effort.clone();
    let _ = tx.send(ProviderEvent::Session(session.clone()));
    if prompt.contains("please fail") {
        thread::sleep(delay);
        return Err("fake provider failure requested".into());
    }
    let _ = tx.send(ProviderEvent::Activity {
        kind: "tool".into(),
        label: "Read workspace".into(),
        detail: req.cwd.display().to_string(),
    });
    if let Some(rest) = prompt.strip_prefix("write ") {
        let (path, content) = rest.split_once(' ').unwrap_or((rest, ""));
        if req.mode == "readonly" && !ask_approval(tx, pending, format!("Write {path}")) {
            let _ = tx.send(ProviderEvent::Activity {
                kind: "approval".into(),
                label: "Rejected".into(),
                detail: format!("Write {path}"),
            });
        } else {
            write_file(&req.cwd, path, content)?;
            let _ = tx.send(ProviderEvent::Activity {
                kind: "edit".into(),
                label: format!("Edited {path}"),
                detail: format!("+{} lines", content.lines().count().max(1)),
            });
        }
    }
    if let Some(cmd) = prompt.strip_prefix("run ") {
        if req.mode == "readonly" && !ask_approval(tx, pending, format!("Run `{cmd}`")) {
            let _ = tx.send(ProviderEvent::Activity {
                kind: "approval".into(),
                label: "Rejected".into(),
                detail: format!("Run {cmd}"),
            });
        } else {
            let output = Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(&req.cwd)
                .output()
                .map_err(|e| e.to_string())?;
            let _ = tx.send(ProviderEvent::Activity {
                kind: "command".into(),
                label: cmd.to_string(),
                detail: String::from_utf8_lossy(&output.stdout).to_string(),
            });
        }
    }
    if prompt.contains("slow") {
        thread::sleep(Duration::from_secs(5));
    }
    let reply = format!(
        "Gray fake agent received: **{prompt}**\n\nRunning in `{}` with model `{}` and mode `{}` in session `{session}` at effort `{effort}`.\n\n```rust\nfn main() {{\n    println!(\"hello from gray\");\n}}\n```\n",
        req.cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        req.model,
        mode_label(&req.mode)
    );
    for chunk in reply.split_inclusive(' ') {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _ = tx.send(ProviderEvent::Text(chunk.to_string()));
        thread::sleep(delay);
    }
    // Small deterministic usage report so token/cost plumbing is
    // exercised without a real provider.
    let _ = tx.send(ProviderEvent::Usage {
        input: 40,
        output: 12,
        total: 52,
        cost_micros: Some(7),
        requests: Some(1),
    });
    Ok(())
}

fn write_file(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    if rel.contains("..") {
        return Err("refusing to write outside the workspace".into());
    }
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, format!("{}\n", content.replace("\\n", "\n"))).map_err(|e| e.to_string())
}
