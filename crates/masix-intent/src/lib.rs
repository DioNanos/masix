//! Masix Intent Module
//!
//! Safe wrapper around Android `am` commands for intent dispatch from Termux.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntentRequest {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub extras_string: Vec<IntentExtraString>,
    #[serde(default)]
    pub extras_bool: Vec<IntentExtraBool>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub dry_run: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentExtraString {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentExtraBool {
    pub key: String,
    pub value: bool,
}

fn validate_token(value: &str, field: &str) -> Result<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        anyhow::bail!("{} cannot be empty", field);
    }
    if trimmed.chars().any(|c| c == '\n' || c == '\r' || c == '\0') {
        anyhow::bail!("{} contains invalid characters", field);
    }
    Ok(())
}

pub fn build_intent_args(request: &IntentRequest) -> Result<Vec<String>> {
    let mode = request.mode.as_deref().unwrap_or("start");
    let mode_cmd = match mode {
        "start" => "start",
        "broadcast" => "broadcast",
        "service" => "startservice",
        _ => anyhow::bail!("Unsupported intent mode '{}'", mode),
    };

    let mut args = vec![mode_cmd.to_string()];

    if let Some(action) = request.action.as_deref() {
        validate_token(action, "action")?;
        args.push("-a".to_string());
        args.push(action.trim().to_string());
    }

    if let Some(data) = request.data.as_deref() {
        validate_token(data, "data")?;
        args.push("-d".to_string());
        args.push(data.trim().to_string());
    }

    if request.package.is_some() || request.class.is_some() {
        let package = request
            .package
            .as_deref()
            .ok_or_else(|| anyhow!("package is required when class is set"))?;
        let class = request
            .class
            .as_deref()
            .ok_or_else(|| anyhow!("class is required when package is set"))?;
        validate_token(package, "package")?;
        validate_token(class, "class")?;
        args.push("-n".to_string());
        args.push(format!("{}/{}", package.trim(), class.trim()));
    }

    for category in &request.categories {
        validate_token(category, "category")?;
        args.push("-c".to_string());
        args.push(category.trim().to_string());
    }

    for extra in &request.extras_string {
        validate_token(&extra.key, "extras_string.key")?;
        validate_token(&extra.value, "extras_string.value")?;
        args.push("--es".to_string());
        args.push(extra.key.trim().to_string());
        args.push(extra.value.trim().to_string());
    }

    for extra in &request.extras_bool {
        validate_token(&extra.key, "extras_bool.key")?;
        args.push("--ez".to_string());
        args.push(extra.key.trim().to_string());
        args.push(if extra.value { "true" } else { "false" }.to_string());
    }

    for flag in &request.flags {
        validate_token(flag, "flag")?;
        args.push(flag.trim().to_string());
    }

    if args.len() == 1 {
        anyhow::bail!("Intent request is empty: set at least action/data/component");
    }

    Ok(args)
}

pub async fn execute_intent(request: &IntentRequest) -> Result<String> {
    let args = build_intent_args(request)?;
    let dry_run = request.dry_run.unwrap_or(false);

    if dry_run {
        return Ok(format!("dry-run: am {}", args.join(" ")));
    }

    let output = Command::new("am").args(&args).output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            Ok("Intent dispatched.".to_string())
        } else if stderr.is_empty() {
            Ok(stdout)
        } else if stdout.is_empty() {
            Ok(stderr)
        } else {
            Ok(format!("{}\n{}", stdout, stderr))
        }
    } else {
        Err(anyhow!(
            "am {} failed: {}",
            args.join(" "),
            if stderr.is_empty() {
                "unknown error".to_string()
            } else {
                stderr
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{build_intent_args, IntentExtraString, IntentRequest};

    #[test]
    fn build_intent_args_start_activity() {
        let req = IntentRequest {
            action: Some("android.intent.action.VIEW".to_string()),
            data: Some("https://example.com".to_string()),
            ..Default::default()
        };
        let args = build_intent_args(&req).expect("build args");
        assert_eq!(
            args,
            vec![
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                "https://example.com"
            ]
        );
    }

    #[test]
    fn build_intent_args_component_and_extra() {
        let req = IntentRequest {
            mode: Some("broadcast".to_string()),
            package: Some("com.example.app".to_string()),
            class: Some("com.example.app.Receiver".to_string()),
            extras_string: vec![IntentExtraString {
                key: "source".to_string(),
                value: "masix".to_string(),
            }],
            ..Default::default()
        };
        let args = build_intent_args(&req).expect("build args");
        assert_eq!(
            args,
            vec![
                "broadcast",
                "-n",
                "com.example.app/com.example.app.Receiver",
                "--es",
                "source",
                "masix"
            ]
        );
    }
}
