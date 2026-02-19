//! Masix CLI
//!
//! Command-line interface for Masix messaging agent

use anyhow::Result;
use clap::{Parser, Subcommand};
use masix_config::Config;
use masix_core::MasixRuntime;
use masix_exec::{manage_termux_boot, BootAction};
use masix_storage::Storage;
use serde_json::json;
use tracing::info;
use tracing_subscriber::{self, EnvFilter};

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
    /// Start the Masix runtime
    Start,

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
    /// Initialize configuration
    Init,
    /// Show current configuration
    Show,
    /// Validate configuration
    Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    match cli.command {
        Commands::Start => {
            let config = load_config(cli.config)?;
            let data_dir = get_data_dir(&config);
            std::fs::create_dir_all(&data_dir)?;

            let db_path = data_dir.join("masix.db");
            let storage = Storage::new(&db_path)?;
            let runtime = MasixRuntime::new(config, storage)?;

            info!("Starting Masix runtime...");
            runtime.run().await?;
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
                            let adapter = masix_whatsapp::WhatsAppAdapter::new(
                                whatsapp_config.transport_path.clone(),
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
                    println!("WhatsApp login flow (QR code)...");
                    println!("Starting whatsapp-transport.js...");
                    // QR code will be displayed by transport
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
            ConfigCommands::Init => {
                println!("Creating default configuration...");
                let config_dir = dirs::config_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from(".config"))
                    .join("masix");
                std::fs::create_dir_all(&config_dir)?;

                let config_path = config_dir.join("config.toml");
                let config_content = include_str!("../../../config/config.example.toml");
                std::fs::write(&config_path, config_content)?;

                println!("Configuration created at: {}", config_path.display());
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

    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
