//! Magnet cache for persistent storage
//!
//! Caches extracted magnet links to avoid repeated fetches

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::sync::Mutex;
use tracing::debug;

use super::MAGNET_CACHE_MAX_AGE_HOURS;

const CACHE_FILE: &str = "magnet_cache.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    magnet: String,
    url: String,
    timestamp: u64,
}

pub struct MagnetCache {
    path: PathBuf,
    data: Mutex<HashMap<String, CacheEntry>>,
}

impl MagnetCache {
    pub fn new() -> Result<Self> {
        let base_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("masix");

        let path = base_dir.join(CACHE_FILE);
        
        Ok(Self {
            path,
            data: Mutex::new(HashMap::new()),
        })
    }

    pub async fn get(&self, url: &str) -> Option<String> {
        let data = self.data.lock().await;
        
        if let Some(entry) = data.get(url) {
            if self.is_fresh(entry.timestamp) {
                return Some(entry.magnet.clone());
            }
        }

        None
    }

    pub async fn set(&self, url: &str, magnet: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let entry = CacheEntry {
            magnet: magnet.to_string(),
            url: url.to_string(),
            timestamp: now,
        };

        {
            let mut data = self.data.lock().await;
            data.insert(url.to_string(), entry);
        }

        if let Err(e) = self.save().await {
            debug!("Failed to save magnet cache: {}", e);
        }
    }

    pub async fn load(&self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }

        let content = fs::read_to_string(&self.path).await?;
        let entries: Vec<CacheEntry> = serde_json::from_str(&content)?;

        let mut data = self.data.lock().await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for entry in entries {
            if self.is_fresh_at(entry.timestamp, now) {
                data.insert(entry.url.clone(), entry);
            }
        }

        debug!("Loaded {} cached magnets", data.len());
        Ok(())
    }

    async fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let data = self.data.lock().await;
        let entries: Vec<&CacheEntry> = data.values().collect();
        let content = serde_json::to_string_pretty(&entries)?;

        fs::write(&self.path, content).await?;
        debug!("Saved {} cached magnets", entries.len());
        Ok(())
    }

    fn is_fresh(&self, timestamp: u64) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.is_fresh_at(timestamp, now)
    }

    fn is_fresh_at(&self, timestamp: u64, now: u64) -> bool {
        let max_age_secs = MAGNET_CACHE_MAX_AGE_HOURS * 3600;
        now.saturating_sub(timestamp) < max_age_secs
    }

    pub async fn clear(&self) -> Result<()> {
        let mut data = self.data.lock().await;
        data.clear();

        if self.path.exists() {
            fs::remove_file(&self.path).await?;
        }

        Ok(())
    }

    pub async fn size(&self) -> usize {
        self.data.lock().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_set_get() {
        let cache = MagnetCache::new().expect("cache");
        cache.set("https://example.com/torrent", "magnet:?xt=urn:btih:ABC").await;
        
        let result = cache.get("https://example.com/torrent").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "magnet:?xt=urn:btih:ABC");
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = MagnetCache::new().expect("cache");
        let result = cache.get("https://nonexistent.com").await;
        assert!(result.is_none());
    }
}
