use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub rel: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
}

const SKIP: &[&str] = &[".git", "target", "node_modules", ".gray"];

pub fn tree(root: &Path, expanded: &HashSet<String>) -> Vec<FileEntry> {
    let mut out = Vec::new();
    walk(root, root, 0, expanded, &mut out);
    out
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    expanded: &HashSet<String>,
    out: &mut Vec<FileEntry>,
) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| (!e.path().is_dir(), e.file_name()));
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let path = e.path();
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let is_dir = path.is_dir();
        out.push(FileEntry {
            rel: rel.clone(),
            name,
            depth,
            is_dir,
        });
        if is_dir && expanded.contains(&rel) {
            walk(root, &path, depth + 1, expanded, out);
        }
    }
}

pub fn all_files(root: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    collect(root, root, &mut out, limit);
    out.sort();
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>, limit: usize) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for e in read.filter_map(Result::ok) {
        if out.len() >= limit {
            return;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            collect(root, &path, out, limit);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().to_string());
        }
    }
}

pub fn language_for(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" => "javascript",
        "json" => "json",
        "toml" => "toml",
        "md" => "markdown",
        "py" => "python",
        "go" => "go",
        "sh" | "bash" => "bash",
        "yml" | "yaml" => "yaml",
        "html" => "html",
        "css" => "css",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        _ => "text",
    }
}

pub fn run_shell(cwd: &Path, command: &str) -> Receiver<String> {
    let (tx, rx) = channel();
    let cwd = cwd.to_path_buf();
    let command = command.to_string();
    thread::spawn(move || {
        let child = Command::new("sh")
            .arg("-c")
            .arg(format!("{command} 2>&1"))
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn();
        match child {
            Ok(mut child) => {
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        let _ = tx.send(line);
                    }
                }
                let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
                let _ = tx.send(format!("[exit {code}]"));
            }
            Err(e) => {
                let _ = tx.send(format!("error: {e}"));
            }
        }
    });
    rx
}
