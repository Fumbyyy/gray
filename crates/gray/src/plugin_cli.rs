//! Generic catalog installation and terminal command forwarding. No model calls.
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use gray_plugin::lock::{LockEntry, LockFile};

// Package code executes with user privileges, just like other installed plugins.
// Pin reviewed source; dependency resolution remains the package's responsibility.
const CATALOG: &[(&str, &str, &str)] = &[(
    "discord",
    "git+https://github.com/vstaln/gray-discord-plugin.git@b149022a1ca7c9e099007ffd8411c14489d8db93",
    "gray_discord",
)];

pub fn home() -> anyhow::Result<PathBuf> {
    Ok(crate::sys_prompt_path()?
        .parent()
        .context("cannot resolve gray home")?
        .to_path_buf())
}

fn registry_path(home: &Path) -> PathBuf {
    home.join("plugins/commands.json")
}

fn load(home: &Path) -> anyhow::Result<LockFile> {
    let registry = LockFile::load(&registry_path(home))?;
    anyhow::ensure!(
        registry.schema == 1,
        "unsupported plugin command registry schema"
    );
    Ok(registry)
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.is_empty()
            && name
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-'),
        "invalid plugin command name"
    );
    Ok(())
}

fn run(program: impl AsRef<OsStr>, args: &[&OsStr]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .env("PIP_NO_INPUT", "1")
        .status()
        .context("could not start plugin installer; check Python and python3-venv")?;
    anyhow::ensure!(
        status.success(),
        "plugin installation step failed ({status}); see output above"
    );
    Ok(())
}

/// Private per-plugin venv, published only after installation and import succeed.
/// TempDir rolls back failed installs; a file lock serializes registry updates.
pub fn install(home: &Path, name: &str) -> anyhow::Result<()> {
    validate_name(name)?;
    let (_, source, module) = CATALOG
        .iter()
        .find(|(n, _, _)| *n == name)
        .with_context(|| format!("Unknown plugin '{name}'. Available: discord"))?;
    let root = home.join("plugins/cli");
    std::fs::create_dir_all(&root)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("plugins/.commands.lock"))?;
    lock.try_lock()
        .context("another plugin installation is running")?;
    let mut registry = load(home)?;
    if registry.plugins.contains_key(name) {
        println!("Plugin '{name}' is already registered. Run: gray {name} --help");
        return Ok(());
    }
    let env = tempfile::Builder::new()
        .prefix(&format!("{name}-"))
        .tempdir_in(root)?;
    let python = match std::env::var_os("GRAY_PLUGIN_PYTHON") {
        Some(python) => python,
        None => ["python3", "/usr/bin/python3"].into_iter().find(|python| {
            Command::new(python).args(["-c", "import sys, venv, ensurepip, ctypes; assert sys.version_info >= (3, 11)"])
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .status().is_ok_and(|status| status.success())
        }).context("Python 3.11+ with venv/pip support is required; install python3-venv or set GRAY_PLUGIN_PYTHON")?.into(),
    };
    println!("Installing '{name}' from {source} (plugin code runs with your user permissions).");
    run(
        python,
        &[OsStr::new("-m"), OsStr::new("venv"), env.path().as_os_str()],
    )?;
    let executable = env.path().join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    run(
        &executable,
        &[
            OsStr::new("-m"),
            OsStr::new("pip"),
            OsStr::new("install"),
            OsStr::new(source),
        ],
    )?;
    run(
        &executable,
        &[OsStr::new("-m"), OsStr::new(module), OsStr::new("--help")],
    )?;
    registry.plugins.insert(
        name.into(),
        LockEntry {
            ecosystem: "gray-cli".into(),
            version: "catalog".into(),
            hash: String::new(),
            source: source.to_string(),
            argv: vec![
                executable.to_string_lossy().into_owned(),
                "-m".into(),
                module.to_string(),
            ],
            adapter_version: "1".into(),
            installed_at: chrono::Utc::now().to_rfc3339(),
            scope: "user".into(),
            enabled: true,
        },
    );
    registry.save(&registry_path(home))?;
    // Keep the original venv path: moving it would invalidate Python entry-point shebangs.
    let _ = env.keep();
    println!("Installed '{name}'. Next: gray {name} setup");
    Ok(())
}

/// Resolve only explicitly registered commands (never arbitrary PATH executables).
/// exec preserves terminal, signals, argument boundaries, and the child's exit code.
pub fn forward(home: &Path, name: &str, rest: &[String]) -> anyhow::Result<()> {
    validate_name(name)?;
    let registry = load(home)?;
    let entry = registry.plugins.get(name).with_context(|| {
        format!("no plugin command '{name}' — install it with: gray install plugin {name}")
    })?;
    anyhow::ensure!(entry.enabled, "plugin command '{name}' is disabled");
    let (program, args) = entry
        .argv
        .split_first()
        .context("plugin command has no entry point")?;
    anyhow::ensure!(
        !program.is_empty(),
        "plugin command has an empty executable"
    );
    let mut command = Command::new(program);
    command
        .args(args)
        .args(rest)
        .env("GRAY_BIN", std::env::current_exe()?);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec()).context("could not start installed plugin command")
    }
    #[cfg(not(unix))]
    {
        let status = command
            .status()
            .context("could not start installed plugin command")?;
        std::process::exit(status.code().unwrap_or(1));
    }
}
