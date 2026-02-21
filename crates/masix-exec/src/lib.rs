//! Masix Exec
//!
//! Guarded command execution for chat/runtime, with Termux-specific helpers.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_MAX_OUTPUT_CHARS: usize = 3500;
const DEFAULT_BOOT_SCRIPT_NAME: &str = "masix";
const DEFAULT_BOOT_START_DELAY_SECS: u64 = 8;
const DEFAULT_WAKELOCK_STATE_NAME: &str = "wakelock.state.json";
const TERMUX_SHELL: &str = "/data/data/com.termux/files/usr/bin/sh";
const TERMUX_PREFIX: &str = "/data/data/com.termux/files/usr";

const DEFAULT_BASE_ALLOWLIST: &[&str] = &[
    "pwd", "ls", "whoami", "date", "uname", "uptime", "df", "du", "free", "head", "tail", "wc",
];

const DEFAULT_TERMUX_ALLOWLIST: &[&str] = &[
    "termux-info",
    "termux-battery-status",
    "termux-location",
    "termux-wifi-connectioninfo",
    "termux-telephony-deviceinfo",
    "termux-clipboard-get",
    "termux-sensor",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    Base,
    Termux,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecPolicy {
    pub enabled: bool,
    pub allow_base: bool,
    pub allow_termux: bool,
    pub timeout_secs: u64,
    pub max_output_chars: usize,
    pub base_allowlist: Vec<String>,
    pub termux_allowlist: Vec<String>,
}

impl Default for ExecPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            allow_base: false,
            allow_termux: false,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_output_chars: DEFAULT_MAX_OUTPUT_CHARS,
            base_allowlist: DEFAULT_BASE_ALLOWLIST
                .iter()
                .map(|v| v.to_string())
                .collect(),
            termux_allowlist: DEFAULT_TERMUX_ALLOWLIST
                .iter()
                .map(|v| v.to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResult {
    pub command: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl ExecResult {
    pub fn format_for_chat(&self) -> String {
        if self.timed_out {
            return format!("Command timed out: `{}`", self.command);
        }

        let mut lines = vec![format!(
            "Command: `{}`\nExit code: `{}`",
            self.command, self.exit_code
        )];

        if !self.stdout.trim().is_empty() {
            lines.push(format!("Stdout:\n```text\n{}\n```", self.stdout));
        }

        if !self.stderr.trim().is_empty() {
            lines.push(format!("Stderr:\n```text\n{}\n```", self.stderr));
        }

        if self.stdout.trim().is_empty() && self.stderr.trim().is_empty() {
            lines.push("No output.".to_string());
        }

        lines.join("\n\n")
    }
}

pub async fn run_command(
    policy: &ExecPolicy,
    mode: ExecMode,
    raw_command: &str,
    workdir: &Path,
) -> Result<ExecResult> {
    if !policy.enabled {
        bail!("Exec module is disabled");
    }

    match mode {
        ExecMode::Base if !policy.allow_base => bail!("Base exec commands are disabled"),
        ExecMode::Termux if !policy.allow_termux => bail!("Termux exec commands are disabled"),
        _ => {}
    }

    if mode == ExecMode::Termux && !is_termux_environment() {
        bail!("Not running in Termux environment");
    }

    let tokens = shlex::split(raw_command).ok_or_else(|| anyhow!("Invalid command syntax"))?;
    if tokens.is_empty() {
        bail!("Missing command");
    }

    let command = tokens[0].clone();
    let args = tokens[1..].to_vec();

    for arg in &args {
        validate_argument(arg)?;
    }

    let allowlist = match mode {
        ExecMode::Base => &policy.base_allowlist,
        ExecMode::Termux => &policy.termux_allowlist,
    };
    if !allowlist.iter().any(|item| item == &command) {
        bail!("Command '{}' is not in allowlist", command);
    }

    let mut process = Command::new(&command);
    process
        .args(args.clone())
        .current_dir(workdir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .kill_on_drop(true);

    let timeout_secs = policy.timeout_secs.max(1);
    let output = match timeout(Duration::from_secs(timeout_secs), process.output()).await {
        Ok(result) => result.map_err(|e| anyhow!("Failed to execute '{}': {}", command, e))?,
        Err(_) => {
            return Ok(ExecResult {
                command: join_for_display(&command, &args),
                exit_code: -1,
                stdout: String::new(),
                stderr: String::new(),
                timed_out: true,
            });
        }
    };

    let max_len = policy.max_output_chars.max(256);
    let stdout = truncate_output(&String::from_utf8_lossy(&output.stdout), max_len);
    let stderr = truncate_output(&String::from_utf8_lossy(&output.stderr), max_len);

    Ok(ExecResult {
        command: join_for_display(&command, &args),
        exit_code: output.status.code().unwrap_or(-1),
        stdout,
        stderr,
        timed_out: false,
    })
}

pub fn is_termux_environment() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX")
            .ok()
            .map(|v| v.contains("com.termux"))
            .unwrap_or(false)
        || Path::new(TERMUX_PREFIX).exists()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootAction {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootStatus {
    pub enabled: bool,
    pub script_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeLockAction {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeLockStatus {
    pub supported: bool,
    pub enabled: bool,
    pub state_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WakeLockState {
    pid: u32,
    acquired_at_unix: u64,
}

pub async fn manage_termux_boot(
    action: BootAction,
    masix_bin: &Path,
    config_path: Option<&Path>,
) -> Result<BootStatus> {
    manage_termux_boot_with_home(action, masix_bin, config_path, dirs::home_dir().as_deref()).await
}

pub async fn manage_termux_boot_with_home(
    action: BootAction,
    masix_bin: &Path,
    config_path: Option<&Path>,
    home_dir: Option<&Path>,
) -> Result<BootStatus> {
    if !is_termux_environment() {
        bail!("Termux boot management is available only in Termux");
    }

    let home = home_dir
        .map(|v| v.to_path_buf())
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow!("Home directory not found"))?;

    let boot_dir = home.join(".termux").join("boot");
    let script_path = boot_dir.join(DEFAULT_BOOT_SCRIPT_NAME);

    match action {
        BootAction::Enable => {
            tokio::fs::create_dir_all(&boot_dir).await?;
            let script = render_boot_script(masix_bin, config_path);
            tokio::fs::write(&script_path, script).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                std::fs::set_permissions(&script_path, perms)?;
            }
        }
        BootAction::Disable => {
            if script_path.exists() {
                tokio::fs::remove_file(&script_path).await?;
            }
        }
        BootAction::Status => {}
    }

    Ok(BootStatus {
        enabled: script_path.exists(),
        script_path,
    })
}

pub async fn manage_termux_wake_lock(
    action: WakeLockAction,
    data_dir: Option<&Path>,
) -> Result<WakeLockStatus> {
    let state_path = wake_lock_state_path(data_dir)?;
    let supported = is_termux_environment();

    match action {
        WakeLockAction::Enable => {
            if supported {
                run_wake_command("termux-wake-lock").await?;
                let state = WakeLockState {
                    pid: std::process::id(),
                    acquired_at_unix: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                };
                write_wake_lock_state(&state_path, &state).await?;
            }
            Ok(WakeLockStatus {
                supported,
                enabled: supported && wake_lock_enabled(&state_path).await,
                state_path,
            })
        }
        WakeLockAction::Disable => {
            if supported {
                run_wake_command("termux-wake-unlock").await?;
            }
            remove_wake_lock_state(&state_path).await;
            Ok(WakeLockStatus {
                supported,
                enabled: false,
                state_path,
            })
        }
        WakeLockAction::Status => {
            let enabled = wake_lock_enabled(&state_path).await;
            Ok(WakeLockStatus {
                supported,
                enabled,
                state_path,
            })
        }
    }
}

fn wake_lock_state_path(data_dir: Option<&Path>) -> Result<PathBuf> {
    let base = match data_dir {
        Some(path) => path.to_path_buf(),
        None => dirs::home_dir()
            .ok_or_else(|| anyhow!("Home directory not found"))?
            .join(".masix"),
    };
    Ok(base.join("runtime").join(DEFAULT_WAKELOCK_STATE_NAME))
}

async fn write_wake_lock_state(path: &Path, state: &WakeLockState) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let body = serde_json::to_string(state)?;
    tokio::fs::write(path, body).await?;
    Ok(())
}

async fn wake_lock_enabled(path: &Path) -> bool {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return false;
    };
    let Ok(_state) = serde_json::from_str::<WakeLockState>(&content) else {
        return false;
    };
    true
}

async fn run_wake_command(binary: &str) -> Result<()> {
    let output = Command::new(binary)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| anyhow!("Failed to run '{}': {}", binary, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            bail!("'{}' failed with status {}", binary, output.status);
        }
        bail!("'{}' failed: {}", binary, stderr);
    }
    Ok(())
}

async fn remove_wake_lock_state(path: &Path) {
    if tokio::fs::try_exists(path).await.unwrap_or(false) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

fn render_boot_script(masix_bin: &Path, config_path: Option<&Path>) -> String {
    let bin = masix_bin.display().to_string();
    let config_arg = match config_path {
        Some(path) => format!(
            " -c '{}'",
            escape_single_quotes(&path.display().to_string())
        ),
        None => String::new(),
    };

    format!(
        "#!{shell}\n\
         # Auto-generated by MasiX\n\
         export PATH=\"$PREFIX/bin:$PATH\"\n\
         mkdir -p \"$HOME/.masix/logs\"\n\
         sleep {delay}\n\
         nohup '{bin}' start{config_arg} >> \"$HOME/.masix/logs/boot.log\" 2>&1 &\n",
        shell = TERMUX_SHELL,
        delay = DEFAULT_BOOT_START_DELAY_SECS,
        bin = escape_single_quotes(&bin),
        config_arg = config_arg
    )
}

fn escape_single_quotes(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}

fn validate_argument(arg: &str) -> Result<()> {
    if arg.contains('\0') {
        bail!("Argument contains null byte");
    }
    if arg.starts_with('/') {
        bail!("Absolute paths are not allowed");
    }
    if arg.contains("..") {
        bail!("Path traversal is not allowed");
    }
    if arg.contains('|')
        || arg.contains(';')
        || arg.contains('&')
        || arg.contains('>')
        || arg.contains('<')
        || arg.contains('`')
    {
        bail!("Unsafe shell characters are not allowed");
    }
    Ok(())
}

fn join_for_display(command: &str, args: &[String]) -> String {
    if args.is_empty() {
        return command.to_string();
    }
    format!("{} {}", command, args.join(" "))
}

fn truncate_output(content: &str, max_chars: usize) -> String {
    let mut chars = content.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}\n...[truncated]", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_safe_disabled() {
        let p = ExecPolicy::default();
        assert!(!p.enabled);
        assert!(!p.allow_base);
        assert!(!p.allow_termux);
    }

    #[test]
    fn validate_argument_rejects_dangerous_tokens() {
        assert!(validate_argument("/etc/passwd").is_err());
        assert!(validate_argument("../secret").is_err());
        assert!(validate_argument("a|b").is_err());
    }

    #[test]
    fn render_boot_script_includes_start_command() {
        let script =
            render_boot_script(Path::new("/data/data/com.termux/files/usr/bin/masix"), None);
        assert!(script.contains("nohup"));
        assert!(script.contains("start"));
    }
}
