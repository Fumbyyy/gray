use std::fs::OpenOptions;
use std::path::Path;

pub fn now_secs() -> i64 {
    gray_core::spill::now_secs()
}

pub fn atomic_write_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    use std::io::Write as _;
    let body = serde_json::to_string_pretty(value)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(".gray-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> anyhow::Result<()> {
        let mut file = options.open(&tmp)?;
        file.write_all(body.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn redact_secrets(text: &str, home: &std::path::Path) -> String {
    let mut out = text.to_string();
    for secret in known_secrets(home) {
        out = out.replace(&secret, gray_core::redaction::REDACTED);
    }
    let redacted = gray_core::redaction::redact_for_disclosure(&out);
    if redacted.has_secret() {
        redacted.into_text()
    } else {
        out
    }
}

fn known_secrets(home: &std::path::Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(home.join("auth.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    collect_strings(&v, &mut found);
    found.retain(|s| s.chars().count() >= 16 && !s.chars().any(char::is_whitespace));
    found.sort_by_key(|s| std::cmp::Reverse(s.len()));
    found.dedup();
    found
}

fn collect_strings(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_strings(x, out)),
        serde_json::Value::Object(m) => m.values().for_each(|x| collect_strings(x, out)),
        _ => {}
    }
}

#[path = "util_tests.rs"]
#[cfg(test)]
mod tests;
