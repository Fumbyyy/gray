// Hermes-style attention inbox + per-thread message queue. Pure logic.
use crate::store::{Item, State, Thread};
use std::collections::{HashMap, HashSet, VecDeque};

/// Attention severity ordering: higher variants sort first in the inbox.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Phase {
    Idle = 0,
    Snoozed = 1,
    Working = 2,
    ReadyUnread = 3,
    Errored = 4,
    Blocked = 5,
}

/// Compute a thread's current attention phase.
///
/// Precedence: Blocked (id in `blocked`) > Errored (last_status == "error" and
/// not settled) > ReadyUnread (id in `unread`) > Working (id in `running`) >
/// Snoozed (snoozed_until > now_ms) > Idle.
pub fn phase_of(
    t: &Thread,
    running: &HashSet<String>,
    blocked: &HashSet<String>,
    unread: &HashSet<String>,
    now_ms: i64,
) -> Phase {
    if blocked.contains(&t.id) {
        Phase::Blocked
    } else if t.last_status == "error" && !t.settled {
        Phase::Errored
    } else if unread.contains(&t.id) {
        Phase::ReadyUnread
    } else if running.contains(&t.id) {
        Phase::Working
    } else if t.snoozed_until > now_ms {
        Phase::Snoozed
    } else {
        Phase::Idle
    }
}

#[derive(Clone, Debug)]
pub struct AttentionItem {
    pub thread_id: String,
    pub project_id: String,
    pub title: String,
    pub phase: Phase,
    pub detail: String,
    pub updated_at: i64,
}

/// Collapse whitespace and truncate to ~`max` chars with an ellipsis.
fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flat.chars();
    let taken: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

/// Last activity label or last message preview (~80 chars, whitespace-collapsed).
fn detail_of(t: &Thread) -> String {
    for item in t.items.iter().rev() {
        let candidate = match item {
            Item::Activity { label, detail, .. } => {
                if label.trim().is_empty() {
                    detail.as_str()
                } else {
                    label.as_str()
                }
            }
            Item::Message { text, .. } => text.as_str(),
        };
        let p = preview(candidate, 80);
        if !p.is_empty() {
            return p;
        }
    }
    String::new()
}

/// All non-archived, non-settled threads whose phase != Idle (Working included),
/// sorted by phase desc then updated_at desc.
pub fn attention(
    state: &State,
    running: &HashSet<String>,
    blocked: &HashSet<String>,
    unread: &HashSet<String>,
    now_ms: i64,
) -> Vec<AttentionItem> {
    let mut items: Vec<AttentionItem> = state
        .threads
        .iter()
        .filter(|t| !t.archived && !t.settled)
        .filter_map(|t| {
            let phase = phase_of(t, running, blocked, unread, now_ms);
            (phase != Phase::Idle).then(|| AttentionItem {
                thread_id: t.id.clone(),
                project_id: t.project_id.clone(),
                title: t.title.clone(),
                phase,
                detail: detail_of(t),
                updated_at: t.updated_at,
            })
        })
        .collect();
    items.sort_by(|a, b| {
        b.phase
            .cmp(&a.phase)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    items
}

/// FIFO queue of pending user messages per thread (steer-while-running).
#[derive(Default)]
pub struct MessageQueue {
    map: HashMap<String, VecDeque<String>>,
}

impl MessageQueue {
    pub fn push(&mut self, thread_id: &str, text: String) {
        self.map
            .entry(thread_id.to_string())
            .or_default()
            .push_back(text);
    }

    pub fn pop(&mut self, thread_id: &str) -> Option<String> {
        let q = self.map.get_mut(thread_id)?;
        let out = q.pop_front();
        if q.is_empty() {
            self.map.remove(thread_id);
        }
        out
    }

    /// Drop every queued message across all threads.
    pub fn clear_all(&mut self) {
        self.map.clear();
    }

    pub fn len(&self, thread_id: &str) -> usize {
        self.map.get(thread_id).map_or(0, VecDeque::len)
    }
}
