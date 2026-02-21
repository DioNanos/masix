//! Masix CLI
//!
//! Command-line interface for Masix messaging agent

mod logging;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use masix_config::Config;
use masix_core::MasixRuntime;
use masix_exec::{manage_termux_boot, BootAction};
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
}

#[derive(Subcommand)]
enum TermuxBootCommands {
    Enable,
    Disable,
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
    Telegram,
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
            let data_dir = get_data_dir(&config);
            std::fs::create_dir_all(&data_dir)?;

            let pid_path = data_dir.join(PID_FILE);

            // Check if already running
            if let Some(running_pid) = check_daemon_running(&pid_path)? {
                return Err(anyhow!("Masix is already running (PID: {})", running_pid));
            }

            if foreground {
                acquire_termux_wake_lock();
                let log_dir = data_dir.join("logs");
                std::fs::create_dir_all(&log_dir)?;
                let _logging_guard = logging::init_logging(&log_dir, &cli.log_level)?;
                let db_path = data_dir.join("masix.db");
                let storage = Storage::new(&db_path)?;
                let runtime = MasixRuntime::new(config, storage)?;
                info!("Starting Masix runtime in foreground...");
                runtime.run().await?;
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
                Ok(pid) => println!("Masix stopped (was PID: {})", pid),
                Err(e) => eprintln!("Error: {}", e),
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

        Commands::Whatsapp { action } => {
            match action {
                WhatsappCommands::Start => {
                    println!("Starting WhatsApp adapter...");
                    let config = load_config(cli.config)?;
                    if let Some(whatsapp_config) = &config.whatsapp {
                        if whatsapp_config.enabled {
                            let adapter = masix_whatsapp::WhatsAppAdapter::from_config(
                                whatsapp_config,
                            );
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
                    println!(
                        "Run `masix whatsapp start` and scan the QR when bridge prints it."
                    );
                    println!("Mode is read-only: outbound send is disabled by design.");
                }
            }
        }

        Commands::Sms { action } => {
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
            ConfigCommands::Telegram => {
                run_telegram_wizard(cli.config.clone())?;
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
            check_for_update(json, force).await?;
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
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    fs::write(&pid_path, format!("{}\n{}", pid, timestamp))?;

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
    release_termux_wake_lock();

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

fn acquire_termux_wake_lock() {
    // Check if running in Termux
    if std::env::var("TERMUX_VERSION").is_ok()
        || std::path::Path::new("/data/data/com.termux").exists()
    {
        let _ = Command::new("termux-wake-lock").output();
    }
}

fn release_termux_wake_lock() {
    if std::env::var("TERMUX_VERSION").is_ok()
        || std::path::Path::new("/data/data/com.termux").exists()
    {
        let _ = Command::new("termux-wake-unlock").output();
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
    println!("  masix config provider   - Configure LLM provider");

    Ok(())
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
            Config::load(&config_path)?
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

    // Telegram
    println!("\n── Telegram Setup ──");
    if prompt_confirm("Configure Telegram bot?", true)? {
        let bot_token = prompt_input("Bot token (from @BotFather)", "")?;
        if !bot_token.is_empty() {
            let account = masix_config::TelegramAccount {
                bot_token,
                allowed_chats: None,
                bot_profile: None,
            };
            config
                .telegram
                .get_or_insert_with(Default::default)
                .accounts
                .push(account);
            println!("✓ Telegram bot configured");
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

    let bot_token = prompt_input("Bot token", "")?;
    if bot_token.is_empty() {
        println!("No token provided, aborting.");
        return Ok(());
    }

    let allowed_chats = prompt_input(
        "Allowed chat IDs (comma-separated, or press Enter for all)",
        "",
    )?;
    let bot_profile = prompt_input("Bot profile name (optional)", "")?;

    // Load or create config
    let config_path = get_config_path(config_path)?;
    let mut config = if config_path.exists() {
        Config::load(&config_path)?
    } else {
        Config::default()
    };

    let account = masix_config::TelegramAccount {
        bot_token,
        allowed_chats: if allowed_chats.is_empty() {
            None
        } else {
            Some(
                allowed_chats
                    .split(',')
                    .map(|s| s.trim().parse::<i64>().unwrap_or(0))
                    .filter(|&id| id != 0)
                    .collect(),
            )
        },
        bot_profile: if bot_profile.is_empty() {
            None
        } else {
            Some(bot_profile)
        },
    };

    config
        .telegram
        .get_or_insert_with(Default::default)
        .accounts
        .push(account);

    let config_toml = toml::to_string_pretty(&config)?;
    fs::write(&config_path, config_toml)?;

    println!("\n✅ Telegram bot configured");
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
        Config::load(&config_path)?
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
        Config::load(&config_path)?
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

fn configure_default_profile_provider_chain(
    config: &mut Config,
    primary_provider: &str,
) -> Result<()> {
    let mut provider_names = config
        .providers
        .providers
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>();
    provider_names.sort();
    provider_names.dedup();

    if !provider_names.iter().any(|name| name == primary_provider) {
        anyhow::bail!("Primary provider '{}' is not configured", primary_provider);
    }

    let available_fallbacks = provider_names
        .iter()
        .filter(|name| name.as_str() != primary_provider)
        .cloned()
        .collect::<Vec<_>>();

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

    println!("\nAvailable fallback providers:");
    if available_fallbacks.is_empty() {
        println!("  (none)");
    } else {
        for name in &available_fallbacks {
            println!("  - {}", name);
        }
    }

    let fallback_input = prompt_input(
        "Fallback providers (comma-separated, empty for none)",
        &existing_fallback,
    )?;
    let fallback = parse_provider_list(&fallback_input);
    let vision_input = prompt_input(
        "Vision provider for media (provider name, empty for none)",
        &existing_vision,
    )?;
    let vision_provider = if vision_input.trim().is_empty() {
        None
    } else {
        Some(vision_input.trim().to_string())
    };

    for name in &fallback {
        if name == primary_provider {
            anyhow::bail!(
                "Fallback chain cannot include the primary provider '{}'",
                primary_provider
            );
        }
        if !provider_names.iter().any(|provider| provider == name) {
            anyhow::bail!("Fallback provider '{}' is not configured", name);
        }
    }
    if let Some(name) = &vision_provider {
        if !provider_names.iter().any(|provider| provider == name) {
            anyhow::bail!("Vision provider '{}' is not configured", name);
        }
    }

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
        Config::load(&config_path)?
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

async fn check_for_update(json: bool, force: bool) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};

    const NPM_REGISTRY_URL: &str = "https://registry.npmjs.org/@mmmbuto/masix/latest";
    const CACHE_FILE: &str = ".masix/.update-check";
    const CACHE_DURATION_SECS: u64 = 24 * 60 * 60; // 24 hours

    let current_version = env!("CARGO_PKG_VERSION");
    let home = dirs::home_dir().ok_or_else(|| anyhow!("Cannot determine home directory"))?;
    let cache_path = home.join(CACHE_FILE);

    // Check cache
    if !force && cache_path.exists() {
        if let Ok(content) = fs::read_to_string(&cache_path) {
            if let Ok(cached) = serde_json::from_str::<serde_json::Value>(&content) {
                if let (Some(ts), Some(latest)) =
                    (cached["timestamp"].as_u64(), cached["latest"].as_str())
                {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    if now - ts < CACHE_DURATION_SECS {
                        let has_update = compare_versions(current_version, latest);
                        if json {
                            println!(
                                "{{\"current\":\"{}\",\"latest\":\"{}\",\"has_update\":{}}}",
                                current_version, latest, has_update
                            );
                        } else if has_update {
                            print_update_message(current_version, latest);
                        } else {
                            println!("✅ masix is up to date (v{})", current_version);
                        }
                        return Ok(());
                    }
                }
            }
        }
    }

    // Fetch from npm registry
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;

    let response = match client.get(NPM_REGISTRY_URL).send().await {
        Ok(r) => r,
        Err(e) => {
            if json {
                println!(
                    "{{\"current\":\"{}\",\"latest\":\"{}\",\"has_update\":false,\"error\":\"{}\"}}",
                    current_version, current_version, e
                );
            } else {
                println!("Unable to check for updates: {}", e);
            }
            return Ok(());
        }
    };

    let body = response.text().await?;
    let pkg: serde_json::Value = serde_json::from_str(&body)?;
    let latest = pkg["version"].as_str().unwrap_or(current_version);

    // Update cache
    if let Some(parent) = cache_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let _ = fs::write(
        &cache_path,
        format!("{{\"timestamp\":{},\"latest\":\"{}\"}}", now, latest),
    );

    let has_update = compare_versions(current_version, latest);

    if json {
        println!(
            "{{\"current\":\"{}\",\"latest\":\"{}\",\"has_update\":{}}}",
            current_version, latest, has_update
        );
    } else if has_update {
        print_update_message(current_version, latest);
    } else {
        println!("✅ masix is up to date (v{})", current_version);
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
    println!();
    println!("┌─────────────────────────────────────────────┐");
    println!("│  📦 Update Available!                        │");
    println!("├─────────────────────────────────────────────┤");
    println!("│  Current: v{:<28} │", current);
    println!("│  Latest:  v{:<28} │", latest);
    println!("├─────────────────────────────────────────────┤");
    println!("│  Run to update:                              │");
    println!("│  npm install -g @mmmbuto/masix@latest       │");
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
}
