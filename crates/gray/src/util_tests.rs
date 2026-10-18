use super::*;
use gray_core::redaction::REDACTED;

#[test]
fn redacts_auth_json_values_and_token_shapes() {
    let home = tempfile::tempdir().unwrap();
    let bot = format!("{}.{}.{}", "B".repeat(24), "I".repeat(10), "c".repeat(27));
    let key = format!("sk-{}", "a1".repeat(12));
    std::fs::write(
        home.path().join("auth.json"),
        serde_json::json!({ "discord": { "token": bot } }).to_string(),
    )
    .unwrap();
    let out = redact_secrets(&format!("bot {bot} and {key} ok"), home.path());
    assert!(!out.contains(&bot), "an auth.json value survived");
    assert!(!out.contains(&key), "a key-shaped token survived");
    assert!(out.contains(REDACTED) && out.contains("ok"), "{out}");
}

#[test]
fn redacts_secret_named_assignments_but_not_ordinary_lines() {
    let home = tempfile::tempdir().unwrap();
    let out = redact_secrets("\"api_key\": \"0123456789abcdef\"", home.path());
    assert!(!out.contains("0123456789abcdef") && out.contains(REDACTED));
    let plain = "total 16\ndrwxr-xr-x 2 u u 4096 supervise\ntoken budget: 5";
    assert_eq!(redact_secrets(plain, home.path()), plain);
}

#[test]
fn a_secret_free_transcript_keeps_its_paths_verbatim() {
    let home = tempfile::tempdir().unwrap();
    let t = "[tool:bash]\n[result:exit 0]\n<untrusted-output>\ndrwxr-xr-x 2 u u 4096 /home/u/roo\n</untrusted-output>\nTime to clean.";
    assert_eq!(redact_secrets(t, home.path()), t);
}
