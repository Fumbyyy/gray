use std::fs;
use std::path::{Path, PathBuf};

pub struct Skill {
    pub name: String,
    pub description: String,
    pub source: String,
    pub file: PathBuf,
    pub body: String,
}

fn parse_skill(file: &Path, source: &str) -> Option<Skill> {
    let raw = fs::read_to_string(file).ok()?;
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let rest = &trimmed[3..];
    let end = rest.find("\n---")?;
    let fm = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n').to_string();
    let mut name = None;
    let mut desc = None;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            desc = Some(v.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }
    let desc = desc.filter(|d| !d.is_empty())?;
    let dir_name = file
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    Some(Skill {
        name: name.filter(|n| !n.is_empty()).unwrap_or(dir_name),
        description: desc,
        source: source.to_string(),
        file: file.to_path_buf(),
        body,
    })
}

fn scan_dir(
    dir: &Path,
    source: &str,
    out: &mut Vec<Skill>,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    let canon = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canon) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md")
            && let Some(s) = parse_skill(&path, source)
        {
            out.push(s);
        }
        if path.is_dir() {
            scan_dir(&path, source, out, visited);
        }
    }
}

/// Home-relative skill dirs shared with other agent CLIs (same set the CLI
/// scans in `crates/gray/src/skills/mod.rs`).
const USER_AGENT_SKILL_DIRS: &[&str] = &[
    ".claude/skills",
    ".codex/skills",
    ".cursor/skills",
    ".openclaw/skills",
    ".hermes/skills",
];

/// Project-relative config dirs scanned for `skills/` at each ancestor.
const PROJECT_SKILL_DIRS: &[&str] = &[
    ".gray",
    ".claude",
    ".codex",
    ".cursor",
    ".devin",
    ".opencode",
    ".agents",
    ".openclaw",
    ".hermes",
];

pub fn discover(work_root: Option<&Path>, agent_dir: &Path) -> Vec<Skill> {
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut scan = |dir: &Path, source: &str| scan_dir(dir, source, &mut out, &mut visited);
    scan(&agent_dir.join("skills"), "user");
    if let Some(home) = dirs_home() {
        scan(&home.join(".agents").join("skills"), "user");
        let config_base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| home.join(".config"));
        let opencode = config_base.join("opencode");
        if let Ok(entries) = fs::read_dir(&opencode) {
            let mut dirs: Vec<PathBuf> = vec![opencode.clone()];
            dirs.extend(entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            for d in dirs {
                scan(&d.join("skills"), "user");
            }
        } else {
            scan(&opencode.join("skills"), "user");
        }
        scan(&config_base.join("devin").join("skills"), "user");
        for rel in USER_AGENT_SKILL_DIRS {
            scan(&home.join(rel), "user");
        }
    }
    if let Some(root) = work_root {
        let mut dir = Some(root.to_path_buf());
        while let Some(d) = dir {
            for cfg in PROJECT_SKILL_DIRS {
                scan(&d.join(cfg).join("skills"), "project");
            }
            dir = d.parent().map(|p| p.to_path_buf());
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|s| {
        let real = fs::canonicalize(&s.file).unwrap_or_else(|_| s.file.clone());
        seen.insert(real)
    });
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn dirs_home() -> Option<PathBuf> {
    // Same precedence as `gray_core::paths::user_home` so the IDE and CLI
    // resolve the same home when both HOME and USERPROFILE are set.
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
