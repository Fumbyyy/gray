use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ProjectAdded {
        id: String,
        name: String,
        path: String,
        at: i64,
    },
    ProjectRemoved {
        id: String,
    },
    ThreadCreated {
        id: String,
        project_id: String,
        title: String,
        provider: String,
        model: String,
        mode: String,
        at: i64,
    },
    ThreadRenamed {
        id: String,
        title: String,
    },
    ThreadPinned {
        id: String,
        pinned: bool,
    },
    ThreadArchived {
        id: String,
        archived: bool,
    },
    ThreadDeleted {
        id: String,
    },
    ThreadConfigured {
        id: String,
        provider: String,
        model: String,
        mode: String,
    },
    MessageAdded {
        thread_id: String,
        id: String,
        role: Role,
        text: String,
        at: i64,
    },
    ActivityAdded {
        thread_id: String,
        id: String,
        kind: String,
        label: String,
        detail: String,
        at: i64,
    },
    TurnFinished {
        thread_id: String,
        status: String,
        at: i64,
    },
    SettingChanged {
        key: String,
        value: String,
    },
}

#[derive(Clone, Debug)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
}

#[derive(Clone, Debug)]
pub enum Item {
    Message {
        id: String,
        role: Role,
        text: String,
    },
    Activity {
        kind: String,
        label: String,
        detail: String,
    },
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider: String,
    pub model: String,
    pub mode: String,
    pub pinned: bool,
    pub archived: bool,
    pub items: Vec<Item>,
    pub updated_at: i64,
    pub last_status: String,
}

#[derive(Default, Clone, Debug)]
pub struct State {
    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    pub settings: BTreeMap<String, String>,
}

impl State {
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::ProjectAdded { id, name, path, .. } => {
                if !self.projects.iter().any(|p| &p.id == id) {
                    self.projects.push(Project {
                        id: id.clone(),
                        name: name.clone(),
                        path: path.clone(),
                    });
                }
            }
            Event::ProjectRemoved { id } => {
                self.projects.retain(|p| &p.id != id);
                self.threads.retain(|t| &t.project_id != id);
            }
            Event::ThreadCreated {
                id,
                project_id,
                title,
                provider,
                model,
                mode,
                at,
            } => self.threads.push(Thread {
                id: id.clone(),
                project_id: project_id.clone(),
                title: title.clone(),
                provider: provider.clone(),
                model: model.clone(),
                mode: mode.clone(),
                pinned: false,
                archived: false,
                items: Vec::new(),
                updated_at: *at,
                last_status: String::new(),
            }),
            Event::ThreadRenamed { id, title } => {
                if let Some(t) = self.thread_mut(id) {
                    t.title = title.clone();
                }
            }
            Event::ThreadPinned { id, pinned } => {
                if let Some(t) = self.thread_mut(id) {
                    t.pinned = *pinned;
                }
            }
            Event::ThreadArchived { id, archived } => {
                if let Some(t) = self.thread_mut(id) {
                    t.archived = *archived;
                }
            }
            Event::ThreadDeleted { id } => self.threads.retain(|t| &t.id != id),
            Event::ThreadConfigured {
                id,
                provider,
                model,
                mode,
            } => {
                if let Some(t) = self.thread_mut(id) {
                    t.provider = provider.clone();
                    t.model = model.clone();
                    t.mode = mode.clone();
                }
            }
            Event::MessageAdded {
                thread_id,
                id,
                role,
                text,
                at,
            } => {
                if let Some(t) = self.thread_mut(thread_id) {
                    t.items.push(Item::Message {
                        id: id.clone(),
                        role: role.clone(),
                        text: text.clone(),
                    });
                    t.updated_at = *at;
                }
            }
            Event::ActivityAdded {
                thread_id,
                kind,
                label,
                detail,
                at,
                ..
            } => {
                if let Some(t) = self.thread_mut(thread_id) {
                    t.items.push(Item::Activity {
                        kind: kind.clone(),
                        label: label.clone(),
                        detail: detail.clone(),
                    });
                    t.updated_at = *at;
                }
            }
            Event::TurnFinished {
                thread_id,
                status,
                at,
            } => {
                if let Some(t) = self.thread_mut(thread_id) {
                    t.last_status = status.clone();
                    t.updated_at = *at;
                }
            }
            Event::SettingChanged { key, value } => {
                self.settings.insert(key.clone(), value.clone());
            }
        }
    }

    pub fn thread(&self, id: &str) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    pub fn thread_mut(&mut self, id: &str) -> Option<&mut Thread> {
        self.threads.iter_mut().find(|t| t.id == id)
    }

    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn setting(&self, key: &str, default: &str) -> String {
        self.settings
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    pub fn threads_for(&self, project_id: &str, archived: bool) -> Vec<&Thread> {
        let mut list: Vec<&Thread> = self
            .threads
            .iter()
            .filter(|t| t.project_id == project_id && t.archived == archived)
            .collect();
        list.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.updated_at.cmp(&a.updated_at))
        });
        list
    }
}

pub struct Store {
    path: PathBuf,
    pub state: State,
}

impl Store {
    pub fn open(dir: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(dir)?;
        let path = dir.join("events.jsonl");
        let mut state = State::default();
        if let Ok(file) = fs::File::open(&path) {
            for line in BufReader::new(file).lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Event>(&line) {
                    state.apply(&event);
                }
            }
        }
        Ok(Self { path, state })
    }

    pub fn append(&mut self, event: Event) {
        self.state.apply(&event);
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            && let Ok(line) = serde_json::to_string(&event)
        {
            let _ = writeln!(file, "{line}");
        }
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
