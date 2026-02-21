//! Masix CLI
//!
//! Command-line interface for Masix messaging agent

mod logging;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use masix_config::Config;
use masix_core::MasixRuntime;
use masix_exec::{
    is_termux_environment, manage_termux_boot, manage_termux_wake_lock, BootAction, WakeLockAction,
};
use masix_providers::{AnthropicProvider, OpenAICompatibleProvider, Provider};
use masix_storage::Storage;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

const PID_FILE: &str = "masix.pid";
const ZAI_STANDARD_BASE_URL: &str = "https://api.z.ai/api/paas/v4";
const ZAI_CODING_BASE_URL: &str = "https://api.z.ai/api/coding/paas/v4";
const NPM_PACKAGE_NAME: &str = "@mmmbuto/masix";
const UPDATE_CACHE_FILE: &str = ".masix/.update-check";
const UPDATE_CACHE_DURATION_SECS: u64 = 24 * 60 * 60;
type KnownProviderDef = (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);

#[derive(Parser)]
#[command(name = "masix")]
#[command(about = "MIT Messaging Agent with MCP + Cron", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Config file path
    #[arg(short, long)]
    config: Option<String>,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Masix runtime (daemon mode, returns immediately)
    Start {
        /// Run in foreground (for debugging)
        #[arg(short, long)]
        foreground: bool,
    },

    /// Stop the Masix daemon
    Stop,

    /// Show daemon status
    Status,

    /// Restart the Masix daemon
    Restart,

    /// Telegram commands
    Telegram {
        #[command(subcommand)]
        action: TelegramCommands,
    },

    /// WhatsApp commands
    Whatsapp {
        #[command(subcommand)]
        action: WhatsappCommands,
    },

    /// SMS commands (Termux)
    Sms {
        #[command(subcommand)]
        action: SmsCommands,
    },

    /// Cron commands
    Cron {
        #[command(subcommand)]
        action: CronCommands,
    },

    /// Termux specific commands
    Termux {
        #[command(subcommand)]
        action: TermuxCommands,
    },

    /// Configuration commands
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Show statistics
    Stats,

    /// Show version
    Version,

    /// Test connections and credentials
    Test {
        #[command(subcommand)]
        action: TestCommands,
    },

    /// Log management commands
    Logs {
        #[command(subcommand)]
        action: LogCommands,
    },

    /// Check for updates
    CheckUpdate {
        /// Output as JSON
        #[arg(short, long)]
        json: bool,
        /// Force check (ignore cache)
        #[arg(short, long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum TestCommands {
    /// Test Telegram bot token
    Telegram,
    /// Test LLM provider API key
    Provider {
        /// Provider name to test (default: all)
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum LogCommands {
    /// Show log files and sizes
    List,
    /// Clean up old logs
    Clean {
        /// Keep only N days of logs
        #[arg(short, long, default_value = "7")]
        days: u64,
    },
    /// Show last N lines of log
    Tail {
        /// Number of lines to show
        #[arg(short, long, default_value = "50")]
        lines: usize,
    },
}

#[derive(Subcommand)]
enum TelegramCommands {
    /// Start Telegram adapter
    Start,
    /// Test bot connection
    Test,
}

#[derive(Subcommand)]
enum WhatsappCommands {
    /// Start WhatsApp adapter
    Start,
    /// Login to WhatsApp (QR code)
    Login,
}

#[derive(Subcommand)]
enum SmsCommands {
    /// List SMS messages
    List {
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
    /// Send SMS
    Send {
        #[arg(short, long)]
        to: String,
        #[arg(short, long)]
        text: String,
    },
    /// List call logs
    Calls {
        #[arg(short, long, default_value = "20")]
        limit: u32,
    },
}

#[derive(Subcommand)]
enum CronCommands {
    /// Create a new cron job
    Add {
        /// Natural language schedule (e.g., "Manda domani alle 11 sms a Gino \"Ricorda la partita\"")
        schedule: String,
        /// Optional account tag scope (Telegram bot id prefix)
        #[arg(long)]
        account_tag: Option<String>,
        /// Optional default recipient override
        #[arg(long)]
        recipient: Option<String>,
    },
    /// List cron jobs
    List {
        /// Optional account tag scope
        #[arg(long)]
        account_tag: Option<String>,
        /// Optional recipient filter
        #[arg(long)]
        recipient: Option<String>,
    },
    /// Cancel a cron job
    Cancel {
        /// Cron job ID
        id: i64,
        /// Optional account tag scope
        #[arg(long)]
        account_tag: Option<String>,
    },
}

#[derive(Subcommand)]
enum TermuxCommands {
    /// Configure MasiX startup at Android boot (Termux:Boot)
    Boot {
        #[command(subcommand)]
        action: TermuxBootCommands,
    },
    /// Manage MasiX runtime wake lock (keeps CPU active while running)
    Wake {
        #[command(subcommand)]
        action: TermuxWakeCommands,
    },
}

#[derive(Subcommand)]
enum TermuxBootCommands {
    Enable,
    Disable,
    Status,
}

#[derive(Subcommand)]
enum TermuxWakeCommands {
    On,
    Off,
    Status,
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Initialize configuration with interactive wizard
    Init {
        /// Skip wizard, use defaults
        #[arg(short, long)]
        defaults: bool,
    },
    /// Show current configuration
    Show,
    /// Validate configuration
    Validate,
    /// Configure Telegram bot interactively
    Telegram {
        /// Show configured Telegram bots/chats and exit
        #[arg(short, long)]
        list: bool,
    },
    /// Configure SMS watcher interactively
    Sms,
    /// Configure LLM provider interactively
    Provider {
        /// Provider name (openai, openrouter, zai, chutes, llama.cpp, xai, groq, etc.)
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Manage LLM providers
    Providers {
        #[command(subcommand)]
        action: ProviderCommands,
    },
    /// Manage MCP servers
    Mcp {
        #[command(subcommand)]
        action: McpCommands,
    },
}

#[derive(Subcommand)]
enum ProviderCommands {
    /// List all configured providers
    List,
    /// Add a new provider
    Add {
        /// Provider name (e.g., openai, xai, groq)
        name: String,
        /// API key
        #[arg(short, long)]
        key: String,
        /// Base URL (optional, uses default for known providers)
        #[arg(short, long)]
        url: Option<String>,
        /// Model name
        #[arg(short = 'm', long)]
        model: Option<String>,
        /// Set as default provider
        #[arg(short, long)]
        default: bool,
    },
    /// Set default provider
    SetDefault {
        /// Provider name
        name: String,
    },
    /// Change model for a provider
    Model {
        /// Provider name
        name: String,
        /// New model name
        model: String,
    },
    /// Remove a provider
    Remove {
        /// Provider name
        name: String,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// List MCP servers
    List,
    /// Add MCP server
    Add {
        /// Server name
        name: String,
        /// Command to run
        command: String,
        /// Command arguments
        args: Vec<String>,
    },
    /// Remove MCP server
    Remove {
        /// Server name
        name: String,
    },
    /// Enable MCP
    Enable,
    /// Disable MCP
    Disable,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Start { foreground } => {
            let config = load_config(cli.config.clone())?;
            if maybe_auto_update_on_start(&config).await? {
                return Ok(());
            }
            let data_dir = get_data_dir(&config);
            std::fs::create_dir_all(&data_dir)?;

            let pid_path = data_dir.join(PID_FILE);
            let current_pid = std::process::id();

            // Check if already running
            if let Some(running_pid) = check_daemon_running(&pid_path)? {
                if running_pid != current_pid {
                    return Err(anyhow!("Masix is already running (PID: {})", running_pid));
                }
            }
            let existing_foreground = find_other_masix_foreground_pids(current_pid);
            if !existing_foreground.is_empty() {
                anyhow::bail!(
                    "Masix foreground instance already running (PID(s): {})",
                    existing_foreground
                        .iter()
                        .map(|pid| pid.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            if foreground {
                if let Err(e) =
                    manage_termux_wake_lock(WakeLockAction::Enable, Some(&data_dir)).await
                {
                    eprintln!("Warning: failed to acquire Termux wake lock: {}", e);
                }
                let log_dir = data_dir.join("logs");
                std::fs::create_dir_all(&log_dir)?;
                let _logging_guard = logging::init_logging(&log_dir, &cli.log_level)?;
                let db_path = data_dir.join("masix.db");
                let storage = Storage::new(&db_path)?;
                let runtime = MasixRuntime::new(config, storage)?;
                write_pid_file(&pid_path, current_pid)?;
                info!("Starting Masix runtime in foreground...");
                let run_result = runtime.run().await;
                if let Err(e) =
                    manage_termux_wake_lock(WakeLockAction::Disable, Some(&data_dir)).await
                {
                    eprintln!("Warning: failed to release Termux wake lock: {}", e);
                }
                clear_pid_file_if_owned(&pid_path, current_pid);
                run_result?;
            } else {
                // Daemon mode - fork and detach
                start_daemon(&data_dir, cli.config, cli.log_level)?;
            }
        }

        Commands::Stop => {
            let config = load_config(cli.config)?;
            let data_dir = get_data_dir(&config);
            let pid_path = data_dir.join(PID_FILE);

            match stop_daemon(&pid_path) {
                Ok(pid) => {
                    println!("Masix stopped (was PID: {})", pid);
                    if let Err(e) =
                        manage_termux_wake_lock(WakeLockAction::Disable, Some(&data_dir)).await
                    {
                        eprintln!("Warning: failed to release Termux wake lock: {}", e);
                    }
                    let current_pid = std::process::id();
                    let unmanaged = find_other_masix_foreground_pids(current_pid);
                    if !unmanaged.is_empty() {
                        for extra_pid in &unmanaged {
                            terminate_process(*extra_pid);
                        }
                        println!(
                            "Stopped additional foreground instance(s): {}",
                            unmanaged
                                .iter()
                                .map(|extra_pid| extra_pid.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
                Err(e) => {
                    let current_pid = std::process::id();
                    let unmanaged = find_other_masix_foreground_pids(current_pid);
                    if unmanaged.is_empty() {
                        eprintln!("Error: {}", e);
                    } else {
                        for pid in &unmanaged {
                            terminate_process(*pid);
                        }
                        println!(
                            "Stopped unmanaged foreground instance(s): {}",
                            unmanaged
                                .iter()
                                .map(|pid| pid.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            }
        }

        Commands::Status => {
            let config = load_config(cli.config)?;
            let data_dir = get_data_dir(&config);
            let pid_path = data_dir.join(PID_FILE);

            match check_daemon_running(&pid_path)? {
                Some(pid) => {
                    println!("Masix is running (PID: {})", pid);
                    if let Ok(uptime) = get_daemon_uptime(&pid_path) {
                        println!("Uptime: {}s", uptime);
                    }
                    let log_manager = logging::LogManager::new(data_dir.join("logs"));
                    println!("Log: {}", log_manager.get_current_log_path().display());
                }
                None => {
                    println!("Masix is not running");
                    if pid_path.exists() {
                        println!("(stale PID file found, cleaning up)");
                        let _ = fs::remove_file(&pid_path);
                    }
                    let current_pid = std::process::id();
                    let unmanaged = find_other_masix_foreground_pids(current_pid);
                    if !unmanaged.is_empty() {
                        println!(
                            "Foreground instance detected without PID file: {}",
                            unmanaged
                                .iter()
                                .map(|pid| pid.to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            }
        }

        Commands::Restart => {
            let config = load_config(cli.config.clone())?;
            let data_dir = get_data_dir(&config);
            let pid_path = data_dir.join(PID_FILE);

            // Stop if running
            if let Some(running_pid) = check_daemon_running(&pid_path)? {
                println!("Stopping Masix (PID: {})...", running_pid);
                stop_daemon(&pid_path)?;
                if let Err(e) =
                    manage_termux_wake_lock(WakeLockAction::Disable, Some(&data_dir)).await
                {
                    eprintln!("Warning: failed to release Termux wake lock: {}", e);
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }

            // Start again
            println!("Starting Masix...");
            start_daemon(&data_dir, cli.config, cli.log_level)?;
        }

        Commands::Telegram { action } => {
            match action {
                TelegramCommands::Start => {
                    println!("Starting Telegram adapter...");
                    let config = load_config(cli.config)?;
                    if let Some(telegram_config) = &config.telegram {
                        if telegram_config.accounts.is_empty() {
                            eprintln!("No Telegram accounts configured");
                            return Ok(());
                        }

                        let mut tasks = tokio::task::JoinSet::new();

                        for account in telegram_config.accounts.clone() {
                            let data_dir = get_data_dir(&config);
                            let poll_timeout = telegram_config.poll_timeout_secs;
                            let recreate_interval = telegram_config.client_recreate_interval_secs;

                            tasks.spawn(async move {
                                let adapter = masix_telegram::TelegramAdapter::new(
                                    &account,
                                    data_dir,
                                    poll_timeout,
                                    recreate_interval,
                                );

                                adapter.poll().await
                            });
                        }

                        while let Some(result) = tasks.join_next().await {
                            match result {
                                Ok(Err(e)) => eprintln!("Telegram adapter error: {}", e),
                                Err(e) => eprintln!("Telegram adapter task failed: {}", e),
                                Ok(Ok(())) => {}
                            }
                        }
                    }
                }
                TelegramCommands::Test => {
                    println!("Testing Telegram bot connection...");
                    // TODO: Implement test
                }
            }
        }

        Commands::Whatsapp { action } => match action {
            WhatsappCommands::Start => {
                println!("Starting WhatsApp adapter...");
                let config = load_config(cli.config)?;
                if let Some(whatsapp_config) = &config.whatsapp {
                    if whatsapp_config.enabled {
                        let adapter = masix_whatsapp::WhatsAppAdapter::from_config(whatsapp_config);
                        if let Err(e) = adapter.start().await {
                            eprintln!("WhatsApp adapter error: {}", e);
                        }
                    } else {
                        eprintln!("WhatsApp is not enabled in config");
                    }
                }
            }
            WhatsappCommands::Login => {
                println!("WhatsApp login flow is handled by the transport bridge.");
                println!("Run `masix whatsapp start` and scan the QR when bridge prints it.");
                println!("Mode is read-only: outbound send is disabled by design.");
            }
        },

        Commands::Sms { action } => {
            if !is_termux_environment() {
                eprintln!("SMS commands are available only on Android Termux.");
                return Ok(());
            }

            let adapter = masix_sms::SmsAdapter::new(None);

            match action {
                SmsCommands::List { limit } => {
                    println!("Listing {} SMS messages...", limit);
                    match adapter.list_sms(limit).await {
                        Ok(messages) => {
                            for msg in messages {
                                println!(
                                    "From: {} | Date: {} | Read: {}",
                                    msg.address, msg.date, msg.read
                                );
                                println!("  {}", msg.body);
                                println!();
                            }
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                SmsCommands::Send { to, text } => {
                    println!("Sending SMS to {}: {}", to, text);
                    match adapter.send_sms(&to, &text).await {
                        Ok(_) => println!("SMS sent successfully"),
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
                SmsCommands::Calls { limit } => {
                    println!("Listing {} call logs...", limit);
                    match adapter.list_calls(limit).await {
                        Ok(logs) => {
                            for log in logs {
                                println!(
                                    "{} | {} | {} | Duration: {}s",
                                    log.number,
                                    log.name.unwrap_or_else(|| "Unknown".to_string()),
                                    log.call_type,
                                    log.duration
                                );
                            }
                        }
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
            }
        }

        Commands::Cron { action } => {
            let config = load_config(cli.config)?;
            let data_dir = get_data_dir(&config);
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("masix.db");
            let storage = Storage::new(&db_path)?;

            match action {
                CronCommands::Add {
                    schedule,
                    account_tag,
                    recipient,
                } => {
                    println!("Creating cron job: {}", schedule);

                    let parser = masix_cron::CronParser::new();
                    let default_recipient = recipient.unwrap_or_else(|| "default".to_string());
                    match parser.parse(&schedule, "telegram", &default_recipient) {
                        Ok(parsed) => {
                            let resolved_account_tag = account_tag
                                .or_else(|| default_telegram_account_tag(&config))
                                .unwrap_or_else(|| "__default__".to_string());
                            match storage.create_cron_job(
                                "cli",
                                &parsed.schedule,
                                &parsed.channel,
                                &parsed.recipient,
                                Some(&resolved_account_tag),
                                &parsed.message,
                                &parsed.timezone,
                                parsed.recurring,
                            ) {
                                Ok(id) => {
                                    println!("Cron job created with ID: {}", id);
                                    println!("  Account tag: {}", resolved_account_tag);
                                    println!("  Schedule: {}", parsed.schedule);
                                    println!("  Channel: {}", parsed.channel);
                                    println!("  Recipient: {}", parsed.recipient);
                                    println!("  Message: {}", parsed.message);
                                    println!("  Recurring: {}", parsed.recurring);
                                }
                                Err(e) => eprintln!("Failed to save: {}", e),
                            }
                        }
                        Err(e) => eprintln!("Parse error: {}", e),
                    }
                }
                CronCommands::List {
                    account_tag,
                    recipient,
                } => {
                    println!("Listing cron jobs...");
                    let jobs_result = if let Some(tag) = account_tag.as_deref() {
                        if let Some(target) = recipient.as_deref() {
                            storage.list_enabled_cron_jobs_for_account_recipient(tag, target)
                        } else {
                            storage.list_enabled_cron_jobs_for_account(tag)
                        }
                    } else {
                        storage.list_enabled_cron_jobs()
                    };

                    match jobs_result {
                        Ok(jobs) => {
                            if jobs.is_empty() {
                                println!("No active cron jobs found.");
                            } else {
                                for job in jobs {
                                    println!(
                                        "ID: {} | Account: {} | Channel: {} | Recipient: {}",
                                        job.id, job.account_tag, job.channel, job.recipient
                                    );
                                    println!("  Message: {}", job.message);
                                    println!(
                                        "  Schedule: {} | Recurring: {}",
                                        job.schedule, job.recurring
                                    );
                                    println!();
                                }
                            }
                        }
                        Err(e) => eprintln!("Error listing jobs: {}", e),
                    }
                }
                CronCommands::Cancel { id, account_tag } => {
                    println!("Cancelling cron job {}", id);
                    let result = if let Some(tag) = account_tag.as_deref() {
                        match storage.disable_cron_job_for_account(id, tag) {
                            Ok(true) => Ok(()),
                            Ok(false) => {
                                eprintln!("Cron job {} not found for account scope '{}'.", id, tag);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        storage.disable_cron_job(id)
                    };

                    match result {
                        Ok(_) => println!("Cron job {} cancelled.", id),
                        Err(e) => eprintln!("Error: {}", e),
                    }
                }
            }
        }

        Commands::Termux { action } => match action {
            _ if !is_termux_environment() => {
                eprintln!("Termux commands are available only on Android Termux.");
            }
            TermuxCommands::Boot { action } => {
                let boot_action = match action {
                    TermuxBootCommands::Enable => BootAction::Enable,
                    TermuxBootCommands::Disable => BootAction::Disable,
                    TermuxBootCommands::Status => BootAction::Status,
                };
                let masix_bin = std::env::current_exe().unwrap_or_else(|_| "masix".into());
                let config_path_buf = cli.config.clone().map(std::path::PathBuf::from);
                match manage_termux_boot(boot_action, &masix_bin, config_path_buf.as_deref()).await
                {
                    Ok(status) => {
                        println!("Script: {}", status.script_path.display());
                        println!("Enabled: {}", status.enabled);
                        if matches!(boot_action, BootAction::Enable) {
                            println!(
                                "Make sure Termux:Boot app is installed and permission granted."
                            );
                        }
                    }
                    Err(e) => eprintln!("Termux boot error: {}", e),
                }
            }
            TermuxCommands::Wake { action } => {
                let wake_action = match action {
                    TermuxWakeCommands::On => WakeLockAction::Enable,
                    TermuxWakeCommands::Off => WakeLockAction::Disable,
                    TermuxWakeCommands::Status => WakeLockAction::Status,
                };

                let data_dir = load_config(cli.config.clone())
                    .ok()
                    .map(|cfg| get_data_dir(&cfg));
                match manage_termux_wake_lock(wake_action, data_dir.as_deref()).await {
                    Ok(status) => {
                        println!("Wake lock supported: {}", status.supported);
                        println!("Wake lock enabled: {}", status.enabled);
                        println!("State: {}", status.state_path.display());
                        if !status.supported {
                            println!("Termux environment not detected.");
                        }
                    }
                    Err(e) => eprintln!("Termux wake lock error: {}", e),
                }
            }
        },

        Commands::Config { action } => match action {
            ConfigCommands::Init { defaults } => {
                if defaults {
                    create_default_config(cli.config.clone())?;
                } else {
                    run_config_wizard(cli.config.clone())?;
                }
            }
            ConfigCommands::Show => match load_config(cli.config) {
                Ok(config) => {
                    println!("Current configuration:");
                    print_redacted_config(&config)?;
                }
                Err(e) => eprintln!("Error loading config: {}", e),
            },
            ConfigCommands::Validate => match load_config(cli.config) {
                Ok(_) => println!("Configuration is valid."),
                Err(e) => eprintln!("Configuration is invalid: {}", e),
            },
            ConfigCommands::Telegram { list } => {
                if list {
                    run_telegram_list(cli.config.clone())?;
                } else {
                    run_telegram_wizard(cli.config.clone())?;
                }
            }
            ConfigCommands::Sms => {
                run_sms_wizard(cli.config.clone())?;
            }
            ConfigCommands::Provider { name } => {
                run_provider_wizard(cli.config.clone(), name)?;
            }
            ConfigCommands::Providers { action } => {
                handle_provider_command(action, cli.config.clone())?;
            }
            ConfigCommands::Mcp { action } => {
                handle_mcp_command(action, cli.config.clone())?;
            }
        },

        Commands::Stats => {
            println!("Masix Statistics");
            println!("================");
            println!("Version: {}", env!("CARGO_PKG_VERSION"));
            println!(
                "Platform: {}-{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            );

            if let Ok(config) = load_config(cli.config.clone()) {
                println!("\nConfiguration:");
                println!("  Default provider: {}", config.providers.default_provider);
                println!(
                    "  Providers configured: {}",
                    config.providers.providers.len()
                );

                if let Some(bots) = &config.bots {
                    println!("  Bot profiles: {}", bots.profiles.len());
                    println!(
                        "  Strict bot/profile mapping: {}",
                        bots.strict_account_profile_mapping.unwrap_or(false)
                    );
                    for profile in &bots.profiles {
                        println!(
                            "    - {} => primary: {}, fallback: {}",
                            profile.name,
                            profile.provider_primary,
                            if profile.provider_fallback.is_empty() {
                                "(none)".to_string()
                            } else {
                                profile.provider_fallback.join(", ")
                            }
                        );
                    }
                }

                if let Some(telegram) = &config.telegram {
                    println!("  Telegram accounts: {}", telegram.accounts.len());
                    for (idx, account) in telegram.accounts.iter().enumerate() {
                        println!(
                            "    - account #{} bot_profile: {}",
                            idx + 1,
                            account.bot_profile.as_deref().unwrap_or("(default)")
                        );
                    }
                }

                if let Some(mcp) = &config.mcp {
                    if mcp.enabled {
                        println!("  MCP servers: {}", mcp.servers.len());
                    }
                }

                let data_dir = get_data_dir(&config);
                let db_path = data_dir.join("masix.db");

                if db_path.exists() {
                    let metadata = std::fs::metadata(&db_path)?;
                    println!("\nDatabase:");
                    println!("  Path: {}", db_path.display());
                    println!("  Size: {} bytes", metadata.len());

                    if let Ok(storage) = Storage::new(&db_path) {
                        if let Ok(count) = storage.count_enabled_cron_jobs() {
                            println!("  Active cron jobs: {}", count);
                        }
                    }
                }
            }
        }

        Commands::Version => {
            println!("masix {}", env!("CARGO_PKG_VERSION"));
        }

        Commands::Test { action } => match action {
            TestCommands::Telegram => {
                println!("Testing Telegram bot connection...\n");
                let config = load_config(cli.config)?;
                test_telegram_bots(&config).await?;
            }
            TestCommands::Provider { name } => {
                println!("Testing LLM provider connection...\n");
                let config = load_config(cli.config)?;
                test_providers(&config, name.as_deref()).await?;
            }
        },

        Commands::Logs { action } => {
            let config = load_config(cli.config.clone())?;
            let data_dir = get_data_dir(&config);
            let log_dir = data_dir.join("logs");
            match action {
                LogCommands::List => {
                    let manager = logging::LogManager::new(log_dir);
                    let files = manager.get_log_files()?;
                    let total_size = manager.get_log_size()?;
                    println!(
                        "Log files ({} total):\n",
                        logging::LogManager::format_size(total_size)
                    );
                    for file in files {
                        let metadata = fs::metadata(&file)?;
                        let size = logging::LogManager::format_size(metadata.len());
                        let modified: chrono::DateTime<chrono::Local> = metadata.modified()?.into();
                        println!(
                            "  {} ({}, modified {})",
                            file.file_name().unwrap().to_string_lossy(),
                            size,
                            modified.format("%Y-%m-%d %H:%M:%S")
                        );
                    }
                }
                LogCommands::Clean { days: _ } => {
                    let manager = logging::LogManager::new(log_dir);
                    let files_before = manager.get_log_files()?.len();
                    manager.cleanup_old_logs()?;
                    let files_after = manager.get_log_files()?.len();
                    println!(
                        "Cleaned {} old log file(s)",
                        files_before.saturating_sub(files_after)
                    );
                }
                LogCommands::Tail { lines } => {
                    let manager = logging::LogManager::new(log_dir);
                    let current_log = manager.get_current_log_path();
                    if current_log.exists() {
                        let content = fs::read_to_string(&current_log)?;
                        let all_lines: Vec<&str> = content.lines().collect();
                        let start = all_lines.len().saturating_sub(lines);
                        for line in &all_lines[start..] {
                            println!("{}", line);
                        }
                    } else {
                        println!("No log file found at {}", current_log.display());
                    }
                }
            }
        }

        Commands::CheckUpdate { json, force } => {
            let channel = load_config(cli.config.clone())
                .ok()
                .map(|config| config.updates.channel)
                .unwrap_or_else(|| "latest".to_string());
            check_for_update(json, force, &channel).await?;
        }
    }

    Ok(())
}

fn load_config(config_path: Option<String>) -> Result<Config> {
    if let Some(path) = config_path {
        Ok(Config::load(&path)?)
    } else if let Some(default_path) = Config::default_path() {
        Ok(Config::load(&default_path)?)
    } else {
        anyhow::bail!("No config file found")
    }
}

fn default_telegram_account_tag(config: &Config) -> Option<String> {
    config.telegram.as_ref().and_then(|telegram| {
        telegram.accounts.first().map(|account| {
            account
                .bot_token
                .split(':')
                .next()
                .unwrap_or("default")
                .to_string()
        })
    })
}

fn get_data_dir(config: &Config) -> std::path::PathBuf {
    if let Some(data_dir) = &config.core.data_dir {
        if data_dir == "~" || data_dir.starts_with("~/") {
            let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
            if data_dir == "~" {
                home
            } else {
                home.join(data_dir.trim_start_matches("~/"))
            }
        } else {
            std::path::PathBuf::from(data_dir)
        }
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".masix")
    }
}

fn print_redacted_config(config: &Config) -> Result<()> {
    let mut value = serde_json::to_value(config)?;

    if let Some(accounts) = value
        .get_mut("telegram")
        .and_then(|t| t.get_mut("accounts"))
        .and_then(|a| a.as_array_mut())
    {
        for account in accounts {
            if let Some(token) = account.get_mut("bot_token") {
                *token = json!("***REDACTED***");
            }
        }
    }

    if let Some(providers) = value
        .get_mut("providers")
        .and_then(|p| p.get_mut("providers"))
        .and_then(|a| a.as_array_mut())
    {
        for provider in providers {
            if let Some(api_key) = provider.get_mut("api_key") {
                *api_key = json!("***REDACTED***");
            }
        }
    }

    if let Some(secret) = value
        .get_mut("whatsapp")
        .and_then(|w| w.get_mut("ingress_shared_secret"))
    {
        *secret = json!("***REDACTED***");
    }

    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

// ============================================================================
// Test Functions
// ============================================================================

async fn test_telegram_bots(config: &Config) -> Result<()> {
    let Some(telegram) = &config.telegram else {
        println!("No Telegram accounts configured.");
        return Ok(());
    };

    if telegram.accounts.is_empty() {
        println!("No Telegram accounts configured.");
        return Ok(());
    }

    let client = reqwest::Client::new();
    let mut success_count = 0;
    let mut fail_count = 0;

    for (idx, account) in telegram.accounts.iter().enumerate() {
        let bot_id = account.bot_token.split(':').next().unwrap_or("unknown");

        println!("Testing account #{} (bot_id: {})...", idx + 1, bot_id);

        let api_url = format!("https://api.telegram.org/bot{}/getMe", account.bot_token);

        match client.post(&api_url).send().await {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();

                if status.is_success() {
                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                        if parsed["ok"].as_bool().unwrap_or(false) {
                            let username =
                                parsed["result"]["username"].as_str().unwrap_or("unknown");
                            let first_name =
                                parsed["result"]["first_name"].as_str().unwrap_or("unknown");
                            println!("  ✓ SUCCESS: @{} ({})", username, first_name);
                            success_count += 1;
                        } else {
                            let error = parsed["description"].as_str().unwrap_or("Unknown error");
                            println!("  ✗ FAILED: {}", error);
                            fail_count += 1;
                        }
                    } else {
                        println!("  ✗ FAILED: Invalid response");
                        fail_count += 1;
                    }
                } else {
                    println!("  ✗ FAILED: HTTP {}", status);
                    fail_count += 1;
                }
            }
            Err(e) => {
                println!("  ✗ FAILED: Network error: {}", e);
                fail_count += 1;
            }
        }
        println!();
    }

    println!("Summary: {} passed, {} failed", success_count, fail_count);
    Ok(())
}

async fn test_providers(config: &Config, name: Option<&str>) -> Result<()> {
    if config.providers.providers.is_empty() {
        println!("No providers configured.");
        return Ok(());
    }

    let mut success_count = 0;
    let mut fail_count = 0;

    for provider_config in &config.providers.providers {
        if let Some(filter) = name {
            if provider_config.name != filter {
                continue;
            }
        }

        println!("Testing provider '{}'...", provider_config.name);
        println!(
            "  Base URL: {}",
            provider_config.base_url.as_deref().unwrap_or("default")
        );
        println!(
            "  Model: {}",
            provider_config.model.as_deref().unwrap_or("default")
        );

        let key_preview = if provider_config.api_key.len() > 8 {
            format!(
                "{}...{}",
                &provider_config.api_key[..4],
                &provider_config.api_key[provider_config.api_key.len() - 4..]
            )
        } else {
            "***".to_string()
        };
        println!("  API Key: {}", key_preview);

        let provider_type = provider_config.provider_type.as_deref().unwrap_or("openai");
        let provider: Box<dyn Provider> = match provider_type {
            "anthropic" => Box::new(AnthropicProvider::new(
                provider_config.name.clone(),
                provider_config.api_key.clone(),
                provider_config.base_url.clone(),
                provider_config.model.clone(),
            )),
            _ => Box::new(OpenAICompatibleProvider::new(
                provider_config.name.clone(),
                provider_config.api_key.clone(),
                provider_config.base_url.clone(),
                provider_config.model.clone(),
            )),
        };

        match provider.health_check().await {
            Ok(true) => {
                println!("  ✓ SUCCESS: Connection OK");
                success_count += 1;
            }
            Ok(false) => {
                println!("  ✗ FAILED: Health check returned false");
                fail_count += 1;
            }
            Err(e) => {
                println!("  ✗ FAILED: {}", e);
                fail_count += 1;
            }
        }
        println!();
    }

    println!("Summary: {} passed, {} failed", success_count, fail_count);
    Ok(())
}

// ============================================================================
// Daemon Management Functions
// ============================================================================

fn start_daemon(data_dir: &PathBuf, config_path: Option<String>, log_level: String) -> Result<()> {
    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir)?;

    let log_manager = logging::LogManager::new(log_dir.clone());
    log_manager.cleanup_old_logs()?;

    let log_path = log_manager.get_current_log_path();
    let pid_path = data_dir.join(PID_FILE);

    // Get current executable
    let masix_bin = std::env::current_exe().context("Failed to get masix executable path")?;

    // Build daemon command with global flags before subcommand (clap requirement)
    let args = build_daemon_args(config_path.as_deref(), &log_level);
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_file_err = log_file
        .try_clone()
        .context("Failed to duplicate log file handle")?;

    // Spawn daemon process
    let mut child = Command::new(&masix_bin)
        .args(&args)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err))
        .spawn()
        .context("Failed to spawn daemon process")?;

    // Detect immediate startup failures so we do not leave stale PID files.
    std::thread::sleep(std::time::Duration::from_millis(300));
    if let Some(status) = child
        .try_wait()
        .context("Failed to check daemon startup status")?
    {
        anyhow::bail!(
            "Masix daemon exited immediately with status {}. Check log: {}",
            status,
            log_path.display()
        );
    }

    let pid = child.id();

    // Write PID file with timestamp
    write_pid_file(&pid_path, pid)?;

    println!("Masix started (PID: {})", pid);
    println!("Log: {}", log_path.display());

    Ok(())
}

fn build_daemon_args(config_path: Option<&str>, log_level: &str) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(path) = config_path {
        args.push("--config".to_string());
        args.push(path.to_string());
    }
    args.push("--log-level".to_string());
    args.push(log_level.to_string());
    args.push("start".to_string());
    args.push("--foreground".to_string());
    args
}

fn write_pid_file(pid_path: &PathBuf, pid: u32) -> Result<()> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    fs::write(pid_path, format!("{}\n{}", pid, timestamp))?;
    Ok(())
}

fn clear_pid_file_if_owned(pid_path: &PathBuf, pid: u32) {
    let Ok(content) = fs::read_to_string(pid_path) else {
        return;
    };
    let owner_pid = content
        .lines()
        .next()
        .and_then(|value| value.trim().parse::<u32>().ok());
    if owner_pid == Some(pid) {
        let _ = fs::remove_file(pid_path);
    }
}

fn stop_daemon(pid_path: &PathBuf) -> Result<u32> {
    let content = fs::read_to_string(pid_path).context("Failed to read PID file")?;

    let pid: u32 = content
        .lines()
        .next()
        .and_then(|s| s.trim().parse().ok())
        .context("Invalid PID in PID file")?;

    // Send SIGTERM
    #[cfg(unix)]
    {
        use std::process::Command as UnixCommand;
        let _ = UnixCommand::new("kill").arg(pid.to_string()).output();
    }

    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }

    // Wait a bit and check if stopped
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if !is_process_running(pid) {
            break;
        }
    }

    // Force kill if still running
    if is_process_running(pid) {
        #[cfg(unix)]
        {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
    }

    fs::remove_file(pid_path).ok();

    Ok(pid)
}

fn check_daemon_running(pid_path: &PathBuf) -> Result<Option<u32>> {
    if !pid_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(pid_path)?;
    let pid = match content
        .lines()
        .next()
        .and_then(|s| s.trim().parse::<u32>().ok())
    {
        Some(p) => p,
        None => return Ok(None),
    };

    if is_process_running(pid) {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

fn get_daemon_uptime(pid_path: &PathBuf) -> Result<u64> {
    let content = fs::read_to_string(pid_path)?;
    let start_time: u64 = content
        .lines()
        .nth(1)
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    Ok(now.saturating_sub(start_time))
}

fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
}

fn terminate_process(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg(pid.to_string()).output();
        std::thread::sleep(std::time::Duration::from_millis(250));
        if is_process_running(pid) {
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
        }
    }

    #[cfg(not(unix))]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
    }
}

fn find_other_masix_foreground_pids(current_pid: u32) -> Vec<u32> {
    #[cfg(unix)]
    {
        let output = match Command::new("ps").args(["-eo", "pid=,args="]).output() {
            Ok(output) => output,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut found: HashSet<u32> = HashSet::new();
        for line in stdout.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            let mut split = trimmed.split_whitespace();
            let Some(pid_str) = split.next() else {
                continue;
            };
            let Ok(pid) = pid_str.parse::<u32>() else {
                continue;
            };
            if pid == current_pid {
                continue;
            }
            let args = trimmed[pid_str.len()..].trim_start();
            let lower = args.to_ascii_lowercase();
            if lower.contains("masix") && lower.contains("start") && lower.contains("--foreground")
            {
                found.insert(pid);
            }
        }
        let mut pids: Vec<u32> = found.into_iter().collect();
        pids.sort_unstable();
        pids
    }

    #[cfg(not(unix))]
    {
        let _ = current_pid;
        Vec::new()
    }
}

// ============================================================================
// Config Wizard Functions
// ============================================================================

fn create_default_config(config_path: Option<String>) -> Result<()> {
    let config_path = get_config_path(config_path)?;
    let config_content = include_str!("../../../config/config.example.toml");
    std::fs::write(&config_path, config_content)?;

    println!("Configuration created at: {}", config_path.display());
    println!("\nEdit the file to add your tokens and API keys, or run:");
    println!("  masix config telegram   - Configure Telegram bot");
    println!("  masix config telegram --list - List Telegram bots/chats");
    println!("  masix config sms        - Configure SMS watcher");
    println!("  masix config provider   - Configure LLM provider");

    Ok(())
}

fn load_config_for_wizard(config_path: &PathBuf) -> Result<Config> {
    let content = fs::read_to_string(config_path)?;
    let mut config: Config = toml::from_str(&content)?;
    let deduped = dedupe_telegram_accounts_by_tag(&mut config);
    if deduped > 0 {
        println!(
            "Warning: removed {} duplicate Telegram account entries (same bot id).",
            deduped
        );
    }
    let normalized_profiles = normalize_telegram_account_profiles(&mut config);
    if normalized_profiles > 0 {
        println!(
            "Warning: normalized {} Telegram account profile binding(s).",
            normalized_profiles
        );
    }
    Ok(config)
}

fn dedupe_telegram_accounts_by_tag(config: &mut Config) -> usize {
    let Some(telegram) = config.telegram.as_mut() else {
        return 0;
    };

    let before = telegram.accounts.len();
    if before <= 1 {
        return 0;
    }

    let mut normalized: Vec<masix_config::TelegramAccount> = Vec::new();
    for account in telegram.accounts.clone() {
        let tag = telegram_account_tag(&account.bot_token);
        if let Some(index) = normalized
            .iter()
            .position(|item| telegram_account_tag(&item.bot_token) == tag)
        {
            normalized[index] = account;
        } else {
            normalized.push(account);
        }
    }

    telegram.accounts = normalized;
    before.saturating_sub(telegram.accounts.len())
}

fn normalize_telegram_account_profiles(config: &mut Config) -> usize {
    let (strict_mapping, profile_names) = if let Some(bots) = &config.bots {
        (
            bots.strict_account_profile_mapping.unwrap_or(false),
            bots.profiles
                .iter()
                .map(|profile| profile.name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>(),
        )
    } else {
        (false, Vec::new())
    };

    let fallback_profile = if profile_names.iter().any(|name| name == "default") {
        Some("default".to_string())
    } else {
        profile_names.first().cloned()
    };

    let Some(telegram) = config.telegram.as_mut() else {
        return 0;
    };

    let mut changed = 0usize;
    for account in &mut telegram.accounts {
        match account
            .bot_profile
            .as_ref()
            .map(|name| name.trim().to_string())
        {
            Some(current) if current.is_empty() => {
                account.bot_profile = None;
                changed += 1;
            }
            Some(current) if !profile_names.is_empty() && !profile_names.contains(&current) => {
                account.bot_profile = fallback_profile.clone();
                changed += 1;
            }
            Some(_) if profile_names.is_empty() => {
                account.bot_profile = None;
                changed += 1;
            }
            None if strict_mapping && !profile_names.is_empty() => {
                account.bot_profile = fallback_profile.clone();
                changed += 1;
            }
            _ => {}
        }
    }

    changed
}

fn run_config_wizard(config_path: Option<String>) -> Result<()> {
    println!("╔════════════════════════════════════════════╗");
    println!("║       MasiX Configuration Wizard           ║");
    println!("╚════════════════════════════════════════════╝");
    println!();

    let config_path = get_config_path(config_path)?;

    let mut config = if config_path.exists() {
        println!("Found existing config at: {}", config_path.display());
        if prompt_confirm("Update existing configuration?", true)? {
            load_config_for_wizard(&config_path)?
        } else {
            return Ok(());
        }
    } else {
        Config::default()
    };

    // Core settings
    println!("\n── Core Settings ──");
    let data_dir = prompt_input("Data directory", "~/.masix")?;
    config.core.data_dir = Some(data_dir);

    println!("\n── Update Settings ──");
    config.updates.enabled = prompt_confirm(
        "Enable startup update check/auto-update",
        config.updates.enabled,
    )?;
    if config.updates.enabled {
        config.updates.check_on_start = prompt_confirm(
            "Check for updates on every start",
            config.updates.check_on_start,
        )?;
        config.updates.auto_apply = prompt_confirm(
            "Auto-apply update when available",
            config.updates.auto_apply,
        )?;
        config.updates.restart_after_update = prompt_confirm(
            "Restart process after successful update",
            config.updates.restart_after_update,
        )?;
        let channel = prompt_input("Update channel (npm dist-tag)", &config.updates.channel)?;
        config.updates.channel = normalize_update_channel(&channel);
    }

    // Telegram
    println!("\n── Telegram Setup ──");
    if prompt_confirm("Configure Telegram bot?", true)? {
        let bot_token = prompt_input("Bot token (from @BotFather)", "")?;
        if !bot_token.is_empty() {
            let account_tag = telegram_account_tag(&bot_token);
            let existing = config
                .telegram
                .as_ref()
                .and_then(|tg| {
                    tg.accounts
                        .iter()
                        .find(|account| telegram_account_tag(&account.bot_token) == account_tag)
                })
                .cloned();
            let account = if let Some(mut current) = existing {
                current.bot_token = bot_token;
                current
            } else {
                masix_config::TelegramAccount {
                    bot_token,
                    allowed_chats: None,
                    bot_profile: None,
                }
            };
            let (replaced, stored_tag) = upsert_telegram_account(&mut config, account);
            if replaced {
                println!("✓ Telegram bot updated (account tag: {})", stored_tag);
            } else {
                println!("✓ Telegram bot configured (account tag: {})", stored_tag);
            }
        }
    }

    // Provider
    println!("\n── LLM Provider Setup ──");
    let providers = get_known_providers();

    println!("Available providers:");
    for (i, (_, name, _, _, _)) in providers.iter().enumerate() {
        println!("  {:2}. {}", i + 1, name);
    }

    let mut selected_primary_provider: Option<String> = None;
    let choice = prompt_input(
        &format!(
            "Select provider (1-{}) or press Enter to skip",
            providers.len()
        ),
        "",
    )?;
    if let Ok(idx) = choice.parse::<usize>() {
        if idx >= 1 && idx <= providers.len() {
            let (key, name, base_url, default_model, provider_type) = &providers[idx - 1];
            let resolved_base_url = if *key == "zai" {
                let current_is_coding = config
                    .providers
                    .providers
                    .iter()
                    .find(|p| p.name == "zai")
                    .and_then(|p| p.base_url.as_deref())
                    .is_some_and(|url| url.contains("/coding/"));
                if prompt_confirm("Use z.ai coding endpoint?", current_is_coding)? {
                    ZAI_CODING_BASE_URL.to_string()
                } else {
                    ZAI_STANDARD_BASE_URL.to_string()
                }
            } else {
                base_url.to_string()
            };
            let api_key = if *key == "llama.cpp" {
                "not-needed".to_string()
            } else {
                prompt_input(&format!("{} API key", name), "")?
            };
            let model = prompt_input("Model name", *default_model)?;

            let provider = masix_config::ProviderConfig {
                name: key.to_string(),
                api_key,
                base_url: Some(resolved_base_url),
                model: Some(model),
                provider_type: Some(provider_type.to_string()),
            };
            let (replaced, stored_name) = upsert_provider(&mut config, provider);
            config.providers.default_provider = stored_name.clone();
            selected_primary_provider = Some(stored_name);
            if replaced {
                println!("✓ {} provider updated", name);
            } else {
                println!("✓ {} provider configured", name);
            }
        }
    }

    if let Some(primary) = selected_primary_provider.as_deref() {
        if prompt_confirm(
            "Configure fallback provider chain for bot profile 'default'?",
            false,
        )? {
            configure_default_profile_provider_chain(&mut config, primary)?;
        }
    }

    // MCP
    println!("\n── MCP (Model Context Protocol) ──");
    if prompt_confirm("Enable MCP for tool calling?", true)? {
        config.mcp.get_or_insert_with(Default::default).enabled = true;
        println!("✓ MCP enabled (filesystem + memory servers)");
    }

    // WhatsApp (read-only)
    println!("\n── WhatsApp Read-Only Setup ──");
    let existing_whatsapp = config.whatsapp.clone();
    let whatsapp_enabled_default = existing_whatsapp
        .as_ref()
        .map(|w| w.enabled)
        .unwrap_or(false);
    if prompt_confirm(
        "Enable WhatsApp read-only listener?",
        whatsapp_enabled_default,
    )? {
        let existing = existing_whatsapp.unwrap_or(masix_config::WhatsappConfig {
            enabled: false,
            read_only: true,
            transport_path: None,
            ingress_shared_secret: None,
            max_message_chars: None,
            allowed_senders: Vec::new(),
            forward_to_telegram_chat_id: None,
            forward_to_telegram_account_tag: None,
            forward_prefix: None,
            accounts: Vec::new(),
        });

        let transport_default = existing
            .transport_path
            .as_deref()
            .unwrap_or("crates/masix-whatsapp/whatsapp-transport.js");
        let transport_path = prompt_input("WhatsApp transport path", transport_default)?;

        let max_chars_default = existing
            .max_message_chars
            .map(|value| value.to_string())
            .unwrap_or_else(|| "4000".to_string());
        let max_chars_input = prompt_input("Max inbound message chars", &max_chars_default)?;
        let max_message_chars = max_chars_input
            .trim()
            .parse::<usize>()
            .map_err(|_| anyhow!("Invalid max chars '{}'", max_chars_input))?;

        let allowed_default = existing.allowed_senders.join(",");
        let allowed_input = prompt_input(
            "Allowed sender IDs (comma-separated, empty = allow all)",
            &allowed_default,
        )?;
        let allowed_senders = parse_csv_list(&allowed_input);

        let secret_default = existing.ingress_shared_secret.unwrap_or_default();
        let secret_input = prompt_input(
            "Ingress shared secret (empty = no signature check)",
            &secret_default,
        )?;
        let ingress_shared_secret = if secret_input.trim().is_empty() {
            None
        } else {
            Some(secret_input.trim().to_string())
        };

        let forward_default = existing.forward_to_telegram_chat_id.is_some();
        let (forward_to_telegram_chat_id, forward_to_telegram_account_tag, forward_prefix) =
            if prompt_confirm("Forward WhatsApp summaries to Telegram?", forward_default)? {
                let chat_default = existing
                    .forward_to_telegram_chat_id
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                let chat_id_input = prompt_input("Telegram chat id for forwarding", &chat_default)?;
                let chat_id = chat_id_input
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| anyhow!("Invalid Telegram chat id '{}'", chat_id_input))?;

                let account_tag_default =
                    existing.forward_to_telegram_account_tag.unwrap_or_default();
                let account_tag_input = prompt_input(
                    "Telegram account tag for forwarding (empty = first account)",
                    &account_tag_default,
                )?;
                let account_tag = if account_tag_input.trim().is_empty() {
                    None
                } else {
                    Some(account_tag_input.trim().to_string())
                };

                let prefix_default = existing
                    .forward_prefix
                    .unwrap_or_else(|| "WhatsApp Alert".to_string());
                let prefix_input = prompt_input("Forward prefix", &prefix_default)?;
                let prefix = if prefix_input.trim().is_empty() {
                    None
                } else {
                    Some(prefix_input.trim().to_string())
                };
                (Some(chat_id), account_tag, prefix)
            } else {
                (None, None, None)
            };

        config.whatsapp = Some(masix_config::WhatsappConfig {
            enabled: true,
            read_only: true,
            transport_path: if transport_path.trim().is_empty() {
                None
            } else {
                Some(transport_path.trim().to_string())
            },
            ingress_shared_secret,
            max_message_chars: Some(max_message_chars),
            allowed_senders,
            forward_to_telegram_chat_id,
            forward_to_telegram_account_tag,
            forward_prefix,
            accounts: existing.accounts,
        });
        println!("✓ WhatsApp read-only listener configured");
    } else if let Some(existing) = existing_whatsapp {
        config.whatsapp = Some(masix_config::WhatsappConfig {
            enabled: false,
            ..existing
        });
        println!("✓ WhatsApp listener disabled");
    }

    // SMS watcher
    configure_sms_watcher(&mut config)?;

    config.validate()?;

    // Write config
    let config_toml = toml::to_string_pretty(&config)?;
    fs::write(&config_path, config_toml)?;

    println!("\n✅ Configuration saved to: {}", config_path.display());
    println!("\nNext steps:");
    println!("  1. Review config: masix config show");
    println!("  2. Start daemon:  masix start");

    Ok(())
}

fn run_telegram_list(config_path: Option<String>) -> Result<()> {
    let config_path = get_config_path(config_path)?;
    let config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    println!("Config path: {}", config_path.display());
    print_telegram_accounts_and_channels(&config);
    Ok(())
}

fn run_sms_wizard(config_path: Option<String>) -> Result<()> {
    println!("╔════════════════════════════════════════════╗");
    println!("║        SMS Watcher Configuration           ║");
    println!("╚════════════════════════════════════════════╝");
    println!();

    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    configure_sms_watcher(&mut config)?;
    config.validate()?;

    let config_toml = toml::to_string_pretty(&config)?;
    fs::write(&config_path, config_toml)?;

    println!("\n✅ SMS configuration saved");
    println!("Config saved to: {}", config_path.display());
    Ok(())
}

fn configure_sms_watcher(config: &mut Config) -> Result<()> {
    println!("\n── SMS Watcher Setup ──");
    print_sms_prereq_status();

    let existing_sms = config.sms.clone();
    let sms_enabled_default = existing_sms.as_ref().map(|s| s.enabled).unwrap_or(false);
    if prompt_confirm("Enable SMS watcher?", sms_enabled_default)? {
        let existing = existing_sms.unwrap_or(masix_config::SmsConfig {
            enabled: false,
            watch_interval_secs: Some(30),
            forward_to_telegram_chat_id: None,
            forward_to_telegram_account_tag: None,
            forward_prefix: None,
            rules: Vec::new(),
        });

        let interval_default = existing.watch_interval_secs.unwrap_or(30).to_string();
        let interval_input = prompt_input("Watch interval seconds", &interval_default)?;
        let watch_interval_secs = interval_input
            .trim()
            .parse::<u64>()
            .map_err(|_| anyhow!("Invalid watch interval '{}'", interval_input))?;

        let forward_default = existing.forward_to_telegram_chat_id.is_some();
        let (forward_to_telegram_chat_id, forward_to_telegram_account_tag, forward_prefix) =
            if prompt_confirm("Forward SMS summaries to Telegram?", forward_default)? {
                if !has_telegram_accounts(config) {
                    anyhow::bail!(
                        "Cannot enable SMS forwarding: no Telegram bot configured. Run `masix config telegram` first."
                    );
                }

                println!("Available Telegram bots/chats for SMS forwarding:");
                print_telegram_accounts_and_channels(config);

                let chat_default = existing
                    .forward_to_telegram_chat_id
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                let chat_id_input = prompt_input("Telegram chat id for forwarding", &chat_default)?;
                let chat_id = chat_id_input
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| anyhow!("Invalid Telegram chat id '{}'", chat_id_input))?;

                let account_tag_default =
                    existing.forward_to_telegram_account_tag.unwrap_or_default();
                let account_tag_input = prompt_input(
                    "Telegram account tag for forwarding (empty = first account)",
                    &account_tag_default,
                )?;
                let account_tag = if account_tag_input.trim().is_empty() {
                    None
                } else {
                    let tag = account_tag_input.trim().to_string();
                    if !telegram_account_tag_exists(config, &tag) {
                        anyhow::bail!(
                            "Unknown Telegram account tag '{}'. Use `masix config telegram --list`.",
                            tag
                        );
                    }
                    Some(tag)
                };

                let prefix_default = existing
                    .forward_prefix
                    .unwrap_or_else(|| "SMS Alert".to_string());
                let prefix_input = prompt_input("Forward prefix", &prefix_default)?;
                let prefix = if prefix_input.trim().is_empty() {
                    None
                } else {
                    Some(prefix_input.trim().to_string())
                };
                (Some(chat_id), account_tag, prefix)
            } else {
                (None, None, None)
            };

        config.sms = Some(masix_config::SmsConfig {
            enabled: true,
            watch_interval_secs: Some(watch_interval_secs),
            forward_to_telegram_chat_id,
            forward_to_telegram_account_tag,
            forward_prefix,
            rules: existing.rules,
        });
        println!("✓ SMS watcher configured");
    } else if let Some(existing) = existing_sms {
        config.sms = Some(masix_config::SmsConfig {
            enabled: false,
            ..existing
        });
        println!("✓ SMS watcher disabled");
    }

    Ok(())
}

fn print_sms_prereq_status() {
    let required = ["termux-sms-list", "termux-sms-send", "termux-call-log"];
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|command| !command_exists(command))
        .collect();
    if missing.is_empty() {
        println!("SMS prerequisites: OK (Termux:API commands detected).");
    } else {
        println!(
            "SMS prerequisites: missing commands: {}",
            missing.join(", ")
        );
        println!("Install/verify: `pkg install termux-api` and Termux:API app permissions.");
    }
}

fn command_exists(command: &str) -> bool {
    Command::new("which")
        .arg(command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn has_telegram_accounts(config: &Config) -> bool {
    config
        .telegram
        .as_ref()
        .map(|telegram| !telegram.accounts.is_empty())
        .unwrap_or(false)
}

fn telegram_account_tag_exists(config: &Config, account_tag: &str) -> bool {
    config
        .telegram
        .as_ref()
        .map(|telegram| {
            telegram
                .accounts
                .iter()
                .any(|account| telegram_account_tag(&account.bot_token) == account_tag)
        })
        .unwrap_or(false)
}

fn print_telegram_accounts_and_channels(config: &Config) {
    let Some(telegram) = &config.telegram else {
        println!("No Telegram account configured yet.");
        return;
    };
    if telegram.accounts.is_empty() {
        println!("No Telegram account configured yet.");
        return;
    }

    println!("Configured Telegram bots:");
    let mut unique_chats = HashSet::new();
    for (index, account) in telegram.accounts.iter().enumerate() {
        let account_tag = telegram_account_tag(&account.bot_token);
        let profile = account.bot_profile.as_deref().unwrap_or("(none)");
        println!("  {:2}. tag={} profile={}", index + 1, account_tag, profile);

        if let Some(chats) = &account.allowed_chats {
            if chats.is_empty() {
                println!("      channels/chats: (all, no filter)");
            } else {
                let mut sorted = chats.clone();
                sorted.sort_unstable();
                sorted.dedup();
                for chat in &sorted {
                    unique_chats.insert(*chat);
                }
                let list = sorted
                    .iter()
                    .map(|chat| chat.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("      channels/chats: {}", list);
            }
        } else {
            println!("      channels/chats: (all, no filter)");
        }
    }

    if unique_chats.is_empty() {
        println!("Known channels/chats from config: (none, bots accept all chats)");
    } else {
        let mut ordered = unique_chats.into_iter().collect::<Vec<_>>();
        ordered.sort_unstable();
        let list = ordered
            .iter()
            .map(|chat| chat.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        println!("Known channels/chats from config: {}", list);
    }
}

fn run_telegram_wizard(config_path: Option<String>) -> Result<()> {
    println!("╔════════════════════════════════════════════╗");
    println!("║       Telegram Bot Configuration           ║");
    println!("╚════════════════════════════════════════════╝");
    println!();
    println!("To get a bot token:");
    println!("  1. Open Telegram and search for @BotFather");
    println!("  2. Send /newbot and follow the instructions");
    println!("  3. Copy the token you receive");
    println!();

    // Load or create config
    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    let existing_accounts = config
        .telegram
        .as_ref()
        .map(|tg| tg.accounts.clone())
        .unwrap_or_default();

    print_telegram_accounts_and_channels(&config);
    if !existing_accounts.is_empty() {
        println!(
            "Tip: inserisci un token dello stesso bot (stesso id prima di ':') per aggiornare in-place."
        );
    }

    let bot_token = prompt_input("Bot token", "")?;
    if bot_token.is_empty() {
        println!("No token provided, aborting.");
        return Ok(());
    }
    let account_tag = telegram_account_tag(&bot_token);

    let existing = existing_accounts
        .iter()
        .find(|account| telegram_account_tag(&account.bot_token) == account_tag)
        .cloned();
    let allowed_default = existing
        .as_ref()
        .and_then(|account| account.allowed_chats.as_ref())
        .map(|ids| {
            ids.iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default();
    let profile_default = existing
        .as_ref()
        .and_then(|account| account.bot_profile.clone())
        .unwrap_or_default();

    let allowed_chats = prompt_input(
        "Allowed chat IDs (comma-separated, or press Enter for all)",
        &allowed_default,
    )?;
    let bot_profile = prompt_input("Bot profile name (optional)", &profile_default)?;

    let account = masix_config::TelegramAccount {
        bot_token,
        allowed_chats: parse_chat_ids_csv(&allowed_chats),
        bot_profile: if bot_profile.is_empty() {
            None
        } else {
            Some(bot_profile)
        },
    };

    let (replaced, stored_tag) = upsert_telegram_account(&mut config, account);
    config.validate()?;

    let config_toml = toml::to_string_pretty(&config)?;
    fs::write(&config_path, config_toml)?;

    if replaced {
        println!("\n✅ Telegram bot updated (account tag: {})", stored_tag);
    } else {
        println!("\n✅ Telegram bot configured (account tag: {})", stored_tag);
    }
    println!("Config saved to: {}", config_path.display());

    Ok(())
}

fn run_provider_wizard(config_path: Option<String>, name: Option<String>) -> Result<()> {
    println!("╔════════════════════════════════════════════╗");
    println!("║       LLM Provider Configuration           ║");
    println!("╚════════════════════════════════════════════╝");
    println!();

    let providers = get_known_providers();

    let selected = if let Some(n) = name {
        providers
            .iter()
            .find(|(key, _, _, _, _)| *key == n.as_str())
    } else {
        println!("Available providers:");
        for (i, (_, name, _, _, _)) in providers.iter().enumerate() {
            println!("  {:2}. {}", i + 1, name);
        }

        let choice = prompt_input(&format!("Select provider (1-{})", providers.len()), "")?;
        let idx = choice.parse::<usize>().unwrap_or(0);
        if idx >= 1 && idx <= providers.len() {
            Some(&providers[idx - 1])
        } else {
            None
        }
    };

    let Some((key, name, base_url, default_model, provider_type)) = selected else {
        println!("Invalid provider selection.");
        return Ok(());
    };

    println!("\nConfiguring {}...", name);

    let api_key = if *key == "llama.cpp" {
        println!("llama.cpp runs locally, no API key needed.");
        "not-needed".to_string()
    } else {
        prompt_input(&format!("{} API key", name), "")?
    };

    let model = prompt_input("Model name", *default_model)?;
    let set_default = prompt_confirm("Set as default provider?", true)?;

    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    let resolved_base_url = if *key == "zai" {
        let current_is_coding = config
            .providers
            .providers
            .iter()
            .find(|p| p.name == "zai")
            .and_then(|p| p.base_url.as_deref())
            .is_some_and(|url| url.contains("/coding/"));
        if prompt_confirm("Use z.ai coding endpoint?", current_is_coding)? {
            ZAI_CODING_BASE_URL.to_string()
        } else {
            ZAI_STANDARD_BASE_URL.to_string()
        }
    } else {
        base_url.to_string()
    };

    let provider = masix_config::ProviderConfig {
        name: key.to_string(),
        api_key,
        base_url: Some(resolved_base_url),
        model: Some(model),
        provider_type: Some(provider_type.to_string()),
    };

    let (replaced, stored_name) = upsert_provider(&mut config, provider);
    if set_default {
        config.providers.default_provider = stored_name.clone();
        if prompt_confirm(
            "Configure fallback provider chain for bot profile 'default'?",
            false,
        )? {
            configure_default_profile_provider_chain(&mut config, &stored_name)?;
        }
    }

    config.validate()?;
    let config_toml = toml::to_string_pretty(&config)?;
    fs::write(&config_path, config_toml)?;

    if replaced {
        println!("\n✅ {} provider updated", name);
    } else {
        println!("\n✅ {} provider configured", name);
    }
    if set_default {
        println!("Set as default provider");
    }
    println!("Config saved to: {}", config_path.display());

    Ok(())
}

fn get_known_providers() -> Vec<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    vec![
        (
            "openai",
            "OpenAI",
            "https://api.openai.com/v1",
            "gpt-4o-mini",
            "openai",
        ),
        (
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            "openai/gpt-4o-mini",
            "openai",
        ),
        (
            "zai",
            "z.ai (GLM)",
            "https://api.z.ai/api/paas/v4",
            "glm-4.5",
            "openai",
        ),
        (
            "chutes",
            "Chutes.ai",
            "https://llm.chutes.ai/v1",
            "zai-org/GLM-5-TEE",
            "openai",
        ),
        (
            "xai",
            "xAI (Grok)",
            "https://api.x.ai/v1",
            "grok-2-latest",
            "openai",
        ),
        (
            "groq",
            "Groq",
            "https://api.groq.com/openai/v1",
            "llama-3.3-70b-versatile",
            "openai",
        ),
        (
            "anthropic",
            "Anthropic (Claude)",
            "https://api.anthropic.com",
            "claude-3-5-sonnet-latest",
            "anthropic",
        ),
        (
            "gemini",
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            "gemini-2.0-flash",
            "openai",
        ),
        (
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            "openai",
        ),
        (
            "mistral",
            "Mistral AI",
            "https://api.mistral.ai/v1",
            "mistral-large-latest",
            "openai",
        ),
        (
            "together",
            "Together AI",
            "https://api.together.xyz/v1",
            "meta-llama/Llama-3-70b-chat-hf",
            "openai",
        ),
        (
            "fireworks",
            "Fireworks AI",
            "https://api.fireworks.ai/inference/v1",
            "accounts/fireworks/models/llama-v3-70b-instruct",
            "openai",
        ),
        (
            "cohere",
            "Cohere",
            "https://api.cohere.ai/v1",
            "command-r",
            "openai",
        ),
        (
            "llama.cpp",
            "llama.cpp (local)",
            "http://localhost:8080/v1",
            "local-model",
            "openai",
        ),
    ]
}

fn handle_provider_command(action: ProviderCommands, config_path: Option<String>) -> Result<()> {
    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    match action {
        ProviderCommands::List => {
            println!("Configured providers:\n");
            if config.providers.providers.is_empty() {
                println!("  No providers configured.");
                println!("\n  Add one with: masix config providers add <name> --key <api-key>");
            } else {
                for provider in &config.providers.providers {
                    let is_default = provider.name == config.providers.default_provider;
                    let default_marker = if is_default { " (default)" } else { "" };
                    let ptype = provider.provider_type.as_deref().unwrap_or("openai");
                    println!("  {}{}", provider.name, default_marker);
                    if let Some(model) = &provider.model {
                        println!("    Model: {}", model);
                    }
                    if let Some(url) = &provider.base_url {
                        println!("    URL: {}", url);
                    }
                    println!("    Type: {}", ptype);
                    let key_preview = if provider.api_key.len() > 8 {
                        format!("{}...", &provider.api_key[..8])
                    } else {
                        "***".to_string()
                    };
                    println!("    Key: {}", key_preview);
                    println!();
                }
            }
        }
        ProviderCommands::Add {
            name,
            key,
            url,
            model,
            default,
        } => {
            let providers = get_known_providers();
            let known = providers.iter().find(|(k, _, _, _, _)| *k == name);

            let base_url = url.or_else(|| known.map(|(_, _, url, _, _)| url.to_string()));
            let provider_type = known.map(|(_, _, _, _, ptype)| ptype.to_string());

            let provider = masix_config::ProviderConfig {
                name: name.clone(),
                api_key: key,
                base_url,
                model,
                provider_type,
            };

            let (replaced, stored_name) = upsert_provider(&mut config, provider);
            if default || config.providers.default_provider.is_empty() {
                config.providers.default_provider = stored_name.clone();
            }

            config.validate()?;
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;

            if replaced {
                println!("✅ Provider '{}' updated", stored_name);
            } else {
                println!("✅ Provider '{}' added", stored_name);
            }
            if default {
                println!("Set as default provider");
            }
        }
        ProviderCommands::SetDefault { name } => {
            let exists = config.providers.providers.iter().any(|p| p.name == name);
            if !exists {
                anyhow::bail!("Provider '{}' not found", name);
            }
            config.providers.default_provider = name.clone();
            config.validate()?;
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ Default provider set to '{}'", name);
        }
        ProviderCommands::Model { name, model } => {
            let provider = config
                .providers
                .providers
                .iter_mut()
                .find(|p| p.name == name)
                .ok_or_else(|| anyhow!("Provider '{}' not found", name))?;
            provider.model = Some(model.clone());
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ Model for '{}' set to '{}'", name, model);
        }
        ProviderCommands::Remove { name } => {
            let len_before = config.providers.providers.len();
            config.providers.providers.retain(|p| p.name != name);
            if config.providers.providers.len() == len_before {
                anyhow::bail!("Provider '{}' not found", name);
            }
            if config.providers.default_provider == name {
                config.providers.default_provider = config
                    .providers
                    .providers
                    .first()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                if !config.providers.default_provider.is_empty() {
                    println!(
                        "Default provider changed to '{}'",
                        config.providers.default_provider
                    );
                }
            }
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ Provider '{}' removed", name);
        }
    }

    Ok(())
}

fn upsert_provider(config: &mut Config, provider: masix_config::ProviderConfig) -> (bool, String) {
    if let Some(existing) = config
        .providers
        .providers
        .iter_mut()
        .find(|p| p.name == provider.name)
    {
        *existing = provider;
        return (true, existing.name.clone());
    }

    let target_key = provider_target_key(&provider);
    if let Some(target_key) = target_key {
        if let Some(existing) = config
            .providers
            .providers
            .iter_mut()
            .find(|p| provider_target_key(p).as_deref() == Some(target_key.as_str()))
        {
            let existing_name = existing.name.clone();
            existing.api_key = provider.api_key;
            existing.base_url = provider.base_url;
            existing.model = provider.model;
            existing.provider_type = provider.provider_type;
            return (true, existing_name);
        }
    }

    let stored_name = provider.name.clone();
    config.providers.providers.push(provider);
    (false, stored_name)
}

fn provider_target_key(provider: &masix_config::ProviderConfig) -> Option<String> {
    let base_url = provider.base_url.as_deref()?.trim().to_lowercase();
    let model = provider.model.as_deref()?.trim().to_lowercase();
    let provider_type = provider
        .provider_type
        .as_deref()
        .unwrap_or("openai")
        .trim()
        .to_lowercase();

    Some(format!("{}|{}|{}", provider_type, base_url, model))
}

#[derive(Debug, PartialEq, Eq)]
enum ProviderReference {
    Configured(String),
    KnownButMissing { key: String, display_name: String },
    Unknown(String),
}

fn configured_provider_names(config: &Config) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    for provider in &config.providers.providers {
        if seen.insert(provider.name.clone()) {
            names.push(provider.name.clone());
        }
    }
    names
}

fn canonical_provider_token(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn resolve_provider_reference(
    token: &str,
    config: &Config,
    known_providers: &[KnownProviderDef],
) -> ProviderReference {
    let token = token.trim();
    if token.is_empty() {
        return ProviderReference::Unknown(String::new());
    }

    let configured = configured_provider_names(config);
    let normalized = canonical_provider_token(token);

    if let Ok(index) = token.parse::<usize>() {
        if index >= 1 && index <= configured.len() {
            return ProviderReference::Configured(configured[index - 1].clone());
        }
    }

    if let Some(name) = configured
        .iter()
        .find(|name| name.eq_ignore_ascii_case(token))
        .cloned()
    {
        return ProviderReference::Configured(name);
    }

    if let Some(name) = configured
        .iter()
        .find(|name| canonical_provider_token(name) == normalized)
        .cloned()
    {
        return ProviderReference::Configured(name);
    }

    for (key, display_name, _, _, _) in known_providers {
        let key_normalized = canonical_provider_token(key);
        let display_normalized = canonical_provider_token(display_name);
        if token.eq_ignore_ascii_case(key)
            || token.eq_ignore_ascii_case(display_name)
            || normalized == key_normalized
            || normalized == display_normalized
        {
            if let Some(name) = configured
                .iter()
                .find(|name| {
                    name.eq_ignore_ascii_case(key)
                        || canonical_provider_token(name) == key_normalized
                })
                .cloned()
            {
                return ProviderReference::Configured(name);
            }

            return ProviderReference::KnownButMissing {
                key: (*key).to_string(),
                display_name: (*display_name).to_string(),
            };
        }
    }

    ProviderReference::Unknown(token.to_string())
}

fn configure_known_provider_interactive(
    config: &mut Config,
    provider_key: &str,
    known_providers: &[KnownProviderDef],
) -> Result<String> {
    let (key, display_name, base_url, default_model, provider_type) = known_providers
        .iter()
        .find(|(key, _, _, _, _)| *key == provider_key)
        .ok_or_else(|| anyhow!("Unknown provider '{}'", provider_key))?;

    println!("\nConfiguring {}...", display_name);

    let existing_api_key = config
        .providers
        .providers
        .iter()
        .find(|p| p.name == *key)
        .map(|p| p.api_key.clone())
        .unwrap_or_default();
    let model_default = config
        .providers
        .providers
        .iter()
        .find(|p| p.name == *key)
        .and_then(|p| p.model.clone())
        .unwrap_or_else(|| (*default_model).to_string());

    let api_key = if *key == "llama.cpp" {
        println!("llama.cpp runs locally, no API key needed.");
        "not-needed".to_string()
    } else {
        prompt_input(&format!("{} API key", display_name), &existing_api_key)?
    };

    let resolved_base_url = if *key == "zai" {
        let current_is_coding = config
            .providers
            .providers
            .iter()
            .find(|p| p.name == "zai")
            .and_then(|p| p.base_url.as_deref())
            .is_some_and(|url| url.contains("/coding/"));
        if prompt_confirm("Use z.ai coding endpoint?", current_is_coding)? {
            ZAI_CODING_BASE_URL.to_string()
        } else {
            ZAI_STANDARD_BASE_URL.to_string()
        }
    } else {
        (*base_url).to_string()
    };

    let model = prompt_input("Model name", &model_default)?;
    let provider = masix_config::ProviderConfig {
        name: (*key).to_string(),
        api_key,
        base_url: Some(resolved_base_url),
        model: Some(model),
        provider_type: Some((*provider_type).to_string()),
    };
    let (replaced, stored_name) = upsert_provider(config, provider);
    if replaced {
        println!("✓ {} provider updated", display_name);
    } else {
        println!("✓ {} provider configured", display_name);
    }

    Ok(stored_name)
}

fn configure_default_profile_provider_chain(
    config: &mut Config,
    primary_provider: &str,
) -> Result<()> {
    let known_providers = get_known_providers();
    let provider_names = configured_provider_names(config);

    if !provider_names.iter().any(|name| name == primary_provider) {
        anyhow::bail!("Primary provider '{}' is not configured", primary_provider);
    }

    let existing_fallback = config
        .bots
        .as_ref()
        .and_then(|bots| bots.profiles.iter().find(|p| p.name == "default"))
        .map(|profile| profile.provider_fallback.join(","))
        .unwrap_or_default();
    let existing_vision = config
        .bots
        .as_ref()
        .and_then(|bots| bots.profiles.iter().find(|p| p.name == "default"))
        .and_then(|profile| profile.vision_provider.clone())
        .unwrap_or_default();

    println!("\nConfigured providers:");
    for (index, name) in provider_names.iter().enumerate() {
        if name == primary_provider {
            println!("  {:2}. {} (primary)", index + 1, name);
        } else {
            println!("  {:2}. {}", index + 1, name);
        }
    }
    println!(
        "References accepted: configured name, numeric index, or known provider key/name (e.g. zai, z.ai, Google Gemini)."
    );
    println!("Missing known providers can be configured directly from this step.");

    let fallback_input = prompt_input(
        "Fallback providers (comma-separated names/indices, empty for none)",
        &existing_fallback,
    )?;
    let mut fallback = Vec::new();
    let mut fallback_seen = HashSet::new();
    for token in parse_provider_list(&fallback_input) {
        let resolved = resolve_provider_reference(&token, config, &known_providers);
        let resolved_name = match resolved {
            ProviderReference::Configured(name) => name,
            ProviderReference::KnownButMissing { key, display_name } => {
                if !prompt_confirm(
                    &format!(
                        "Fallback provider '{}' is not configured. Configure now?",
                        display_name
                    ),
                    true,
                )? {
                    anyhow::bail!("Fallback provider '{}' is not configured", token);
                }
                configure_known_provider_interactive(config, &key, &known_providers)?
            }
            ProviderReference::Unknown(value) => {
                anyhow::bail!(
                    "Unknown fallback provider reference '{}'. Use configured name/index or known provider key/name.",
                    value
                );
            }
        };

        if resolved_name == primary_provider {
            anyhow::bail!(
                "Fallback chain cannot include the primary provider '{}'",
                primary_provider
            );
        }
        if fallback_seen.insert(resolved_name.clone()) {
            fallback.push(resolved_name);
        }
    }

    let vision_input = prompt_input(
        "Vision provider for media (name/index, empty for none)",
        &existing_vision,
    )?;
    let vision_provider = if vision_input.trim().is_empty() {
        None
    } else {
        match resolve_provider_reference(&vision_input, config, &known_providers) {
            ProviderReference::Configured(name) => Some(name),
            ProviderReference::KnownButMissing { key, display_name } => {
                if !prompt_confirm(
                    &format!(
                        "Vision provider '{}' is not configured. Configure now?",
                        display_name
                    ),
                    true,
                )? {
                    anyhow::bail!("Vision provider '{}' is not configured", vision_input);
                }
                Some(configure_known_provider_interactive(
                    config,
                    &key,
                    &known_providers,
                )?)
            }
            ProviderReference::Unknown(value) => {
                anyhow::bail!(
                    "Unknown vision provider reference '{}'. Use configured name/index or known provider key/name.",
                    value
                );
            }
        }
    };

    let (default_workdir, default_memory_file) = default_profile_paths(config);
    let bots = config.bots.get_or_insert_with(|| masix_config::BotsConfig {
        strict_account_profile_mapping: None,
        profiles: Vec::new(),
    });

    if let Some(profile) = bots.profiles.iter_mut().find(|p| p.name == "default") {
        profile.provider_primary = primary_provider.to_string();
        profile.provider_fallback = fallback.clone();
        profile.vision_provider = vision_provider.clone();
        if profile.workdir.trim().is_empty() {
            profile.workdir = default_workdir.clone();
        }
        if profile.memory_file.trim().is_empty() {
            profile.memory_file = default_memory_file.clone();
        }
    } else {
        bots.profiles.push(masix_config::BotProfileConfig {
            name: "default".to_string(),
            workdir: default_workdir,
            memory_file: default_memory_file,
            provider_primary: primary_provider.to_string(),
            vision_provider: vision_provider.clone(),
            provider_fallback: fallback.clone(),
            retry: None,
        });
    }

    let unbound_accounts = config
        .telegram
        .as_ref()
        .map(|tg| {
            tg.accounts
                .iter()
                .filter(|account| account.bot_profile.is_none())
                .count()
        })
        .unwrap_or(0);

    if unbound_accounts > 0
        && prompt_confirm(
            "Map Telegram accounts without bot_profile to profile 'default'?",
            true,
        )?
    {
        if let Some(telegram) = config.telegram.as_mut() {
            for account in &mut telegram.accounts {
                if account.bot_profile.is_none() {
                    account.bot_profile = Some("default".to_string());
                }
            }
        }
    }

    println!(
        "✓ Bot profile 'default' chain: primary='{}' fallback=[{}] vision={}",
        primary_provider,
        fallback.join(", "),
        vision_provider.as_deref().unwrap_or("(none)")
    );
    Ok(())
}

fn parse_provider_list(input: &str) -> Vec<String> {
    parse_csv_list(input)
}

fn parse_chat_ids_csv(input: &str) -> Option<Vec<i64>> {
    let mut values = Vec::new();
    for token in input.split(',') {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(id) = trimmed.parse::<i64>() {
            if id != 0 {
                values.push(id);
            }
        }
    }

    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn telegram_account_tag(bot_token: &str) -> String {
    let token = bot_token.trim();
    token.split(':').next().unwrap_or(token).trim().to_string()
}

fn upsert_telegram_account(
    config: &mut Config,
    account: masix_config::TelegramAccount,
) -> (bool, String) {
    let account_tag = telegram_account_tag(&account.bot_token);
    let telegram = config.telegram.get_or_insert_with(Default::default);

    if let Some(existing) = telegram
        .accounts
        .iter_mut()
        .find(|item| telegram_account_tag(&item.bot_token) == account_tag)
    {
        *existing = account;
        return (true, account_tag);
    }

    telegram.accounts.push(account);
    (false, account_tag)
}

fn parse_csv_list(input: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for token in input.split(',') {
        let name = token.trim();
        if name.is_empty() {
            continue;
        }
        if seen.insert(name.to_string()) {
            items.push(name.to_string());
        }
    }
    items
}

fn default_profile_paths(config: &Config) -> (String, String) {
    let mut data_dir = config
        .core
        .data_dir
        .clone()
        .unwrap_or_else(|| "~/.masix".to_string());
    while data_dir.ends_with('/') && data_dir.len() > 1 {
        data_dir.pop();
    }
    let memory_file = format!("{}/memory/default/MEMORY.md", data_dir);
    (data_dir, memory_file)
}

fn handle_mcp_command(action: McpCommands, config_path: Option<String>) -> Result<()> {
    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        load_config_for_wizard(&config_path)?
    } else {
        Config::default()
    };

    match action {
        McpCommands::List => {
            let mcp = config.mcp.as_ref();
            if mcp.is_none() || !mcp.unwrap().enabled {
                println!("MCP is disabled.");
                println!("\nEnable with: masix config mcp enable");
                return Ok(());
            }
            let mcp = mcp.unwrap();
            println!("MCP Status: enabled\n");
            if mcp.servers.is_empty() {
                println!("  No MCP servers configured.");
                println!("\n  Add one with: masix config mcp add <name> <command> [args...]");
            } else {
                println!("Configured MCP servers:\n");
                for server in &mcp.servers {
                    println!("  {}", server.name);
                    println!("    Command: {} {:?}", server.command, server.args);
                }
            }
        }
        McpCommands::Add {
            name,
            command,
            args,
        } => {
            let mcp = config.mcp.get_or_insert_with(Default::default);
            mcp.enabled = true;
            mcp.servers.push(masix_config::McpServer {
                name: name.clone(),
                command,
                args,
            });
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ MCP server '{}' added", name);
        }
        McpCommands::Remove { name } => {
            let mcp = config
                .mcp
                .as_mut()
                .ok_or_else(|| anyhow!("MCP not configured"))?;
            let len_before = mcp.servers.len();
            mcp.servers.retain(|s| s.name != name);
            if mcp.servers.len() == len_before {
                anyhow::bail!("MCP server '{}' not found", name);
            }
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ MCP server '{}' removed", name);
        }
        McpCommands::Enable => {
            let mcp = config.mcp.get_or_insert_with(Default::default);
            mcp.enabled = true;
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ MCP enabled");
        }
        McpCommands::Disable => {
            if let Some(mcp) = &mut config.mcp {
                mcp.enabled = false;
            }
            let config_toml = toml::to_string_pretty(&config)?;
            fs::write(&config_path, config_toml)?;
            println!("✅ MCP disabled");
        }
    }

    Ok(())
}

fn get_config_path(config_path: Option<String>) -> Result<PathBuf> {
    if let Some(path) = config_path {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(path)
    } else {
        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from(".config"))
            .join("masix");
        std::fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("config.toml"))
    }
}

fn prompt_input(prompt: &str, default: &str) -> Result<String> {
    use std::io::{self, BufRead, Write};

    if default.is_empty() {
        print!("{}: ", prompt);
    } else {
        print!("{} [{}]: ", prompt, default);
    }
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut input = String::new();
    stdin.lock().read_line(&mut input)?;

    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed)
    }
}

fn prompt_confirm(prompt: &str, default: bool) -> Result<bool> {
    let default_str = if default { "Y/n" } else { "y/N" };
    let input = prompt_input(&format!("{} ({})", prompt, default_str), "")?;

    if input.is_empty() {
        Ok(default)
    } else {
        Ok(input.to_lowercase() == "y" || input.to_lowercase() == "yes")
    }
}

#[derive(Debug, Clone)]
struct UpdateStatus {
    current: String,
    latest: String,
    has_update: bool,
}

fn normalize_update_channel(channel: &str) -> String {
    let trimmed = channel.trim();
    if trimmed.is_empty() {
        "latest".to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_dev_binary_path() -> bool {
    std::env::current_exe().ok().is_some_and(|path| {
        let value = path.to_string_lossy();
        value.contains("/target/debug/") || value.contains("/target/release/")
    })
}

fn read_cached_update_status(
    cache_path: &PathBuf,
    current_version: &str,
    channel: &str,
) -> Option<UpdateStatus> {
    let content = fs::read_to_string(cache_path).ok()?;
    let cached = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    let ts = cached["timestamp"].as_u64()?;
    let latest = cached["latest"].as_str()?;
    let cached_channel = cached["channel"].as_str().unwrap_or("latest");

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(ts) >= UPDATE_CACHE_DURATION_SECS || cached_channel != channel {
        return None;
    }

    Some(UpdateStatus {
        current: current_version.to_string(),
        latest: latest.to_string(),
        has_update: compare_versions(current_version, latest),
    })
}

async fn fetch_update_status(force: bool, channel: &str) -> Result<UpdateStatus> {
    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let channel = normalize_update_channel(channel);
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
    let cache_path = home.join(UPDATE_CACHE_FILE);

    if !force {
        if let Some(cached) = read_cached_update_status(&cache_path, &current_version, &channel) {
            return Ok(cached);
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let url = format!(
        "https://registry.npmjs.org/{}/{}",
        NPM_PACKAGE_NAME, channel
    );
    let response = client.get(url).send().await?;
    if !response.status().is_success() {
        anyhow::bail!("npm registry responded with status {}", response.status());
    }
    let body = response.text().await?;
    let pkg: serde_json::Value = serde_json::from_str(&body)?;
    let latest = pkg["version"]
        .as_str()
        .map(|value| value.to_string())
        .unwrap_or_else(|| current_version.clone());

    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let _ = fs::write(
        &cache_path,
        serde_json::json!({
            "timestamp": now,
            "latest": latest,
            "channel": channel,
        })
        .to_string(),
    );

    Ok(UpdateStatus {
        has_update: compare_versions(&current_version, &latest),
        current: current_version,
        latest,
    })
}

async fn maybe_auto_update_on_start(config: &Config) -> Result<bool> {
    const UPDATE_ATTEMPT_ENV: &str = "MASIX_AUTO_UPDATE_ATTEMPTED";

    let updates = &config.updates;
    if !updates.enabled || !updates.check_on_start {
        return Ok(false);
    }

    if std::env::var_os(UPDATE_ATTEMPT_ENV).is_some() {
        return Ok(false);
    }

    // Startup auto-apply uses npm package update flow and is Termux-oriented.
    if !is_termux_environment() {
        return Ok(false);
    }

    if is_dev_binary_path() {
        return Ok(false);
    }

    let channel = normalize_update_channel(&updates.channel);
    let status = match fetch_update_status(false, &channel).await {
        Ok(value) => value,
        Err(e) => {
            eprintln!("Warning: unable to check for updates at startup: {}", e);
            return Ok(false);
        }
    };

    if !status.has_update {
        return Ok(false);
    }

    print_update_message(&status.current, &status.latest);
    if !updates.auto_apply {
        println!("Auto-update disabled by config ([updates].auto_apply=false).");
        return Ok(false);
    }

    if !command_exists("npm") {
        eprintln!("Warning: npm not found; cannot auto-update.");
        return Ok(false);
    }

    let package_target = format!("{}@{}", NPM_PACKAGE_NAME, channel);
    println!(
        "Attempting automatic update: npm install -g {}",
        package_target
    );

    match Command::new("npm")
        .args(["install", "-g", package_target.as_str()])
        .status()
    {
        Ok(exit_status) if exit_status.success() => {
            println!("Automatic update completed successfully.");
        }
        Ok(exit_status) => {
            eprintln!(
                "Warning: automatic update failed with status {}.",
                exit_status
            );
            return Ok(false);
        }
        Err(e) => {
            eprintln!("Warning: automatic update failed: {}", e);
            return Ok(false);
        }
    }

    if !updates.restart_after_update {
        println!("Restart-after-update disabled by config ([updates].restart_after_update=false).");
        return Ok(false);
    }

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut restart = Command::new(&exe);
    restart
        .args(&args)
        .env(UPDATE_ATTEMPT_ENV, "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    match restart.spawn() {
        Ok(child) => {
            println!("Restarted Masix after update (PID: {}).", child.id());
            Ok(true)
        }
        Err(e) => {
            eprintln!("Warning: update installed but restart failed: {}", e);
            Ok(false)
        }
    }
}

async fn check_for_update(json: bool, force: bool, channel: &str) -> Result<()> {
    let status = match fetch_update_status(force, channel).await {
        Ok(value) => value,
        Err(e) => {
            if json {
                println!(
                    "{{\"current\":\"{}\",\"latest\":\"{}\",\"has_update\":false,\"error\":\"{}\"}}",
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_VERSION"),
                    e
                );
            } else {
                println!("Unable to check for updates: {}", e);
            }
            return Ok(());
        }
    };

    if json {
        println!(
            "{{\"current\":\"{}\",\"latest\":\"{}\",\"has_update\":{}}}",
            status.current, status.latest, status.has_update
        );
    } else if status.has_update {
        print_update_message(&status.current, &status.latest);
    } else {
        println!("✅ masix is up to date (v{})", status.current);
    }

    Ok(())
}

fn compare_versions(current: &str, latest: &str) -> bool {
    let parse = |v: &str| {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u32>().ok())
            .collect::<Vec<_>>()
    };
    let current_parts = parse(current);
    let latest_parts = parse(latest);

    for i in 0..std::cmp::max(current_parts.len(), latest_parts.len()) {
        let c = current_parts.get(i).unwrap_or(&0);
        let l = latest_parts.get(i).unwrap_or(&0);
        if c < l {
            return true;
        }
        if c > l {
            return false;
        }
    }
    false
}

fn print_update_message(current: &str, latest: &str) {
    let update_cmd = if is_termux_environment() {
        "npm install -g @mmmbuto/masix@latest"
    } else {
        "brew upgrade masix"
    };

    println!();
    println!("┌─────────────────────────────────────────────┐");
    println!("│  📦 Update Available!                        │");
    println!("├─────────────────────────────────────────────┤");
    println!("│  Current: v{:<28} │", current);
    println!("│  Latest:  v{:<28} │", latest);
    println!("├─────────────────────────────────────────────┤");
    println!("│  Run to update:                              │");
    println!("│  {:<43} │", update_cmd);
    println!("└─────────────────────────────────────────────┘");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(
        name: &str,
        base_url: &str,
        model: &str,
        provider_type: &str,
        api_key: &str,
    ) -> masix_config::ProviderConfig {
        masix_config::ProviderConfig {
            name: name.to_string(),
            api_key: api_key.to_string(),
            base_url: Some(base_url.to_string()),
            model: Some(model.to_string()),
            provider_type: Some(provider_type.to_string()),
        }
    }

    fn make_telegram_account(
        bot_token: &str,
        allowed_chats: Option<Vec<i64>>,
        bot_profile: Option<&str>,
    ) -> masix_config::TelegramAccount {
        masix_config::TelegramAccount {
            bot_token: bot_token.to_string(),
            allowed_chats,
            bot_profile: bot_profile.map(|value| value.to_string()),
        }
    }

    fn make_bot_profile(name: &str) -> masix_config::BotProfileConfig {
        masix_config::BotProfileConfig {
            name: name.to_string(),
            workdir: "~/.masix".to_string(),
            memory_file: "~/.masix/memory/default/MEMORY.md".to_string(),
            provider_primary: "openai".to_string(),
            vision_provider: None,
            provider_fallback: Vec::new(),
            retry: None,
        }
    }

    #[test]
    fn daemon_args_put_global_options_before_subcommand() {
        let args = build_daemon_args(Some("/tmp/config.toml"), "debug");
        assert_eq!(
            args,
            vec![
                "--config",
                "/tmp/config.toml",
                "--log-level",
                "debug",
                "start",
                "--foreground"
            ]
        );
    }

    #[test]
    fn daemon_args_without_config_are_valid() {
        let args = build_daemon_args(None, "info");
        assert_eq!(args, vec!["--log-level", "info", "start", "--foreground"]);
    }

    #[test]
    fn upsert_provider_updates_same_name_in_place() {
        let mut config = Config::default();
        config.providers.providers.push(make_provider(
            "zai",
            "https://api.z.ai/api/paas/v4",
            "glm-4.5",
            "openai",
            "old",
        ));

        let (replaced, stored_name) = upsert_provider(
            &mut config,
            make_provider(
                "zai",
                "https://api.z.ai/api/coding/paas/v4",
                "glm-4.5",
                "openai",
                "new",
            ),
        );

        assert!(replaced);
        assert_eq!(stored_name, "zai");
        assert_eq!(config.providers.providers.len(), 1);
        let provider = &config.providers.providers[0];
        assert_eq!(provider.name, "zai");
        assert_eq!(
            provider.base_url.as_deref(),
            Some("https://api.z.ai/api/coding/paas/v4")
        );
        assert_eq!(provider.api_key, "new");
    }

    #[test]
    fn upsert_provider_reuses_existing_name_for_same_endpoint_and_model() {
        let mut config = Config::default();
        config.providers.providers.push(make_provider(
            "zai-primary",
            "https://api.z.ai/api/paas/v4",
            "glm-4.5",
            "openai",
            "old",
        ));

        let (replaced, stored_name) = upsert_provider(
            &mut config,
            make_provider(
                "zai-alias",
                "https://api.z.ai/api/paas/v4",
                "glm-4.5",
                "openai",
                "new",
            ),
        );

        assert!(replaced);
        assert_eq!(stored_name, "zai-primary");
        assert_eq!(config.providers.providers.len(), 1);
        assert_eq!(config.providers.providers[0].name, "zai-primary");
        assert_eq!(config.providers.providers[0].api_key, "new");
    }

    #[test]
    fn upsert_provider_adds_new_entry_for_different_target() {
        let mut config = Config::default();
        config.providers.providers.push(make_provider(
            "openai",
            "https://api.openai.com/v1",
            "gpt-4o-mini",
            "openai",
            "k1",
        ));

        let (replaced, stored_name) = upsert_provider(
            &mut config,
            make_provider(
                "anthropic",
                "https://api.anthropic.com",
                "claude-3-5-sonnet-latest",
                "anthropic",
                "k2",
            ),
        );

        assert!(!replaced);
        assert_eq!(stored_name, "anthropic");
        assert_eq!(config.providers.providers.len(), 2);
    }

    #[test]
    fn upsert_provider_keeps_default_provider_valid_when_target_matches() {
        let mut config = Config::default();
        config.providers.default_provider = "zai-primary".to_string();
        config.providers.providers.push(make_provider(
            "zai-primary",
            "https://api.z.ai/api/paas/v4",
            "glm-4.5",
            "openai",
            "k1",
        ));

        let (_replaced, stored_name) = upsert_provider(
            &mut config,
            make_provider(
                "zai-secondary",
                "https://api.z.ai/api/paas/v4",
                "glm-4.5",
                "openai",
                "k2",
            ),
        );
        config.providers.default_provider = stored_name;

        assert_eq!(config.providers.providers.len(), 1);
        assert_eq!(config.providers.default_provider, "zai-primary");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn parse_chat_ids_csv_skips_invalid_and_zero_values() {
        assert_eq!(
            parse_chat_ids_csv("123, abc, 0, -10055, , 42"),
            Some(vec![123, -10055, 42])
        );
        assert_eq!(parse_chat_ids_csv("abc,0, ,"), None);
    }

    #[test]
    fn telegram_upsert_updates_existing_account_by_tag() {
        let mut config = Config::default();
        config
            .telegram
            .get_or_insert_with(Default::default)
            .accounts
            .push(make_telegram_account(
                "12345:old-token",
                Some(vec![1, 2]),
                Some("legacy"),
            ));

        let (replaced, tag) = upsert_telegram_account(
            &mut config,
            make_telegram_account("12345:new-token", Some(vec![99]), Some("default")),
        );

        assert!(replaced);
        assert_eq!(tag, "12345");
        let telegram = config.telegram.as_ref().expect("telegram config");
        assert_eq!(telegram.accounts.len(), 1);
        let account = &telegram.accounts[0];
        assert_eq!(account.bot_token, "12345:new-token");
        assert_eq!(account.allowed_chats, Some(vec![99]));
        assert_eq!(account.bot_profile.as_deref(), Some("default"));
    }

    #[test]
    fn telegram_upsert_adds_new_account_for_different_tag() {
        let mut config = Config::default();
        config
            .telegram
            .get_or_insert_with(Default::default)
            .accounts
            .push(make_telegram_account("12345:token-a", None, Some("a")));

        let (replaced, tag) = upsert_telegram_account(
            &mut config,
            make_telegram_account("67890:token-b", None, Some("b")),
        );

        assert!(!replaced);
        assert_eq!(tag, "67890");
        let telegram = config.telegram.as_ref().expect("telegram config");
        assert_eq!(telegram.accounts.len(), 2);
    }

    #[test]
    fn telegram_account_tag_exists_checks_configured_accounts() {
        let mut config = Config::default();
        assert!(!has_telegram_accounts(&config));
        assert!(!telegram_account_tag_exists(&config, "12345"));

        config
            .telegram
            .get_or_insert_with(Default::default)
            .accounts
            .push(make_telegram_account("12345:token-a", None, Some("a")));

        assert!(has_telegram_accounts(&config));
        assert!(telegram_account_tag_exists(&config, "12345"));
        assert!(!telegram_account_tag_exists(&config, "67890"));
    }

    #[test]
    fn normalize_telegram_account_profiles_maps_unknown_to_default_profile() {
        let mut config = Config::default();
        config.bots = Some(masix_config::BotsConfig {
            strict_account_profile_mapping: Some(true),
            profiles: vec![make_bot_profile("default"), make_bot_profile("ops")],
        });
        config
            .telegram
            .get_or_insert_with(Default::default)
            .accounts
            .push(make_telegram_account("12345:token-a", None, Some("MasiX")));

        let changed = normalize_telegram_account_profiles(&mut config);
        assert_eq!(changed, 1);
        let account = &config.telegram.as_ref().expect("telegram config").accounts[0];
        assert_eq!(account.bot_profile.as_deref(), Some("default"));
    }

    #[test]
    fn normalize_telegram_account_profiles_clears_profile_when_no_bots_defined() {
        let mut config = Config::default();
        config
            .telegram
            .get_or_insert_with(Default::default)
            .accounts
            .push(make_telegram_account("12345:token-a", None, Some("legacy")));

        let changed = normalize_telegram_account_profiles(&mut config);
        assert_eq!(changed, 1);
        let account = &config.telegram.as_ref().expect("telegram config").accounts[0];
        assert!(account.bot_profile.is_none());
    }

    #[test]
    fn resolve_provider_reference_accepts_aliases_for_known_missing_provider() {
        let mut config = Config::default();
        config.providers.providers.push(make_provider(
            "chutes",
            "https://llm.chutes.ai/v1",
            "zai-org/GLM-5-TEE",
            "openai",
            "k1",
        ));
        let known = get_known_providers();

        assert_eq!(
            resolve_provider_reference("z.ai", &config, &known),
            ProviderReference::KnownButMissing {
                key: "zai".to_string(),
                display_name: "z.ai (GLM)".to_string()
            }
        );
        assert_eq!(
            resolve_provider_reference("Google Gemini", &config, &known),
            ProviderReference::KnownButMissing {
                key: "gemini".to_string(),
                display_name: "Google Gemini".to_string()
            }
        );
    }

    #[test]
    fn resolve_provider_reference_supports_index_and_case_insensitive_names() {
        let mut config = Config::default();
        config.providers.providers.push(make_provider(
            "chutes",
            "https://llm.chutes.ai/v1",
            "zai-org/GLM-5-TEE",
            "openai",
            "k1",
        ));
        config.providers.providers.push(make_provider(
            "zai",
            "https://api.z.ai/api/paas/v4",
            "glm-4.7",
            "openai",
            "k2",
        ));
        let known = get_known_providers();

        assert_eq!(
            resolve_provider_reference("1", &config, &known),
            ProviderReference::Configured("chutes".to_string())
        );
        assert_eq!(
            resolve_provider_reference("ZAI", &config, &known),
            ProviderReference::Configured("zai".to_string())
        );
        assert_eq!(
            resolve_provider_reference("z.ai", &config, &known),
            ProviderReference::Configured("zai".to_string())
        );
    }
}
