use std::path::Path;
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        None
    }
}

pub fn is_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"]).is_some()
}

pub fn branch(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn branches(dir: &Path) -> Vec<String> {
    git(dir, &["branch", "--format=%(refname:short)"])
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn checkout(dir: &Path, name: &str, create: bool) -> Result<(), String> {
    let mut args = vec!["checkout"];
    if create {
        args.push("-b");
    }
    args.push(name);
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(&args)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn status(dir: &Path) -> Vec<(String, String)> {
    git(dir, &["status", "--porcelain=v1", "-uall"])
        .map(|s| {
            s.lines()
                .filter(|l| l.len() > 3)
                .map(|l| (l[..2].trim().to_string(), l[3..].to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn diff(dir: &Path) -> String {
    let mut text = git(dir, &["diff", "HEAD", "--no-color"])
        .or_else(|| git(dir, &["diff", "--no-color"]))
        .unwrap_or_default();
    for (code, path) in status(dir) {
        if code == "??"
            && let Ok(d) = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["diff", "--no-color", "--no-index", "/dev/null", &path])
                .output()
        {
            text.push_str(&String::from_utf8_lossy(&d.stdout));
        }
    }
    text
}

pub fn checkpoint(dir: &Path) -> Option<String> {
    git(dir, &["add", "-A"])?;
    let tree = git(dir, &["write-tree"])?.trim().to_string();
    let _ = git(dir, &["reset", "-q"]);
    Some(tree)
}

pub fn restore(dir: &Path, tree: &str) -> Result<(), String> {
    let run = |args: &[&str]| -> Result<(), String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    };
    run(&["add", "-A"])?;
    run(&["read-tree", "-u", "--reset", tree])?;
    run(&["reset", "-q"])
}
