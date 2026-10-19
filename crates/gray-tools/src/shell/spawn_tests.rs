use super::*;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
#[test]
fn setsid_failure_is_an_error() {
    assert!(check_setsid(-1).is_err());
    assert!(check_setsid(1234).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_records_real_process_group() {
    let spawned = spawn("true", &PathBuf::from("/tmp"), None, None, None, None)
        .expect("sh -c true must spawn");
    assert_eq!(spawned.pgid, spawned.pid as i32);
}

#[test]
fn prefix_splits_on_whitespace() {
    assert_eq!(
        split_prefix("docker exec -i dev sh -s").unwrap(),
        vec!["docker", "exec", "-i", "dev", "sh", "-s"]
    );
    assert!(split_prefix("   ").unwrap().is_empty());
}

#[test]
fn prefix_honours_quotes_so_a_box_name_may_contain_a_space() {
    assert_eq!(
        split_prefix(r#"ssh -p 2222 'my box' sh -s"#).unwrap(),
        vec!["ssh", "-p", "2222", "my box", "sh", "-s"]
    );
    assert_eq!(split_prefix(r#"sh -s"#).unwrap(), vec!["sh", "-s"]);
    assert_eq!(split_prefix(r#"echo "a b""#).unwrap(), vec!["echo", "a b"]);
}

#[test]
fn an_unterminated_quote_is_an_error_not_a_truncated_prefix() {
    assert!(split_prefix("ssh 'my box").is_err());
    assert!(split_prefix("ssh box\\").is_err());
}

#[test]
fn exported_values_are_posix_quoted() {
    assert_eq!(shell_quote("/tmp/gray report"), "'/tmp/gray report'");
    assert_eq!(shell_quote("it's"), r"'it'\''s'");
}

#[cfg(unix)]
#[tokio::test]
async fn exec_prefix_runs_the_command_on_the_far_shell() {
    let spawned = spawn_with_prefix(
        "echo hello-from-prefix",
        &PathBuf::from("/tmp"),
        None,
        None,
        Some(vec!["sh".into(), "-s".into()]),
        None,
        None,
    )
    .expect("prefixed spawn must succeed");
    let out = spawned.child.wait_with_output().await.expect("child");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hello-from-prefix"),
        "prefixed run produced {:?}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn exec_prefix_carries_gray_env_across_the_boundary() {
    let report = std::env::temp_dir().join("gray-exec-env-test.txt");
    let spawned = spawn_with_prefix(
        r#"case "$GRAY_CWD_REPORT" in /*) echo report-ok;; *) echo report-missing;; esac"#,
        &PathBuf::from("/tmp"),
        Some("sess-1"),
        Some(&report),
        Some(vec!["sh".into(), "-s".into()]),
        None,
        None,
    )
    .expect("prefixed spawn must succeed");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("report-ok"), "prefixed env: {stdout:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_heredoc_survives_the_prefix_boundary_intact() {
    let dir = std::env::temp_dir().join(format!("gray-exec-heredoc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let file = dir.join("body.txt");
    let file = file.display();
    let script = format!("cat > {file} <<'EOF'\nline one\nEOF\nwc -l < {file}");
    let spawned = spawn_with_prefix(
        &script,
        &dir,
        None,
        None,
        Some(vec!["sh".into(), "-s".into()]),
        None,
        None,
    )
    .expect("prefixed spawn must succeed");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains('1'), "heredoc via prefix: {stdout:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn plugin_env(vars: &[(&str, &str)], path_prepend: &[&str]) -> PluginEnv {
    let mut env = PluginEnv::default();
    env.add_plugin(
        "test-plugin",
        vars.iter().map(|(k, v)| (k.to_string(), v.to_string())),
        path_prepend.iter().map(std::path::PathBuf::from),
    );
    env
}

#[test]
fn plugin_env_skips_relative_path_prepend_entries() {
    let env = plugin_env(&[], &["relative/dir", "also/relative"]);
    assert!(env.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_vars_reach_the_spawned_shell() {
    let env = plugin_env(&[("GRAY_PLUGIN_THING", "from-plugin")], &[]);
    let spawned = spawn(
        "echo thing=$GRAY_PLUGIN_THING",
        &PathBuf::from("/tmp"),
        None,
        None,
        None,
        Some(&env),
    )
    .expect("spawn");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("thing=from-plugin"), "stdout: {stdout:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_vars_are_exported_into_a_prefixed_script() {
    let env = plugin_env(&[("GRAY_PLUGIN_THING", "across")], &[]);
    let spawned = spawn_with_prefix(
        "echo thing=$GRAY_PLUGIN_THING",
        &PathBuf::from("/tmp"),
        None,
        None,
        Some(vec!["sh".into(), "-s".into()]),
        None,
        Some(&env),
    )
    .expect("prefixed spawn");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("thing=across"), "stdout: {stdout:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_path_prepend_leads_the_inherited_path() {
    let dir = std::env::temp_dir().join(format!("gray-plugin-path-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let shim = dir.join("gray-plugin-shim");
    std::fs::write(&shim, "#!/bin/sh\necho shim-won\n").expect("shim");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let env = plugin_env(&[], &[dir.to_str().expect("utf8 path")]);
    let spawned = spawn(
        r#"gray-plugin-shim; echo "first=${PATH%%:*}""#,
        &PathBuf::from("/tmp"),
        None,
        None,
        None,
        Some(&env),
    )
    .expect("spawn");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("shim-won"), "stdout: {stdout:?}");
    assert!(
        stdout.contains(&format!("first={}", dir.display())),
        "plugin dir must lead PATH: {stdout:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[tokio::test]
async fn plugin_env_cannot_override_reserved_vars() {
    let env = plugin_env(
        &[
            ("GRAY_SESSION_ID", "forged"),
            ("TERM", "xterm"),
            ("PATH", "/nowhere"),
        ],
        &[],
    );
    let spawned = spawn(
        "echo s=$GRAY_SESSION_ID t=$TERM; echo $PATH",
        &PathBuf::from("/tmp"),
        Some("real-session"),
        None,
        None,
        Some(&env),
    )
    .expect("spawn");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("s=real-session"), "stdout: {stdout:?}");
    assert!(stdout.contains("t=dumb"), "stdout: {stdout:?}");
    assert!(!stdout.contains("forged"), "stdout: {stdout:?}");
    assert!(!stdout.contains("xterm"), "stdout: {stdout:?}");
    assert!(!stdout.contains("/nowhere"), "stdout: {stdout:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_later_plugin_var_overrides_an_earlier_one() {
    let mut env = PluginEnv::default();
    env.add_plugin("first", [("GRAY_ORDER".to_string(), "one".to_string())], []);
    env.add_plugin(
        "second",
        [("GRAY_ORDER".to_string(), "two".to_string())],
        [],
    );
    let spawned = spawn(
        "echo order=$GRAY_ORDER",
        &PathBuf::from("/tmp"),
        None,
        None,
        None,
        Some(&env),
    )
    .expect("spawn");
    let out = spawned.child.wait_with_output().await.expect("child");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("order=two"), "stdout: {stdout:?}");
}
