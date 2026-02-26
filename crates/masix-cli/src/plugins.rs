use crate::PluginCommands;
use anyhow::{anyhow, Context, Result};
use masix_config::Config;
use masix_exec::is_termux_environment;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_PLUGIN_SERVER_URL: &str = "http://127.0.0.1:8787";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginCatalog {
    #[serde(default)]
    catalog_version: u32,
    #[serde(default)]
    generated_at: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    plugins: Vec<PluginCatalogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginCatalogEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
    version: String,
    #[serde(default = "default_visibility")]
    visibility: String,
    #[serde(default)]
    requires_masix: Option<String>,
    #[serde(default)]
    platforms: Vec<String>,
    #[serde(default)]
    package_type: Option<String>,
    #[serde(default)]
    entrypoint: Option<String>,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PluginAuthStore {
    #[serde(default)]
    server_url: Option<String>,
    #[serde(default)]
    plugin_keys: BTreeMap<String, String>,
    #[serde(default)]
    updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginAuthRequest {
    plugin_id: String,
    license_key: String,
    device_id: String,
    masix_version: String,
    platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PluginAuthResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct InstalledPluginsRegistry {
    #[serde(default)]
    plugins: Vec<InstalledPluginRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPluginRecord {
    plugin_id: String,
    version: String,
    visibility: String,
    platform: String,
    source_server: String,
    install_path: String,
    #[serde(default)]
    entrypoint: Option<String>,
    installed_at: u64,
    enabled: bool,
}

fn default_visibility() -> String {
    "free".to_string()
}

pub async fn handle_plugin_command(action: PluginCommands, config_path: Option<String>) -> Result<()> {
    match action {
        PluginCommands::List {
            server,
            platform,
            json,
        } => {
            let platform = platform.unwrap_or_else(plugin_platform_id);
            let server_url = resolve_plugin_server_url(server, None);
            let catalog = fetch_catalog(&server_url, &platform).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&catalog)?);
                return Ok(());
            }

            println!("Plugin server: {}", server_url);
            println!("Platform: {}", platform);
            println!("Catalog version: {}", catalog.catalog_version);
            if let Some(ts) = &catalog.generated_at {
                println!("Generated at: {}", ts);
            }
            println!();

            let mut entries: Vec<_> = catalog
                .plugins
                .into_iter()
                .filter(|p| p.platforms.is_empty() || p.platforms.iter().any(|x| x == &platform))
                .collect();
            entries.sort_by(|a, b| {
                a.id.cmp(&b.id)
                    .then_with(|| cmp_versionish(&b.version, &a.version))
            });

            if entries.is_empty() {
                println!("No plugins available for this platform.");
                return Ok(());
            }

            for entry in entries {
                println!(
                    "- {} {} [{}]{}",
                    entry.id,
                    entry.version,
                    entry.visibility,
                    entry
                        .package_type
                        .as_ref()
                        .map(|v| format!(" ({})", v))
                        .unwrap_or_default()
                );
                if let Some(name) = &entry.name {
                    println!("  {}", name);
                }
                if let Some(req) = &entry.requires_masix {
                    println!("  requires_masix: {}", req);
                }
                if !entry.platforms.is_empty() {
                    println!("  platforms: {}", entry.platforms.join(", "));
                }
            }
        }
        PluginCommands::Auth {
            plugin,
            key,
            server,
            platform,
        } => {
            let platform = platform.unwrap_or_else(plugin_platform_id);
            let data_dir = resolve_data_dir(config_path.as_deref());
            let plugins_dir = plugin_root_dir(&data_dir);
            let auth_path = plugin_auth_store_path(&plugins_dir);
            let mut auth_store = load_auth_store(&auth_path)?;
            let server_url = resolve_plugin_server_url(server, auth_store.server_url.clone());

            let _catalog = fetch_catalog(&server_url, &platform).await?;
            let auth_resp = request_plugin_auth(&server_url, &plugin, &key, &platform).await?;
            auth_store.server_url = Some(server_url.clone());
            auth_store.plugin_keys.insert(plugin.clone(), key);
            auth_store.updated_at = Some(now_unix_secs());
            save_auth_store(&auth_path, &auth_store)?;

            println!("Plugin key stored for '{}'.", plugin);
            if let Some(msg) = auth_resp.message {
                println!("Server: {}", msg);
            }
            if let Some(exp) = auth_resp.expires_at {
                println!("Token expires: {}", exp);
            }
        }
        PluginCommands::Install {
            plugin,
            version,
            key,
            server,
            platform,
        } => {
            let platform = platform.unwrap_or_else(plugin_platform_id);
            let data_dir = resolve_data_dir(config_path.as_deref());
            let plugins_dir = plugin_root_dir(&data_dir);
            let auth_path = plugin_auth_store_path(&plugins_dir);
            let mut auth_store = load_auth_store(&auth_path)?;
            let server_url = resolve_plugin_server_url(server, auth_store.server_url.clone());
            let catalog = fetch_catalog(&server_url, &platform).await?;

            let install_result = install_plugin_from_catalog(
                &plugins_dir,
                &server_url,
                &catalog,
                &plugin,
                version.as_deref(),
                &platform,
                key.as_deref(),
                &mut auth_store,
            )
            .await?;
            save_auth_store(&auth_path, &auth_store)?;
            println!("Installed plugin '{}' {}", install_result.plugin_id, install_result.version);
            println!("Package: {}", install_result.install_path);
        }
        PluginCommands::Update {
            plugin,
            key,
            server,
            platform,
        } => {
            let platform = platform.unwrap_or_else(plugin_platform_id);
            let data_dir = resolve_data_dir(config_path.as_deref());
            let plugins_dir = plugin_root_dir(&data_dir);
            let auth_path = plugin_auth_store_path(&plugins_dir);
            let registry_path = plugin_registry_path(&plugins_dir);
            let mut auth_store = load_auth_store(&auth_path)?;
            let mut registry = load_registry(&registry_path)?;
            let server_url = resolve_plugin_server_url(server, auth_store.server_url.clone());
            let catalog = fetch_catalog(&server_url, &platform).await?;

            if registry.plugins.is_empty() {
                println!("No installed plugins found.");
                return Ok(());
            }

            let targets: Vec<String> = if let Some(id) = plugin {
                vec![id]
            } else {
                let mut ids: Vec<String> =
                    registry.plugins.iter().map(|p| p.plugin_id.clone()).collect();
                ids.sort();
                ids.dedup();
                ids
            };

            let mut updated = 0usize;
            let mut checked = 0usize;
            for plugin_id in targets {
                checked += 1;
                let installed = registry
                    .plugins
                    .iter()
                    .find(|p| p.plugin_id == plugin_id)
                    .cloned();
                let Some(installed) = installed else {
                    println!("- {}: not installed", plugin_id);
                    continue;
                };

                let candidate = select_catalog_plugin(&catalog, &plugin_id, None, &platform)?;
                if candidate.version == installed.version {
                    println!("- {}: up-to-date ({})", plugin_id, installed.version);
                    continue;
                }
                println!(
                    "- {}: updating {} -> {}",
                    plugin_id, installed.version, candidate.version
                );

                install_plugin_from_catalog(
                    &plugins_dir,
                    &server_url,
                    &catalog,
                    &plugin_id,
                    Some(&candidate.version),
                    &platform,
                    key.as_deref(),
                    &mut auth_store,
                )
                .await?;
                updated += 1;
            }

            // Refresh registry from disk after install(s)
            registry = load_registry(&registry_path)?;
            save_auth_store(&auth_path, &auth_store)?;
            println!(
                "Update check complete: {} checked, {} updated, {} installed total.",
                checked,
                updated,
                registry.plugins.len()
            );
        }
    }

    Ok(())
}

async fn install_plugin_from_catalog(
    plugins_dir: &Path,
    server_url: &str,
    catalog: &PluginCatalog,
    plugin_id: &str,
    version: Option<&str>,
    platform: &str,
    key_override: Option<&str>,
    auth_store: &mut PluginAuthStore,
) -> Result<InstalledPluginRecord> {
    let entry = select_catalog_plugin(catalog, plugin_id, version, platform)?;

    let auth_resp = if entry.visibility.eq_ignore_ascii_case("private") {
        let key = key_override
            .map(str::to_string)
            .or_else(|| auth_store.plugin_keys.get(plugin_id).cloned())
            .ok_or_else(|| {
                anyhow!(
                    "Plugin '{}' is private. Run `masix plugin auth {} --key <KEY>` or pass --key.",
                    plugin_id,
                    plugin_id
                )
            })?;
        let resp = request_plugin_auth(server_url, plugin_id, &key, platform).await?;
        if key_override.is_some() {
            auth_store.plugin_keys.insert(plugin_id.to_string(), key);
            auth_store.updated_at = Some(now_unix_secs());
        }
        Some(resp)
    } else {
        None
    };

    let package_bytes = download_plugin_package(server_url, &entry, platform, auth_resp.as_ref()).await?;
    verify_download_hash(&package_bytes, entry.sha256.as_deref())?;

    let file_name = format!(
        "{}-{}-{}.pkg",
        entry.id,
        entry.version,
        sanitize_file_component(
            entry.entrypoint
                .as_deref()
                .unwrap_or_else(|| entry.package_type.as_deref().unwrap_or("module"))
        )
    );
    let package_path = plugins_dir
        .join("packages")
        .join(&entry.id)
        .join(&entry.version)
        .join(file_name);
    if let Some(parent) = package_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&package_path, &package_bytes)?;

    let registry_path = plugin_registry_path(plugins_dir);
    let mut registry = load_registry(&registry_path)?;
    registry
        .plugins
        .retain(|p| !(p.plugin_id == entry.id && p.platform == platform));
    let record = InstalledPluginRecord {
        plugin_id: entry.id.clone(),
        version: entry.version.clone(),
        visibility: entry.visibility.clone(),
        platform: platform.to_string(),
        source_server: server_url.to_string(),
        install_path: package_path.display().to_string(),
        entrypoint: entry.entrypoint.clone(),
        installed_at: now_unix_secs(),
        enabled: false,
    };
    registry.plugins.push(record.clone());
    registry.plugins.sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    save_registry(&registry_path, &registry)?;

    Ok(record)
}

fn select_catalog_plugin<'a>(
    catalog: &'a PluginCatalog,
    plugin_id: &str,
    version: Option<&str>,
    platform: &str,
) -> Result<&'a PluginCatalogEntry> {
    let mut candidates: Vec<&PluginCatalogEntry> = catalog
        .plugins
        .iter()
        .filter(|entry| entry.id == plugin_id)
        .filter(|entry| entry.platforms.is_empty() || entry.platforms.iter().any(|p| p == platform))
        .collect();

    if candidates.is_empty() {
        anyhow::bail!(
            "Plugin '{}' not found in catalog for platform '{}'",
            plugin_id,
            platform
        );
    }

    if let Some(version) = version {
        return candidates
            .into_iter()
            .find(|entry| entry.version == version)
            .ok_or_else(|| anyhow!("Plugin '{}' version '{}' not found", plugin_id, version));
    }

    candidates.sort_by(|a, b| cmp_versionish(&b.version, &a.version));
    Ok(candidates[0])
}

async fn fetch_catalog(server_url: &str, platform: &str) -> Result<PluginCatalog> {
    let url = format!(
        "{}/v1/plugins/catalog?platform={}&masix_version={}",
        server_url.trim_end_matches('/'),
        url_encode(platform),
        env!("CARGO_PKG_VERSION")
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch plugin catalog from {}", url))?;
    if !response.status().is_success() {
        anyhow::bail!("Plugin catalog request failed: HTTP {} ({})", response.status(), url);
    }
    let catalog: PluginCatalog = response
        .json()
        .await
        .with_context(|| format!("Invalid plugin catalog JSON from {}", url))?;
    Ok(catalog)
}

async fn request_plugin_auth(
    server_url: &str,
    plugin_id: &str,
    key: &str,
    platform: &str,
) -> Result<PluginAuthResponse> {
    let url = format!("{}/v1/plugins/auth", server_url.trim_end_matches('/'));
    let req = PluginAuthRequest {
        plugin_id: plugin_id.to_string(),
        license_key: key.to_string(),
        device_id: plugin_device_id(),
        masix_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: platform.to_string(),
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let response = client
        .post(&url)
        .json(&req)
        .send()
        .await
        .with_context(|| format!("Failed to reach plugin auth endpoint {}", url))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Plugin auth failed (HTTP {}): {}", status, body.trim());
    }
    let parsed: PluginAuthResponse = response
        .json()
        .await
        .with_context(|| format!("Invalid auth response JSON from {}", url))?;
    Ok(parsed)
}

async fn download_plugin_package(
    server_url: &str,
    entry: &PluginCatalogEntry,
    platform: &str,
    auth: Option<&PluginAuthResponse>,
) -> Result<Vec<u8>> {
    let mut url = auth
        .and_then(|resp| resp.download_url.clone())
        .unwrap_or_else(|| {
            format!(
                "{}/v1/plugins/download/{}/{}?platform={}",
                server_url.trim_end_matches('/'),
                url_encode(&entry.id),
                url_encode(&entry.version),
                url_encode(platform)
            )
        });
    if !url.contains("platform=") {
        let sep = if url.contains('?') { '&' } else { '?' };
        url.push(sep);
        url.push_str("platform=");
        url.push_str(&url_encode(platform));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let mut request = client.get(&url);
    if let Some(token) = auth.and_then(|resp| resp.access_token.as_ref()) {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("Failed to download plugin package from {}", url))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Plugin download failed (HTTP {}): {}", status, body.trim());
    }
    Ok(response.bytes().await?.to_vec())
}

fn verify_download_hash(bytes: &[u8], expected_sha256: Option<&str>) -> Result<()> {
    let Some(expected) = expected_sha256.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if expected.contains('<') || expected.eq_ignore_ascii_case("skip") {
        return Ok(());
    }
    let computed = sha256_hex(bytes);
    if !computed.eq_ignore_ascii_case(expected) {
        anyhow::bail!(
            "Plugin hash mismatch: expected {}, got {}",
            expected,
            computed
        );
    }
    Ok(())
}

fn load_auth_store(path: &Path) -> Result<PluginAuthStore> {
    if !path.exists() {
        return Ok(PluginAuthStore::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let parsed = serde_json::from_str(&raw).context("Invalid plugin auth store JSON")?;
    Ok(parsed)
}

fn save_auth_store(path: &Path, store: &PluginAuthStore) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(store)?;
    std::fs::write(path, body)?;
    Ok(())
}

fn load_registry(path: &Path) -> Result<InstalledPluginsRegistry> {
    if !path.exists() {
        return Ok(InstalledPluginsRegistry::default());
    }
    let raw = std::fs::read_to_string(path)?;
    let parsed = serde_json::from_str(&raw).context("Invalid installed plugin registry JSON")?;
    Ok(parsed)
}

fn save_registry(path: &Path, registry: &InstalledPluginsRegistry) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(registry)?;
    std::fs::write(path, body)?;
    Ok(())
}

fn resolve_plugin_server_url(cli_override: Option<String>, stored: Option<String>) -> String {
    cli_override
        .or_else(|| std::env::var("MASIX_PLUGIN_SERVER_URL").ok())
        .or(stored)
        .unwrap_or_else(|| DEFAULT_PLUGIN_SERVER_URL.to_string())
}

fn resolve_data_dir(config_path: Option<&str>) -> PathBuf {
    if let Some(path) = config_path {
        if let Ok(config) = Config::load(path) {
            return data_dir_from_config(&config);
        }
    } else if let Some(default_path) = Config::default_path() {
        if let Ok(config) = Config::load(&default_path) {
            return data_dir_from_config(&config);
        }
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".masix")
}

fn data_dir_from_config(config: &Config) -> PathBuf {
    if let Some(data_dir) = &config.core.data_dir {
        if data_dir == "~" || data_dir.starts_with("~/") {
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            if data_dir == "~" {
                home
            } else {
                home.join(data_dir.trim_start_matches("~/"))
            }
        } else {
            PathBuf::from(data_dir)
        }
    } else {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".masix")
    }
}

fn plugin_root_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

fn plugin_auth_store_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join("auth.json")
}

fn plugin_registry_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join("installed.json")
}

fn plugin_platform_id() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    if is_termux_environment() && os == "android" {
        return format!("{}-{}-termux", os, arch);
    }
    format!("{}-{}", os, arch)
}

fn plugin_device_id() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(read_hostname_fallback)
        .unwrap_or_else(|| "unknown-host".to_string());
    let raw = format!(
        "{}|{}|{}|{}|{}",
        user,
        host,
        std::env::consts::OS,
        std::env::consts::ARCH,
        if is_termux_environment() { "termux" } else { "std" }
    );
    let digest = sha256_hex(raw.as_bytes());
    format!("mx-{}", &digest[..16.min(digest.len())])
}

fn read_hostname_fallback() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes.as_ref());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02x}", b);
    }
    out
}

fn sanitize_file_component(value: &str) -> String {
    let mut s = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').to_string()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for b in value.as_bytes() {
        let is_unreserved = matches!(
            *b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if is_unreserved {
            out.push(*b as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "%{:02X}", b);
        }
    }
    out
}

fn cmp_versionish(a: &str, b: &str) -> std::cmp::Ordering {
    let a_parts = versionish_parts(a);
    let b_parts = versionish_parts(b);
    for (x, y) in a_parts.iter().zip(b_parts.iter()) {
        let ord = x.cmp(y);
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }
    a_parts.len().cmp(&b_parts.len()).then_with(|| a.cmp(b))
}

fn versionish_parts(value: &str) -> Vec<String> {
    value
        .split(['.', '-', '_'])
        .map(|p| format!("{:020}", p.parse::<u64>().unwrap_or(0)))
        .collect()
}

