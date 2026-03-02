//! Masix Cron - Cron Jobs in Chat
//!
//! Natural language parsing for scheduling messages

use anyhow::{anyhow, Result};
use chrono::{Datelike, Duration as ChronoDuration, Local};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCron {
    pub schedule: String, // ISO timestamp or cron expression
    pub channel: String,  // telegram, whatsapp, sms
    pub recipient: String,
    pub message: String,
    pub recurring: bool,
    pub timezone: String,
}

pub struct CronParser {
    // Regex patterns
    domani_re: Regex,
    tra_re: Regex,
    ogni_re: Regex,
    alle_re: Regex,
    il_re: Regex,
}

impl CronParser {
    pub fn new() -> Self {
        Self {
            domani_re: Regex::new(r"(?i)domani").unwrap(),
            tra_re: Regex::new(r"(?i)tra\s+(\d+)\s*(ore|minuti|giorni)").unwrap(),
            ogni_re: Regex::new(r"(?i)ogni\s+(\w+)\s+alle\s+(\d+)").unwrap(),
            alle_re: Regex::new(r"(?i)alle\s+(\d+)").unwrap(),
            il_re: Regex::new(r"(?i)il\s+(\d+)\s+(\w+)").unwrap(),
        }
    }

    pub fn parse(
        &self,
        input: &str,
        default_channel: &str,
        default_recipient: &str,
    ) -> Result<ParsedCron> {
        let input_lower = input.to_lowercase();

        // Extract message (text in quotes)
        let message = self
            .extract_message(input)
            .unwrap_or_else(|| input.to_string());

        // Parse schedule
        let (schedule, recurring) = if self.domani_re.is_match(&input_lower) {
            // "domani alle 11"
            let hour = self.extract_hour(input).unwrap_or(12);
            let tomorrow_date = Local::now()
                .date_naive()
                .succ_opt()
                .ok_or_else(|| anyhow!("Unable to compute tomorrow date"))?;
            let tomorrow = Self::local_datetime_at_hour(tomorrow_date, hour)?;
            (tomorrow.to_rfc3339(), false)
        } else if let Some(caps) = self.tra_re.captures(&input_lower) {
            // "tra 2 ore" or "tra 30 minuti"
            let value: i64 = caps.get(1).unwrap().as_str().parse()?;
            let unit = caps.get(2).unwrap().as_str();

            let duration = match unit {
                "ore" => ChronoDuration::hours(value),
                "minuti" => ChronoDuration::minutes(value),
                "giorni" => ChronoDuration::days(value),
                _ => ChronoDuration::hours(value),
            };

            let target = Local::now() + duration;
            (target.to_rfc3339(), false)
        } else if let Some(caps) = self.ogni_re.captures(&input_lower) {
            // "ogni lunedì alle 9"
            let day = caps.get(1).unwrap().as_str();
            let hour: u32 = caps.get(2).unwrap().as_str().parse()?;

            let cron_expr = self.day_to_cron(day, hour);
            (cron_expr, true)
        } else if let Some(caps) = self.il_re.captures(&input_lower) {
            // "il 1 marzo alle 15"
            let day: u32 = caps.get(1).unwrap().as_str().parse()?;
            let month_str = caps.get(2).unwrap().as_str();
            let hour = self.extract_hour(input).unwrap_or(12);

            let month = self
                .month_str_to_num(month_str)
                .ok_or_else(|| anyhow!("Invalid month '{}'", month_str))?;
            let year = Local::now().year();

            let date = chrono::NaiveDate::from_ymd_opt(year, month, day).ok_or_else(|| {
                anyhow!("Invalid date: day={} month={} year={}", day, month, year)
            })?;
            let target = Self::local_datetime_at_hour(date, hour)?;

            (target.to_rfc3339(), false)
        } else {
            // Default: tomorrow at noon
            let tomorrow_date = Local::now()
                .date_naive()
                .succ_opt()
                .ok_or_else(|| anyhow!("Unable to compute tomorrow date"))?;
            let tomorrow = Self::local_datetime_at_hour(tomorrow_date, 12)?;
            (tomorrow.to_rfc3339(), false)
        };

        // Extract channel and recipient if specified
        let (channel, recipient) =
            self.extract_channel_recipient(input, default_channel, default_recipient);

        Ok(ParsedCron {
            schedule,
            channel,
            recipient,
            message,
            recurring,
            timezone: "+01:00".to_string(),
        })
    }

    fn extract_message(&self, input: &str) -> Option<String> {
        let re = Regex::new(r#""([^"]+)""#).unwrap();
        re.captures(input)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
    }

    fn extract_hour(&self, input: &str) -> Option<u32> {
        if let Some(caps) = self.alle_re.captures(input) {
            caps.get(1).and_then(|m| m.as_str().parse().ok())
        } else {
            None
        }
    }

    fn day_to_cron(&self, day: &str, hour: u32) -> String {
        let dow = match day.to_lowercase().as_str() {
            "lunedì" | "lunedi" => "1",
            "martedì" | "martedi" => "2",
            "mercoledì" | "mercoledi" => "3",
            "giovedì" | "giovedi" => "4",
            "venerdì" | "venerdi" => "5",
            "sabato" => "6",
            "domenica" => "0",
            _ => "1",
        };

        format!("0 {} * * {}", hour, dow)
    }

    fn month_str_to_num(&self, month: &str) -> Option<u32> {
        match month.to_lowercase().as_str() {
            "gennaio" => Some(1),
            "febbraio" => Some(2),
            "marzo" => Some(3),
            "aprile" => Some(4),
            "maggio" => Some(5),
            "giugno" => Some(6),
            "luglio" => Some(7),
            "agosto" => Some(8),
            "settembre" => Some(9),
            "ottobre" => Some(10),
            "novembre" => Some(11),
            "dicembre" => Some(12),
            _ => None,
        }
    }

    fn local_datetime_at_hour(
        date: chrono::NaiveDate,
        hour: u32,
    ) -> Result<chrono::DateTime<Local>> {
        let naive = date
            .and_hms_opt(hour, 0, 0)
            .ok_or_else(|| anyhow!("Invalid hour '{}'", hour))?;

        naive
            .and_local_timezone(Local)
            .single()
            .ok_or_else(|| anyhow!("Failed to convert local datetime with timezone"))
    }

    fn extract_channel_recipient(
        &self,
        input: &str,
        default_channel: &str,
        default_recipient: &str,
    ) -> (String, String) {
        // Check for explicit channel/recipient
        let re = Regex::new(r"(?i)(sms|telegram|whatsapp)\s+(?:a|al|allo|alla|ai|alle)\s+(\S+)")
            .unwrap();

        if let Some(caps) = re.captures(input) {
            let channel = caps.get(1).unwrap().as_str().to_lowercase();
            let recipient = caps.get(2).unwrap().as_str().to_string();
            (channel, recipient)
        } else {
            (default_channel.to_string(), default_recipient.to_string())
        }
    }
}

impl Default for CronParser {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CronExecutor {
    check_interval_secs: u64,
}

impl CronExecutor {
    pub fn new() -> Self {
        Self {
            check_interval_secs: 30,
        }
    }

    pub async fn run(&self) -> Result<()> {
        anyhow::bail!(
            "CronExecutor::run requires storage and outbound sender; use run_with_storage()"
        );
    }

    pub async fn run_with_storage(
        &self,
        storage: std::sync::Arc<masix_storage::Storage>,
        outbound_sender: tokio::sync::mpsc::Sender<masix_ipc::OutboundMessage>,
    ) -> Result<()> {
        info!("Cron executor started with storage");

        loop {
            let now = Local::now().to_rfc3339();

            match storage.get_due_cron_jobs(&now) {
                Ok(jobs) => {
                    for job in jobs {
                        info!("Executing cron job {}: {}", job.id, job.message);

                        let channel = job.channel.clone();
                        let recipient = job.recipient.clone();
                        let message = job.message.clone();

                        if let Ok(chat_id) = recipient.parse::<i64>() {
                            let account_tag = if job.account_tag == "__default__" {
                                None
                            } else {
                                Some(job.account_tag.clone())
                            };
                            let msg = masix_ipc::OutboundMessage {
                                channel,
                                account_tag,
                                chat_id,
                                text: message,
                                reply_to: None,
                                edit_message_id: None,
                                inline_keyboard: None,
                                chat_action: None,
                            };

                            if let Err(e) = outbound_sender.send(msg).await {
                                tracing::error!("Failed to send cron job {}: {}", job.id, e);
                            }
                        } else {
                            tracing::warn!(
                                "Skipping cron job {}: non-numeric recipient '{}' for channel '{}'",
                                job.id,
                                recipient,
                                channel
                            );
                        }

                        if job.recurring {
                            if let Err(e) =
                                storage.update_cron_next_run(job.id, &job.schedule, &job.timezone)
                            {
                                tracing::error!(
                                    "Failed to update next_run for job {}: {}",
                                    job.id,
                                    e
                                );
                            }
                        } else if let Err(e) = storage.disable_cron_job(job.id) {
                            tracing::error!("Failed to disable job {}: {}", job.id, e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to get due cron jobs: {}", e);
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(self.check_interval_secs)).await;
        }
    }
}

impl Default for CronExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{CronExecutor, CronParser};

    #[test]
    fn parse_invalid_calendar_date_returns_error() {
        let parser = CronParser::new();
        let result = parser.parse(
            r#"il 31 febbraio alle 10 "Test reminder""#,
            "telegram",
            "12345",
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_domani_creates_non_recurring_schedule() {
        let parser = CronParser::new();
        let result = parser
            .parse(r#"domani alle 9 "Morning test""#, "telegram", "12345")
            .expect("expected valid schedule");
        assert!(!result.recurring);
        assert_eq!(result.channel, "telegram");
    }

    #[tokio::test]
    async fn run_without_storage_returns_error() {
        let executor = CronExecutor::new();
        let err = executor
            .run()
            .await
            .expect_err("run() must fail without storage");
        assert!(
            err.to_string().contains("use run_with_storage"),
            "unexpected error: {}",
            err
        );
    }
}
