//! Masix Exec
//!
//! Guarded command execution for chat/runtime, with Termux-specific helpers.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const DEFAULT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_MAX_OUTPUT_CHARS: usize = 3500;
const DEFAULT_BOOT_SCRIPT_NAME: &str = "masix";
const DEFAULT_BOOT_START_DELAY_SECS: u64 = 8;
const DEFAULT_WAKELOCK_STATE_NAME: &str = "wakelock.state.json";
const DESKTOP_BOOT_SCRIPT_NAME: &str = "masix-autostart.sh";
const LINUX_AUTOSTART_DESKTOP_FILE: &str = "masix.desktop";
const LINUX_SYSTEMD_SERVICE_NAME: &str = "masix.service";
const LINUX_SYSTEMD_SYSTEM_SERVICE_PATH: &str = "/etc/systemd/system/masix.service";
const LINUX_SYSTEMD_USER_SERVICE_REL_PATH: &str = ".config/systemd/user/masix.service";
const MACOS_LAUNCH_AGENT_LABEL: &str = "com.mmmbuto.masix";
const MACOS_LAUNCH_AGENT_FILE: &str = "com.mmmbuto.masix.plist";
const MACOS_LAUNCH_DAEMON_PATH: &str = "/Library/LaunchDaemons/com.mmmbuto.masix.plist";
const TERMUX_SHELL: &str = "/data/data/com.termux/files/usr/bin/sh";
const TERMUX_PREFIX: &str = "/data/data/com.termux/files/usr";
const TERMUX_STABLE_MASIX_BIN: &str = "/data/data/com.termux/files/usr/bin/masix";

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
    pub method: String,
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
    let home = home_dir
        .map(|v| v.to_path_buf())
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow!("Home directory not found"))?;

    if is_termux_environment() {
        return manage_termux_boot_native(action, masix_bin, config_path, &home).await;
    }

    match std::env::consts::OS {
        "linux" => manage_linux_boot_autostart(action, masix_bin, config_path, &home).await,
        "macos" => manage_macos_boot_autostart(action, masix_bin, config_path, &home).await,
        _ => bail!("Boot management is unsupported on this platform"),
    }
}

async fn manage_termux_boot_native(
    action: BootAction,
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
) -> Result<BootStatus> {
    let boot_dir = home.join(".termux").join("boot");
    let script_path = boot_dir.join(DEFAULT_BOOT_SCRIPT_NAME);

    match action {
        BootAction::Enable => {
            tokio::fs::create_dir_all(&boot_dir).await?;
            let stable_bin = resolve_boot_binary_path(masix_bin);
            let script = render_boot_script(&stable_bin, config_path);
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
        method: "termux-boot-script".to_string(),
    })
}

async fn manage_linux_boot_autostart(
    action: BootAction,
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
) -> Result<BootStatus> {
    match action {
        BootAction::Enable => {
            let stable_bin = resolve_boot_binary_path(masix_bin);

            if let Some(status) =
                try_enable_linux_system_service(&stable_bin, config_path, home).await?
            {
                return Ok(status);
            }

            if let Some(status) =
                try_enable_linux_user_service(&stable_bin, config_path, home).await?
            {
                return Ok(status);
            }

            let (desktop_path, launch_script_path) = linux_autostart_paths(home);
            if let Some(parent) = desktop_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            if let Some(parent) = launch_script_path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let launch_script = render_desktop_launch_script(&stable_bin, config_path);
            tokio::fs::write(&launch_script_path, launch_script).await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let perms = std::fs::Permissions::from_mode(0o755);
                std::fs::set_permissions(&launch_script_path, perms)?;
            }
            let desktop_entry = render_linux_desktop_entry(&launch_script_path);
            tokio::fs::write(&desktop_path, desktop_entry).await?;

            Ok(BootStatus {
                enabled: desktop_path.exists(),
                script_path: desktop_path,
                method: "linux-autostart-user".to_string(),
            })
        }
        BootAction::Disable => {
            let system_path = linux_system_service_path();
            let user_service_path = linux_user_service_path(home);
            let (desktop_path, launch_script_path) = linux_autostart_paths(home);

            let _ = run_status_command(
                "systemctl",
                &["disable", "--now", LINUX_SYSTEMD_SERVICE_NAME],
            );
            let _ = run_status_command(
                "systemctl",
                &["--user", "disable", "--now", LINUX_SYSTEMD_SERVICE_NAME],
            );

            if system_path.exists() {
                let _ = tokio::fs::remove_file(&system_path).await;
            }
            if user_service_path.exists() {
                let _ = tokio::fs::remove_file(&user_service_path).await;
            }
            if desktop_path.exists() {
                let _ = tokio::fs::remove_file(&desktop_path).await;
            }
            if launch_script_path.exists() {
                let _ = tokio::fs::remove_file(&launch_script_path).await;
            }

            linux_boot_status(home).await
        }
        BootAction::Status => linux_boot_status(home).await,
    }
}

async fn manage_macos_boot_autostart(
    action: BootAction,
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
) -> Result<BootStatus> {
    match action {
        BootAction::Enable => {
            let stable_bin = resolve_boot_binary_path(masix_bin);
            let system_plist = PathBuf::from(MACOS_LAUNCH_DAEMON_PATH);
            let user_plist = home
                .join("Library")
                .join("LaunchAgents")
                .join(MACOS_LAUNCH_AGENT_FILE);
            let logs_dir = home.join(".masix").join("logs");
            tokio::fs::create_dir_all(&logs_dir).await?;

            // Server-style first: LaunchDaemon (requires root permission).
            if let Some(parent) = system_plist.parent() {
                if tokio::fs::create_dir_all(parent).await.is_ok() {
                    let plist = render_macos_launch_plist(
                        MACOS_LAUNCH_AGENT_LABEL,
                        &stable_bin,
                        config_path,
                        &logs_dir.join("launchd.log"),
                    );
                    if tokio::fs::write(&system_plist, plist).await.is_ok() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let perms = std::fs::Permissions::from_mode(0o644);
                            let _ = std::fs::set_permissions(&system_plist, perms);
                        }
                        let _ = run_status_command(
                            "launchctl",
                            &["bootstrap", "system", MACOS_LAUNCH_DAEMON_PATH],
                        );
                        let _ = run_status_command(
                            "launchctl",
                            &["enable", &format!("system/{}", MACOS_LAUNCH_AGENT_LABEL)],
                        );
                        return Ok(BootStatus {
                            enabled: true,
                            script_path: system_plist,
                            method: "macos-launchd-system".to_string(),
                        });
                    }
                }
            }

            // Fallback: LaunchAgent (login session).
            if let Some(parent) = user_plist.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let plist = render_macos_launch_plist(
                MACOS_LAUNCH_AGENT_LABEL,
                &stable_bin,
                config_path,
                &logs_dir.join("launchd.log"),
            );
            tokio::fs::write(&user_plist, plist).await?;
            let uid = current_uid().unwrap_or_default();
            let _ = run_status_command(
                "launchctl",
                &[
                    "bootstrap",
                    &format!("gui/{}", uid),
                    user_plist.to_string_lossy().as_ref(),
                ],
            );

            Ok(BootStatus {
                enabled: true,
                script_path: user_plist,
                method: "macos-launchagent-user".to_string(),
            })
        }
        BootAction::Disable => {
            let system_plist = PathBuf::from(MACOS_LAUNCH_DAEMON_PATH);
            let user_plist = home
                .join("Library")
                .join("LaunchAgents")
                .join(MACOS_LAUNCH_AGENT_FILE);
            let uid = current_uid().unwrap_or_default();

            let _ = run_status_command(
                "launchctl",
                &[
                    "bootout",
                    "system",
                    &format!("system/{}", MACOS_LAUNCH_AGENT_LABEL),
                ],
            );
            let _ = run_status_command(
                "launchctl",
                &[
                    "bootout",
                    &format!("gui/{}", uid),
                    &format!("gui/{}/{}", uid, MACOS_LAUNCH_AGENT_LABEL),
                ],
            );

            if system_plist.exists() {
                let _ = tokio::fs::remove_file(&system_plist).await;
            }
            if user_plist.exists() {
                let _ = tokio::fs::remove_file(&user_plist).await;
            }
            macos_boot_status(home).await
        }
        BootAction::Status => macos_boot_status(home).await,
    }
}

fn linux_system_service_path() -> PathBuf {
    PathBuf::from(LINUX_SYSTEMD_SYSTEM_SERVICE_PATH)
}

fn linux_user_service_path(home: &Path) -> PathBuf {
    home.join(LINUX_SYSTEMD_USER_SERVICE_REL_PATH)
}

fn linux_autostart_paths(home: &Path) -> (PathBuf, PathBuf) {
    (
        home.join(".config")
            .join("autostart")
            .join(LINUX_AUTOSTART_DESKTOP_FILE),
        home.join(".masix")
            .join("runtime")
            .join(DESKTOP_BOOT_SCRIPT_NAME),
    )
}

async fn linux_boot_status(home: &Path) -> Result<BootStatus> {
    let system_path = linux_system_service_path();
    if system_path.exists() {
        let enabled = run_status_command("systemctl", &["is-enabled", LINUX_SYSTEMD_SERVICE_NAME]);
        return Ok(BootStatus {
            enabled: enabled || system_path.exists(),
            script_path: system_path,
            method: "linux-systemd-system".to_string(),
        });
    }

    let user_path = linux_user_service_path(home);
    if user_path.exists() {
        let enabled = run_status_command(
            "systemctl",
            &["--user", "is-enabled", LINUX_SYSTEMD_SERVICE_NAME],
        );
        return Ok(BootStatus {
            enabled: enabled || user_path.exists(),
            script_path: user_path,
            method: "linux-systemd-user".to_string(),
        });
    }

    let (desktop_path, _) = linux_autostart_paths(home);
    Ok(BootStatus {
        enabled: desktop_path.exists(),
        script_path: desktop_path,
        method: "linux-autostart-user".to_string(),
    })
}

async fn try_enable_linux_system_service(
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
) -> Result<Option<BootStatus>> {
    let service_path = linux_system_service_path();
    if !command_exists("systemctl") || !Path::new("/run/systemd/system").exists() {
        return Ok(None);
    }
    if let Some(parent) = service_path.parent() {
        if tokio::fs::create_dir_all(parent).await.is_err() {
            return Ok(None);
        }
    }

    let service = render_linux_systemd_service(masix_bin, config_path, home, false);
    if tokio::fs::write(&service_path, service).await.is_err() {
        return Ok(None);
    }

    let _ = run_status_command("systemctl", &["daemon-reload"]);
    if run_status_command(
        "systemctl",
        &["enable", "--now", LINUX_SYSTEMD_SERVICE_NAME],
    ) {
        return Ok(Some(BootStatus {
            enabled: true,
            script_path: service_path,
            method: "linux-systemd-system".to_string(),
        }));
    }

    Ok(None)
}

async fn try_enable_linux_user_service(
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
) -> Result<Option<BootStatus>> {
    if !command_exists("systemctl") {
        return Ok(None);
    }
    let service_path = linux_user_service_path(home);
    if let Some(parent) = service_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let service = render_linux_systemd_service(masix_bin, config_path, home, true);
    tokio::fs::write(&service_path, service).await?;

    let _ = run_status_command("systemctl", &["--user", "daemon-reload"]);
    if run_status_command(
        "systemctl",
        &["--user", "enable", "--now", LINUX_SYSTEMD_SERVICE_NAME],
    ) {
        if let Some(user) = current_username() {
            let _ = run_status_command("loginctl", &["enable-linger", &user]);
        }
        return Ok(Some(BootStatus {
            enabled: true,
            script_path: service_path,
            method: "linux-systemd-user".to_string(),
        }));
    }

    Ok(None)
}

async fn macos_boot_status(home: &Path) -> Result<BootStatus> {
    let system_path = PathBuf::from(MACOS_LAUNCH_DAEMON_PATH);
    if system_path.exists() {
        return Ok(BootStatus {
            enabled: true,
            script_path: system_path,
            method: "macos-launchd-system".to_string(),
        });
    }

    let user_path = home
        .join("Library")
        .join("LaunchAgents")
        .join(MACOS_LAUNCH_AGENT_FILE);
    Ok(BootStatus {
        enabled: user_path.exists(),
        script_path: user_path,
        method: "macos-launchagent-user".to_string(),
    })
}

fn command_exists(binary: &str) -> bool {
    if binary.contains('/') {
        return Path::new(binary).exists();
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    std::env::split_paths(&paths).any(|path| path.join(binary).exists())
}

fn run_status_command(binary: &str, args: &[&str]) -> bool {
    if !command_exists(binary) {
        return false;
    }

    StdCommand::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn current_username() -> Option<String> {
    if let Ok(user) = std::env::var("USER") {
        let trimmed = user.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if !command_exists("id") {
        return None;
    }

    let output = StdCommand::new("id")
        .arg("-un")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn current_uid() -> Option<String> {
    if !command_exists("id") {
        return None;
    }

    let output = StdCommand::new("id")
        .arg("-u")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn render_linux_systemd_service(
    masix_bin: &Path,
    config_path: Option<&Path>,
    home: &Path,
    user_scope: bool,
) -> String {
    let mut exec = format!("{} start --foreground", masix_bin.display());
    if let Some(path) = config_path {
        exec.push_str(&format!(" -c {}", path.display()));
    }

    let mut unit = String::new();
    unit.push_str("[Unit]\n");
    unit.push_str("Description=MasiX runtime service\n");
    unit.push_str("After=network-online.target\n");
    unit.push_str("Wants=network-online.target\n\n");
    unit.push_str("[Service]\n");
    unit.push_str("Type=simple\n");
    unit.push_str(&format!("WorkingDirectory={}\n", home.display()));
    unit.push_str(&format!("Environment=HOME={}\n", home.display()));
    unit.push_str(&format!("ExecStart={}\n", exec));
    unit.push_str("Restart=always\n");
    unit.push_str("RestartSec=5\n\n");
    unit.push_str("[Install]\n");
    if user_scope {
        unit.push_str("WantedBy=default.target\n");
    } else {
        unit.push_str("WantedBy=multi-user.target\n");
    }
    unit
}

fn render_macos_launch_plist(
    label: &str,
    masix_bin: &Path,
    config_path: Option<&Path>,
    log_path: &Path,
) -> String {
    let mut args = vec![
        format!(
            "<string>{}</string>",
            xml_escape(&masix_bin.display().to_string())
        ),
        "<string>start</string>".to_string(),
        "<string>--foreground</string>".to_string(),
    ];
    if let Some(path) = config_path {
        args.push("<string>-c</string>".to_string());
        args.push(format!(
            "<string>{}</string>",
            xml_escape(&path.display().to_string())
        ));
    }
    let args_xml = args.join("\n      ");
    let escaped_log = xml_escape(&log_path.display().to_string());

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key>\n\
           <string>{label}</string>\n\
           <key>ProgramArguments</key>\n\
           <array>\n\
             {args}\n\
           </array>\n\
           <key>RunAtLoad</key>\n\
           <true/>\n\
           <key>KeepAlive</key>\n\
           <true/>\n\
           <key>StandardOutPath</key>\n\
           <string>{log}</string>\n\
           <key>StandardErrorPath</key>\n\
           <string>{log}</string>\n\
         </dict>\n\
         </plist>\n",
        label = xml_escape(label),
        args = args_xml,
        log = escaped_log
    )
}

fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

fn render_desktop_launch_script(masix_bin: &Path, config_path: Option<&Path>) -> String {
    let bin = masix_bin.display().to_string();
    let config_arg = match config_path {
        Some(path) => format!(
            " -c '{}'",
            escape_single_quotes(&path.display().to_string())
        ),
        None => String::new(),
    };

    format!(
        "#!/usr/bin/env sh\n\
         # Auto-generated by MasiX\n\
         mkdir -p \"$HOME/.masix/logs\"\n\
         nohup '{bin}' start{config_arg} >> \"$HOME/.masix/logs/boot.log\" 2>&1 &\n",
        bin = escape_single_quotes(&bin),
        config_arg = config_arg
    )
}

fn render_linux_desktop_entry(launch_script_path: &Path) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=MasiX\n\
         Comment=Auto-start MasiX runtime\n\
         Exec={}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n",
        launch_script_path.display()
    )
}

fn resolve_boot_binary_path(masix_bin: &Path) -> PathBuf {
    let raw = masix_bin.to_string_lossy();

    // npm/launcher wrappers in some Termux setups expose transient /.l2s paths
    // that are not valid after reboot.
    if raw.contains("/.l2s/") {
        return PathBuf::from(TERMUX_STABLE_MASIX_BIN);
    }

    let stable_termux_bin = PathBuf::from(TERMUX_STABLE_MASIX_BIN);
    if is_termux_environment() && stable_termux_bin.exists() {
        return stable_termux_bin;
    }

    std::fs::canonicalize(masix_bin).unwrap_or_else(|_| masix_bin.to_path_buf())
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

    #[test]
    fn resolve_boot_binary_path_rewrites_transient_l2s_path() {
        let resolved = resolve_boot_binary_path(Path::new("/.l2s/.l2s.masix-abc123"));
        assert_eq!(resolved, PathBuf::from(TERMUX_STABLE_MASIX_BIN));
    }

    #[test]
    fn render_desktop_launch_script_includes_start_command() {
        let script = render_desktop_launch_script(Path::new("/usr/local/bin/masix"), None);
        assert!(script.contains("nohup"));
        assert!(script.contains("start"));
    }

    #[test]
    fn render_linux_desktop_entry_uses_script_path() {
        let entry = render_linux_desktop_entry(Path::new("/tmp/masix-autostart.sh"));
        assert!(entry.contains("Exec=/tmp/masix-autostart.sh"));
        assert!(entry.contains("[Desktop Entry]"));
    }

    #[test]
    fn render_linux_systemd_service_sets_execstart_and_target() {
        let service = render_linux_systemd_service(
            Path::new("/usr/local/bin/masix"),
            Some(Path::new("/tmp/masix.toml")),
            Path::new("/home/test"),
            false,
        );
        assert!(
            service.contains("ExecStart=/usr/local/bin/masix start --foreground -c /tmp/masix.toml")
        );
        assert!(service.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn render_macos_launch_plist_contains_args_and_keepalive() {
        let plist = render_macos_launch_plist(
            "com.mmmbuto.masix",
            Path::new("/usr/local/bin/masix"),
            Some(Path::new("/tmp/masix.toml")),
            Path::new("/tmp/masix&boot.log"),
        );
        assert!(plist.contains("<string>/usr/local/bin/masix</string>"));
        assert!(plist.contains("<string>--foreground</string>"));
        assert!(plist.contains("<string>-c</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("/tmp/masix&amp;boot.log"));
    }
}
