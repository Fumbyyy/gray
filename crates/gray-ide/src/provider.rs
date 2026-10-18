use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
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
    child: Arc<Mutex<Option<Child>>>,
}

impl Turn {
    pub fn cancel(&self) {
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
}

pub fn start(req: TurnRequest) -> Turn {
    let (tx, rx) = channel();
    let child = Arc::new(Mutex::new(None));
    let slot = child.clone();
    thread::spawn(move || {
        let result = match req.provider.as_str() {
            "fake" => run_fake(&req, &tx),
            "claude" => run_process(claude_command(&req), &tx, &slot, parse_claude),
            "codex" => run_process(codex_command(&req), &tx, &slot, parse_codex),
            _ => run_process(gray_command(&req), &tx, &slot, parse_gray),
        };
        match result {
            Ok(()) => {
                let _ = tx.send(ProviderEvent::Done);
            }
            Err(e) => {
                let _ = tx.send(ProviderEvent::Error(e));
            }
        }
    });
    Turn { rx, child }
}

fn bin(env: &str, default: &str) -> String {
    std::env::var(env).unwrap_or_else(|_| default.to_string())
}

fn gray_command(req: &TurnRequest) -> Command {
    let mut cmd = Command::new(bin("GRAY_IDE_GRAY_BIN", "gray"));
    cmd.arg("-p").arg(&req.prompt).arg("--json");
    if !req.model.is_empty() && req.model != "default" {
        cmd.arg("--model").arg(&req.model);
    }
    if let Some(sid) = &req.session {
        cmd.arg("--session").arg(sid);
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
    parse: fn(&serde_json::Value, &mut ParseState) -> Vec<ProviderEvent>,
) -> Result<(), String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {:?}: {e}", cmd.get_program()))?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take();
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(child);
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
            if st.got_text {
                vec![]
            } else {
                vec![ProviderEvent::Replace(s(v, "text"))]
            }
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

fn run_fake(req: &TurnRequest, tx: &Sender<ProviderEvent>) -> Result<(), String> {
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
        write_file(&req.cwd, path, content)?;
        let _ = tx.send(ProviderEvent::Activity {
            kind: "edit".into(),
            label: format!("Edited {path}"),
            detail: format!("+{} lines", content.lines().count().max(1)),
        });
    }
    if let Some(cmd) = prompt.strip_prefix("run ") {
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
    if prompt.contains("slow") {
        thread::sleep(Duration::from_secs(5));
    }
    let reply = format!(
        "Gray fake agent received: **{prompt}**\n\nRunning in `{}` with model `{}` and mode `{}` in session `{session}`.\n\n```rust\nfn main() {{\n    println!(\"hello from gray\");\n}}\n```\n",
        req.cwd
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        req.model,
        mode_label(&req.mode)
    );
    for chunk in reply.split_inclusive(' ') {
        let _ = tx.send(ProviderEvent::Text(chunk.to_string()));
        thread::sleep(delay);
    }
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
