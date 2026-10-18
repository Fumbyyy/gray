use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clap::Parser;

use gray::ide::{IdeSession, IdeUsage, SubmitOutcome, UiCommand};
use gray::session_store::{JsonlSessionStore, SessionSummary};
use gray_core::event::AgentEvent;
use gray_core::message::Message;

pub enum IdeCommand {
    Submit(String),
    RefreshSessions,
    ListModels,
}

pub enum IdeEvent {
    Ready {
        label: String,
    },
    Label(String),
    Fatal(String),
    Sessions(Vec<SessionSummary>),
    Sync {
        session: Option<String>,
        history: Vec<Message>,
    },
    UserEcho(String),
    Output(String),
    Agent(AgentEvent),
    Models(Vec<(String, String)>),
    Usage(IdeUsage),
    TurnDone {
        error: Option<String>,
    },
    Ui(UiCommand),
    Quit,
}

#[derive(Clone)]
pub struct IdeHandle {
    cmd: flume::Sender<IdeCommand>,
    steer: std::sync::mpsc::SyncSender<String>,
}

impl IdeHandle {
    pub fn submit(&self, text: String) {
        // Mid-turn input steers the running turn — except commands the TUI
        // runs locally mid-turn (/usage, /jobs, /help, …), which must not be
        // fed to the model as prompt text. Those queue behind the turn.
        if gray::ide::turn_in_flight() && !gray::ide::is_turn_local_command(&text) {
            if self.steer.try_send(text.clone()).is_err() {
                // Steer channel full or disconnected — queue the input to run
                // after the turn instead of silently dropping it.
                let _ = self.cmd.send(IdeCommand::Submit(text));
            }
        } else {
            let _ = self.cmd.send(IdeCommand::Submit(text));
        }
    }

    pub fn send(&self, cmd: IdeCommand) {
        let _ = self.cmd.send(cmd);
    }

    pub fn cancel(&self) {
        gray::ide::cancel_turn();
    }
}

pub fn spawn(cwd: PathBuf) -> (IdeHandle, flume::Receiver<IdeEvent>) {
    let (cmd_tx, cmd_rx) = flume::unbounded::<IdeCommand>();
    let (evt_tx, evt_rx) = flume::unbounded::<IdeEvent>();
    let (steer_tx, steer_rx) = std::sync::mpsc::sync_channel::<String>(64);
    let handle = IdeHandle {
        cmd: cmd_tx.clone(),
        steer: steer_tx,
    };
    std::thread::Builder::new()
        .name("gray-agent".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("agent runtime");
            rt.block_on(agent_thread(cmd_tx, cmd_rx, evt_tx, steer_rx, cwd));
        })
        .expect("spawn agent thread");
    (handle, evt_rx)
}

async fn agent_thread(
    cmd_tx: flume::Sender<IdeCommand>,
    cmd_rx: flume::Receiver<IdeCommand>,
    evt: flume::Sender<IdeEvent>,
    steer_rx: std::sync::mpsc::Receiver<String>,
    cwd: PathBuf,
) {
    let cli = gray::Cli::parse_from(["gray-ide"]);
    let config = match gray::Config::resolve(&cli) {
        Ok(c) => c,
        Err(e) => {
            let _ = evt.send(IdeEvent::Fatal(format!("config: {e:#}")));
            return;
        }
    };

    let mut session = IdeSession::new(config, cwd.clone());
    let _ = evt.send(IdeEvent::Ready {
        label: session.model_label(),
    });

    let Some(root) = gray::session_store::default_root() else {
        let _ = evt.send(IdeEvent::Fatal("no GRAY_HOME / sessions root".into()));
        return;
    };
    let store = JsonlSessionStore::new(root);
    let _ = evt.send(IdeEvent::Sessions(store.list().await));

    let sink_evt = evt.clone();
    gray::ide::set_output_sink(Some(Box::new(move |line: &str| {
        let _ = sink_evt.send(IdeEvent::Output(line.to_string()));
    })));
    let tap_evt = evt.clone();
    gray::ide::set_event_tap(Some(Box::new(move |ev: &AgentEvent| {
        let _ = tap_evt.send(IdeEvent::Agent(ev.clone()));
    })));
    let steer_rx = Arc::new(Mutex::new(steer_rx));
    let steer_src = steer_rx.clone();
    gray::ide::set_steer_source(Some(Box::new(move || {
        steer_src.lock().ok().and_then(|r| r.try_recv().ok())
    })));

    let mut notices = tokio::time::interval(std::time::Duration::from_millis(250));
    notices.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    // Transcript fingerprint: session id + history rewrite revision. Appends
    // (normal turns) don't bump the revision — only new/resumed sessions and
    // rewrites (undo, compaction) do — so the UI resyncs exactly when the
    // transcript can no longer be trusted.
    let mut last_fp: (Option<String>, u64) = (None, 0);
    let mut last_label = session.model_label();
    let mut idle_ticks = 0u8;
    loop {
        tokio::select! {
            cmd = cmd_rx.recv_async() => {
                let Ok(cmd) = cmd else { break };
                match cmd {
                    IdeCommand::RefreshSessions => {
                        let _ = evt.send(IdeEvent::Sessions(store.list().await));
                    }
                    IdeCommand::ListModels => {
                        let _ = evt.send(IdeEvent::Models(session.catalog_models()));
                    }
                    IdeCommand::Submit(line) => {
                        // The TUI retracts the echo of commands that run
                        // locally; mirror that instead of showing them as
                        // user bubbles.
                        if !line.trim().is_empty()
                            && !IdeSession::parse(&line).runs_local()
                        {
                            let _ = evt.send(IdeEvent::UserEcho(line.clone()));
                        }
                        let result = session.submit(&line).await;
                        if let Ok(rx) = steer_rx.lock() {
                            while let Ok(text) = rx.try_recv() {
                                let _ = cmd_tx.send(IdeCommand::Submit(text));
                            }
                        }
                        let (error, outcome) = match result {
                            Ok(o) => (None, o),
                            Err(e) => (Some(format!("{e:#}")), SubmitOutcome::Done),
                        };
                        emit_tail(&evt, &session, &store, &mut last_fp, &mut last_label, error)
                            .await;
                        match outcome {
                            SubmitOutcome::Done => {}
                            SubmitOutcome::NeedsUi(ui) => {
                                let _ = evt.send(IdeEvent::Ui(ui));
                            }
                            SubmitOutcome::Quit => {
                                let _ = evt.send(IdeEvent::Quit);
                                break;
                            }
                        }
                    }
                }
            }
            _ = notices.tick() => {
                session.drain_notices();
                // The repl drains wake cards / inbound sends every input
                // read; poll them here on a slower cadence (they hit the
                // disk) or immediately when something requested a wake.
                idle_ticks = idle_ticks.wrapping_add(1);
                if gray::ide::take_wake() || idle_ticks >= 4 {
                    idle_ticks = 0;
                    match session.poll_idle().await {
                        Ok(true) => {
                            emit_tail(&evt, &session, &store, &mut last_fp, &mut last_label, None)
                                .await;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            emit_tail(
                                &evt,
                                &session,
                                &store,
                                &mut last_fp,
                                &mut last_label,
                                Some(format!("{e:#}")),
                            )
                            .await;
                        }
                    }
                }
            }
        }
    }
    gray::ide::set_output_sink(None);
    gray::ide::set_event_tap(None);
    gray::ide::set_steer_source(None);
}

async fn emit_tail(
    evt: &flume::Sender<IdeEvent>,
    session: &IdeSession,
    store: &JsonlSessionStore,
    last_fp: &mut (Option<String>, u64),
    last_label: &mut String,
    error: Option<String>,
) {
    let label = session.model_label();
    if label != *last_label {
        *last_label = label.clone();
        let _ = evt.send(IdeEvent::Label(label));
    }
    let sid = session.session_id();
    let fp = (sid.clone(), session.history_rev());
    if fp != *last_fp {
        *last_fp = fp;
        let _ = evt.send(IdeEvent::Sync {
            session: sid,
            history: session.history().to_vec(),
        });
    }
    let _ = evt.send(IdeEvent::TurnDone { error });
    let _ = evt.send(IdeEvent::Usage(session.totals()));
    let _ = evt.send(IdeEvent::Sessions(store.list().await));
}
