use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

static NEXT: AtomicU32 = AtomicU32::new(0);

fn have(bin: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn sh(dir: &Path, cmd: &str) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "command failed: {cmd}");
}

struct Gray {
    name: String,
    root: PathBuf,
    project: PathBuf,
    home: PathBuf,
    state: PathBuf,
    display: String,
    xvfb: Child,
    app: Option<Child>,
}

impl Gray {
    fn start(name: &str) -> Option<Self> {
        if !have("Xvfb") || !have("xdotool") {
            eprintln!("skipping {name}: Xvfb and xdotool are required for e2e tests");
            return None;
        }
        let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("gray-e2e")
            .join(name);
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let home = root.join("home");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(
            project.join("src/main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        fs::write(project.join("README.md"), "# demo\n").unwrap();
        sh(
            &project,
            "git init -q -b main && git add -A && git -c user.email=e2e@gray -c user.name=e2e commit -qm init",
        );
        fs::write(
            home.join("events.jsonl"),
            "{\"type\":\"setting_changed\",\"key\":\"default_provider\",\"value\":\"fake\"}\n",
        )
        .unwrap();
        let mut n = 300 + (std::process::id() % 100) * 16 + NEXT.fetch_add(1, Ordering::SeqCst);
        while Path::new(&format!("/tmp/.X{n}-lock")).exists() {
            n += 1;
        }
        let xvfb = Command::new("Xvfb")
            .arg(format!(":{n}"))
            .args(["-screen", "0", "1600x1000x24", "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start Xvfb");
        let socket = format!("/tmp/.X11-unix/X{n}");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !Path::new(&socket).exists() && Instant::now() < deadline {
            sleep(Duration::from_millis(50));
        }
        let state = root.join("state.json");
        let mut g = Self {
            name: name.to_string(),
            root,
            project,
            home,
            state,
            display: format!(":{n}"),
            xvfb,
            app: None,
        };
        g.launch();
        Some(g)
    }

    fn launch(&mut self) {
        let _ = fs::remove_file(&self.state);
        let xdg = self.root.join("xdg");
        fs::create_dir_all(&xdg).unwrap();
        let log = fs::File::create(self.root.join("app.log")).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_gray-ide"))
            .arg(&self.project)
            .env("DISPLAY", &self.display)
            .env("VK_ICD_FILENAMES", "/nonexistent.json")
            .env("XDG_RUNTIME_DIR", &xdg)
            .env("GRAY_IDE_HOME", &self.home)
            .env("GRAY_IDE_STATE", &self.state)
            .env("GRAY_IDE_FAKE_DELAY_MS", "5")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("start gray-ide");
        self.app = Some(child);
        self.wait_for("app ready", |s| s["ready"] == true);
        sleep(Duration::from_millis(600));
    }

    fn restart(&mut self) {
        if let Some(mut app) = self.app.take() {
            let _ = app.kill();
            let _ = app.wait();
        }
        self.launch();
    }

    fn state(&self) -> Value {
        fs::read_to_string(&self.state)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null)
    }

    fn wait_for(&self, what: &str, f: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + Duration::from_secs(25);
        loop {
            let s = self.state();
            if !s.is_null() && f(&s) {
                return s;
            }
            if Instant::now() > deadline {
                let shot = self.screenshot("failure");
                panic!(
                    "timed out waiting for {what}\nscreenshot: {}\nstate: {s:#}",
                    shot.display()
                );
            }
            sleep(Duration::from_millis(100));
        }
    }

    fn xdo(&self, args: &[&str]) {
        let ok = Command::new("xdotool")
            .args(args)
            .env("DISPLAY", &self.display)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "xdotool {args:?} failed");
    }

    fn key(&self, k: &str) {
        self.xdo(&["key", "--delay", "40", k]);
        sleep(Duration::from_millis(200));
    }

    fn type_text(&self, text: &str) {
        self.xdo(&["type", "--delay", "20", text]);
        sleep(Duration::from_millis(150));
    }

    fn send(&self, text: &str) {
        self.type_text(text);
        self.key("Return");
    }

    fn palette(&self, query: &str) {
        self.key("ctrl+k");
        self.wait_for("palette open", |s| s["palette_open"] == true);
        self.type_text(query);
        self.key("Return");
        self.wait_for("palette closed", |s| s["palette_open"] == false);
    }

    fn screenshot(&self, label: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("gray-e2e-screenshots");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(format!("{}-{label}.png", self.name));
        let _ = Command::new("import")
            .args(["-window", "root"])
            .arg(&path)
            .env("DISPLAY", &self.display)
            .status();
        path
    }
}

impl Drop for Gray {
    fn drop(&mut self) {
        if let Some(mut app) = self.app.take() {
            let _ = app.kill();
            let _ = app.wait();
        }
        let _ = self.xvfb.kill();
        let _ = self.xvfb.wait();
    }
}

fn messages(s: &Value) -> Vec<(String, String)> {
    s["messages"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|m| {
                    (
                        m["role"].as_str().unwrap_or("").to_string(),
                        m["text"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn finished(s: &Value) -> bool {
    s["running"] == false
        && messages(s)
            .last()
            .map(|m| m.0 == "assistant")
            .unwrap_or(false)
}

#[test]
fn chat_turn_streams_reply_and_persists_across_restart() {
    let Some(mut g) = Gray::start("chat_turn") else {
        return;
    };
    g.send("hello gray");
    let s = g.wait_for("assistant reply", finished);
    let msgs = messages(&s);
    assert_eq!(msgs[0], ("user".into(), "hello gray".into()));
    assert!(
        msgs[1]
            .1
            .contains("Gray fake agent received: **hello gray**")
    );
    assert_eq!(s["thread"]["title"], "hello gray");
    assert_eq!(s["thread"]["provider"], "fake");
    assert!(
        s["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "checkpoint")
    );
    assert_eq!(s["threads"][0]["status"], "completed");
    assert_eq!(s["composer"], "");
    g.screenshot("reply");
    g.restart();
    let s = g.wait_for("restored thread", |s| messages(s).len() == 2);
    assert_eq!(s["thread"]["title"], "hello gray");
    assert_eq!(s["threads"].as_array().unwrap().len(), 1);
}

#[test]
fn keyboard_shortcuts_toggle_every_surface() {
    let Some(g) = Gray::start("shortcuts") else {
        return;
    };
    assert_eq!(g.state()["sidebar_open"], true);
    g.key("ctrl+b");
    g.wait_for("sidebar hidden", |s| s["sidebar_open"] == false);
    g.key("ctrl+b");
    g.wait_for("sidebar shown", |s| s["sidebar_open"] == true);
    g.key("ctrl+j");
    g.wait_for("terminal open", |s| s["terminal_open"] == true);
    g.key("ctrl+j");
    g.wait_for("terminal closed", |s| s["terminal_open"] == false);
    g.key("ctrl+d");
    g.wait_for("diff panel", |s| s["panel"] == "Diff");
    g.key("ctrl+e");
    g.wait_for("files panel", |s| s["panel"] == "Files");
    g.key("ctrl+e");
    g.wait_for("panel closed", |s| s["panel"].is_null());
    g.key("ctrl+comma");
    g.wait_for("settings open", |s| s["settings_open"] == true);
    g.screenshot("settings");
    g.key("Escape");
    g.wait_for("settings closed", |s| s["settings_open"] == false);
    g.key("ctrl+k");
    g.wait_for("palette open", |s| s["palette_open"] == true);
    g.key("Escape");
    g.wait_for("palette closed", |s| s["palette_open"] == false);
    g.key("ctrl+n");
    g.wait_for("new thread", |s| s["thread"]["title"] == "New thread");
}

#[test]
fn palette_switches_provider_model_and_mode() {
    let Some(g) = Gray::start("palette_model") else {
        return;
    };
    g.send("first");
    g.wait_for("reply", finished);
    g.key("ctrl+shift+m");
    let s = g.wait_for("model palette", |s| {
        s["palette_open"] == true && s["palette_query"] == "Model:"
    });
    assert!(
        s["palette_items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i.as_str().unwrap().starts_with("Model:"))
    );
    g.key("Escape");
    g.palette("Model: Claude opus");
    g.wait_for("claude opus", |s| {
        s["thread"]["provider"] == "claude" && s["thread"]["model"] == "opus"
    });
    g.palette("Mode: Read only");
    g.wait_for("read only", |s| s["thread"]["mode"] == "readonly");
    g.palette("Provider: Fake");
    let s = g.wait_for("fake provider", |s| s["thread"]["provider"] == "fake");
    assert_eq!(s["thread"]["model"], "fake-1");
    assert_eq!(s["thread"]["mode"], "readonly");
    g.send("second");
    let s = g.wait_for("second reply", |s| finished(s) && messages(s).len() == 4);
    assert!(messages(&s)[3].1.contains("mode `Read only`"));
    g.screenshot("readonly");
}

#[test]
fn agent_edits_show_in_diff_and_checkpoint_restores() {
    let Some(g) = Gray::start("diff_checkpoint") else {
        return;
    };
    g.send("write notes.txt hello from the agent");
    let s = g.wait_for("edit reply", finished);
    assert!(
        s["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["label"] == "Edited notes.txt")
    );
    assert_eq!(
        fs::read_to_string(g.project.join("notes.txt")).unwrap(),
        "hello from the agent\n"
    );
    g.key("ctrl+d");
    let s = g.wait_for("diff lists file", |s| {
        s["panel"] == "Diff"
            && s["diff_files"]
                .as_array()
                .map(|a| a.iter().any(|f| f == "notes.txt"))
                .unwrap_or(false)
    });
    assert!(
        s["tree"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "notes.txt")
    );
    g.screenshot("diff");
    g.palette("Restore last checkpoint");
    g.wait_for("diff empty", |s| {
        s["diff_files"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false)
    });
    assert!(!g.project.join("notes.txt").exists());
}

#[test]
fn file_picker_opens_edits_and_saves_files() {
    let Some(g) = Gray::start("editor") else {
        return;
    };
    g.key("ctrl+p");
    g.wait_for("file picker", |s| {
        s["palette_open"] == true && s["palette_mode"] == "Files"
    });
    g.type_text("main");
    let s = g.wait_for("filtered", |s| s["palette_items"][0] == "src/main.rs");
    assert_eq!(s["palette_items"].as_array().unwrap().len(), 1);
    g.key("Return");
    g.wait_for("tab open", |s| {
        s["active_tab"] == "src/main.rs" && s["panel"] == "Files"
    });
    g.key("ctrl+Home");
    g.type_text("// gray e2e");
    g.key("Return");
    g.wait_for("editor changed", |s| {
        s["editor_text"]
            .as_str()
            .unwrap_or("")
            .starts_with("// gray e2e\n")
    });
    g.screenshot("dirty");
    g.key("ctrl+s");
    g.wait_for("saved toast", |s| s["toast"] == "Saved src/main.rs");
    let content = fs::read_to_string(g.project.join("src/main.rs")).unwrap();
    assert!(content.starts_with("// gray e2e\nfn main() {"));
    g.wait_for("diff tracks save", |s| {
        s["diff_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "src/main.rs")
    });
}

#[test]
fn terminal_runs_shell_commands_in_project() {
    let Some(g) = Gray::start("terminal") else {
        return;
    };
    g.key("ctrl+j");
    g.wait_for("terminal", |s| s["terminal_open"] == true);
    g.type_text("echo gray-e2e && pwd");
    g.key("Return");
    let s = g.wait_for("output", |s| {
        s["terminal"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l == "[exit 0]")
    });
    let lines: Vec<&str> = s["terminal"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l.as_str())
        .collect();
    assert!(lines.contains(&"$ echo gray-e2e && pwd"));
    assert!(lines.contains(&"gray-e2e"));
    assert!(lines.iter().any(|l| l.ends_with("/project")));
    g.screenshot("terminal");
    g.type_text("touch made-by-terminal.txt");
    g.key("Return");
    g.wait_for("tree refresh", |s| {
        s["tree"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "made-by-terminal.txt")
    });
}

#[test]
fn slash_commands_manage_thread_lifecycle_and_branches() {
    let Some(g) = Gray::start("lifecycle") else {
        return;
    };
    g.send("alpha task");
    g.wait_for("alpha", finished);
    g.send("/rename Alpha renamed");
    g.wait_for("renamed", |s| s["thread"]["title"] == "Alpha renamed");
    g.send("/pin");
    g.wait_for("pinned", |s| s["thread"]["pinned"] == true);
    g.key("ctrl+n");
    g.wait_for("new thread", |s| s["thread"]["title"] == "New thread");
    g.send("beta task");
    let s = g.wait_for("beta", |s| {
        finished(s) && s["thread"]["title"] == "beta task"
    });
    let titles: Vec<&str> = s["threads"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["title"].as_str())
        .collect();
    assert!(titles.contains(&"Alpha renamed") && titles.contains(&"beta task"));
    g.send("/archive");
    let s = g.wait_for("archived", |s| {
        s["threads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"] == "beta task" && t["archived"] == true)
    });
    assert!(s["thread"].is_null());
    g.send("/branch feature-x");
    g.wait_for("branch created", |s| s["branch"] == "feature-x");
    g.send("/checkout main");
    let s = g.wait_for("back on main", |s| s["branch"] == "main");
    assert!(
        s["branches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b == "feature-x")
    );
    g.screenshot("lifecycle");
}

#[test]
fn failed_turn_surfaces_error_and_theme_persists() {
    let Some(mut g) = Gray::start("errors_theme") else {
        return;
    };
    g.send("please fail");
    let s = g.wait_for("error", |s| {
        s["running"] == false && s["threads"][0]["status"] == "error"
    });
    assert!(
        s["activities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "error" && a["detail"] == "fake provider failure requested")
    );
    assert!(s["toast"].as_str().unwrap().starts_with("Turn failed"));
    g.screenshot("error");
    assert_eq!(s["theme"], "dark");
    g.key("ctrl+shift+l");
    g.wait_for("light", |s| s["theme"] == "light");
    g.screenshot("light");
    g.restart();
    g.wait_for("light after restart", |s| s["theme"] == "light");
}
