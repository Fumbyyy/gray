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
    if let Some(home) = crate::home_dir() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gray-ide-skills-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_skill(dir: &Path, name: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: d\n---\n\nbody\n"),
        )
        .unwrap();
    }

    /// A directory symlink back into the tree must not recurse forever and
    /// must surface the skill once. Windows symlink creation needs elevated
    /// privileges or Developer Mode, so it degrades to a plain scan there.
    #[test]
    fn a_symlink_cycle_terminates() {
        let root = tempdir("cycle");
        let inner = root.join("skills").join("a");
        write_skill(&inner, "cycleskill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&inner, inner.join("loop")).unwrap();
        #[cfg(windows)]
        let _ = std::os::windows::fs::symlink_dir(&inner, inner.join("loop"));

        let mut out = Vec::new();
        let mut visited = std::collections::HashSet::new();
        scan_dir(&root.join("skills"), "user", &mut out, &mut visited);
        let _ = fs::remove_dir_all(&root);

        let names: Vec<&str> = out.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["cycleskill"]);
    }

    /// A dir reachable both as the agent dir's `skills/` and as a project
    /// ancestor is scanned once and keeps the earlier source label.
    #[test]
    fn shared_root_scans_once() {
        let root = tempdir("dedup");
        let agent_dir = root.join(".agents");
        write_skill(&agent_dir.join("skills").join("dedupskill"), "dedupskill");

        let found = discover(Some(&root), &agent_dir);
        let _ = fs::remove_dir_all(&root);

        let hits: Vec<&Skill> = found.iter().filter(|s| s.name == "dedupskill").collect();
        assert_eq!(hits.len(), 1, "expected one dedupskill");
        assert_eq!(hits[0].source, "user");
    }

    /// A project's `.claude/skills` dir is discovered at any ancestor.
    #[test]
    fn discovers_project_claude_skills() {
        let root = tempdir("claude");
        let work = root.join("proj").join("sub");
        fs::create_dir_all(&work).unwrap();
        write_skill(
            &root.join("proj").join(".claude").join("skills").join("cdesk"),
            "cdesk",
        );

        let found = discover(Some(&work), &root.join("agent"));
        let _ = fs::remove_dir_all(&root);

        let hits: Vec<&Skill> = found
            .iter()
            .filter(|s| s.name == "cdesk" && s.file.starts_with(&root))
            .collect();
        assert_eq!(hits.len(), 1, "expected one project cdesk");
        assert_eq!(hits[0].source, "project");
    }
}
