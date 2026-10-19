use super::*;

#[cfg(windows)]
#[test]
fn exe_candidates_expands_pathext_suffixes() {
    let candidates = exe_candidates("claude");
    assert_eq!(candidates[0], "claude");
    // Real PATHEXT on any Windows box carries at least .EXE and .CMD.
    assert!(
        candidates
            .iter()
            .any(|c| c.eq_ignore_ascii_case("claude.exe")),
        "expected a .exe candidate, got {candidates:?}"
    );
    assert!(
        candidates
            .iter()
            .any(|c| c.eq_ignore_ascii_case("claude.cmd")),
        "expected a .cmd candidate, got {candidates:?}"
    );
}

#[cfg(windows)]
#[test]
fn exe_candidates_keeps_explicit_extension() {
    let candidates = exe_candidates("tool.ps1");
    assert_eq!(candidates, vec!["tool.ps1".to_string()]);
}

#[cfg(not(windows))]
#[test]
fn exe_candidates_is_bare_name_on_unix() {
    assert_eq!(exe_candidates("claude"), vec!["claude".to_string()]);
}
