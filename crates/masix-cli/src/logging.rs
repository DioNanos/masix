//! Logging module with rotation and cleanup
//!
//! Provides daily log rotation with automatic cleanup of logs older than 7 days

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    fmt::{self, format::FmtSpan},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

const LOG_RETENTION_DAYS: u64 = 7;
const LOG_PREFIX: &str = "masix";

pub struct LogManager {
    log_dir: PathBuf,
}

impl LogManager {
    pub fn new(log_dir: PathBuf) -> Self {
        Self { log_dir }
    }

    pub fn get_current_log_path(&self) -> PathBuf {
        let today = chrono::Local::now().format("%Y-%m-%d");
        self.log_dir.join(format!("{}.{}.log", LOG_PREFIX, today))
    }

    pub fn cleanup_old_logs(&self) -> Result<()> {
        let cutoff = SystemTime::now() - Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60);
        let entries = fs::read_dir(&self.log_dir)?;
        let mut deleted_count = 0;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !filename.starts_with(LOG_PREFIX) || !filename.ends_with(".log") {
                continue;
            }
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?;
            if modified < cutoff {
                if let Err(e) = fs::remove_file(&path) {
                    eprintln!("Failed to delete old log {}: {}", path.display(), e);
                } else {
                    deleted_count += 1;
                }
            }
        }
        if deleted_count > 0 {
            tracing::info!("Cleaned up {} old log file(s)", deleted_count);
        }
        Ok(())
    }

    pub fn get_log_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let entries = fs::read_dir(&self.log_dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                    if filename.starts_with(LOG_PREFIX) && filename.ends_with(".log") {
                        files.push(path);
                    }
                }
            }
        }
        files.sort();
        files.reverse();
        Ok(files)
    }

    pub fn get_log_size(&self) -> Result<u64> {
        let files = self.get_log_files()?;
        let mut total_size = 0u64;
        for file in files {
            if let Ok(metadata) = fs::metadata(&file) {
                total_size += metadata.len();
            }
        }
        Ok(total_size)
    }

    pub fn format_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;
        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

pub struct LoggingGuard {
    _guard: WorkerGuard,
}

pub fn init_logging(log_dir: &Path, log_level: &str) -> Result<LoggingGuard> {
    fs::create_dir_all(log_dir)?;
    let manager = LogManager::new(log_dir.to_path_buf());
    manager.cleanup_old_logs()?;
    let log_path = manager.get_current_log_path();

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(true)
        .with_span_events(FmtSpan::CLOSE)
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"))
        }));

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_target(true)
        .with_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::try_new(log_level).unwrap_or_else(|_| EnvFilter::new("info"))
        }));

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init()?;

    Ok(LoggingGuard { _guard: guard })
}
