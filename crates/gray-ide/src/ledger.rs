// Turn-ledger / review-desk helpers over checkpoint Activity items + git.
use crate::store::{Item, Role, Thread};
use std::path::Path;
use std::process::Command;

/// Extract the tree hash from a checkpoint Activity's detail string:
/// the first whitespace-separated token of the first line.
fn tree_of(detail: &str) -> Option<String> {
    detail
        .lines()
        .next()?
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Files changed relative to the given tree.
pub fn files_from(root: &Path, tree: &str) -> Vec<String> {
    crate::git::files_since(root, tree)
}

/// Restore the workspace to `tree`. SAFETY: first take a git checkpoint of the
/// current workspace; the returned tree is usable to undo the restore
/// (None when `root` is not a repo, in which case the restore is still
/// attempted but cannot be undone).
pub fn restore_to(root: &Path, tree: &str) -> Result<Option<String>, String> {
    let undo = crate::git::checkpoint(root);
    crate::git::restore(root, tree)?;
    Ok(undo)
}

/// Stage everything and commit; returns the short hash of HEAD.
/// Err carries the failing command's stderr.
pub fn commit_all(root: &Path, message: &str) -> Result<String, String> {
    let run = |args: &[&str]| -> Result<String, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    run(&["add", "-A"])?;
    run(&["commit", "-m", message])?;
    run(&["rev-parse", "--short", "HEAD"])
}

/// One user turn's slice of the item stream.
#[derive(Clone, Debug)]
pub struct TurnSpan {
    /// Ordinal index of the span (0-based).
    pub index: usize,
    /// Tree of any checkpoint Activity inside the span (last one wins).
    pub checkpoint: Option<String>,
    /// First Activity label in the span, else a preview of the user message.
    pub first_label: String,
    /// "latest" for the final span, "done" for earlier spans.
    pub status: String,
}

fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(max).collect()
}

/// Group items into turn spans: a span starts at each user Message and runs
/// until the next user Message. Items before the first user message are
/// ignored. Spans are per-thread, so the last span is "latest" and all
/// earlier spans are "done" (no per-span finish status is stored).
pub fn turn_spans(t: &Thread) -> Vec<TurnSpan> {
    struct Acc {
        user_text: String,
        checkpoint: Option<String>,
        activity_label: String,
    }

    let mut accs: Vec<Acc> = Vec::new();
    for item in &t.items {
        if matches!(
            item,
            Item::Message {
                role: Role::User,
                ..
            }
        ) {
            accs.push(Acc {
                user_text: String::new(),
                checkpoint: None,
                activity_label: String::new(),
            });
        }
        let Some(acc) = accs.last_mut() else {
            continue; // items before the first user message
        };
        match item {
            Item::Message {
                role: Role::User,
                text,
                ..
            } => {
                if acc.user_text.is_empty() {
                    acc.user_text = text.clone();
                }
            }
            Item::Activity {
                kind,
                label,
                detail,
            } => {
                if acc.activity_label.is_empty() && !label.trim().is_empty() {
                    acc.activity_label = label.trim().to_string();
                }
                if kind == "checkpoint"
                    && let Some(tree) = tree_of(detail)
                {
                    acc.checkpoint = Some(tree);
                }
            }
            _ => {}
        }
    }

    let n = accs.len();
    accs.into_iter()
        .enumerate()
        .map(|(i, acc)| TurnSpan {
            index: i,
            checkpoint: acc.checkpoint,
            first_label: if acc.activity_label.is_empty() {
                preview(&acc.user_text, 60)
            } else {
                acc.activity_label
            },
            status: if i + 1 == n { "latest" } else { "done" }.to_string(),
        })
        .collect()
}
