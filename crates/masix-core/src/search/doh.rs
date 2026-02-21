//! DNS-over-HTTPS resolver
//!
//! Bypasses ISP DNS blocking using Cloudflare and Google DoH

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use tracing::{debug, warn};

const CLOUDFLARE_DOH_URL: &str = "https://cloudflare-dns.com/dns-query";
const GOOGLE_DOH_URL: &str = "https://dns.google/dns-query";
const DOH_TIMEOUT_SECS: u64 = 5;

#[derive(Debug, Clone)]
pub struct DohResolver {
    client: Client,
    prefer_ipv4: bool,
}

impl DohResolver {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DOH_TIMEOUT_SECS))
            .user_agent("MasixDNS/1.0")
            .https_only(true)
            .build()?;

        Ok(Self {
            client,
            prefer_ipv4: true,
        })
    }

    pub async fn resolve(&self, hostname: &str) -> Result<Vec<IpAddr>> {
        let mut results: Vec<IpAddr> = Vec::new();

        if self.prefer_ipv4 {
            if let Ok(ips) = self.resolve_a(hostname).await {
                for ip in ips {
                    results.push(IpAddr::V4(ip));
                }
            }
            if let Ok(ips) = self.resolve_aaaa(hostname).await {
                for ip in ips {
                    results.push(IpAddr::V6(ip));
                }
            }
        } else {
            if let Ok(ips) = self.resolve_aaaa(hostname).await {
                for ip in ips {
                    results.push(IpAddr::V6(ip));
                }
            }
            if let Ok(ips) = self.resolve_a(hostname).await {
                for ip in ips {
                    results.push(IpAddr::V4(ip));
                }
            }
        }

        if results.is_empty() {
            return Err(anyhow!("No DNS records found for {}", hostname));
        }

        debug!("DoH resolved {} -> {:?}", hostname, results);
        Ok(results)
    }

    pub async fn resolve_ipv4(&self, hostname: &str) -> Result<Vec<Ipv4Addr>> {
        self.resolve_a(hostname).await
    }

    async fn resolve_a(&self, hostname: &str) -> Result<Vec<Ipv4Addr>> {
        let result = self.query_doh(hostname, "A", CLOUDFLARE_DOH_URL).await;
        
        match result {
            Ok(ips) => Ok(ips.into_iter().filter_map(|ip| {
                if let IpAddr::V4(v4) = ip { Some(v4) } else { None }
            }).collect()),
            Err(e) => {
                warn!("Cloudflare DoH A query failed for {}: {}, trying Google", hostname, e);
                let result = self.query_doh(hostname, "A", GOOGLE_DOH_URL).await?;
                Ok(result.into_iter().filter_map(|ip| {
                    if let IpAddr::V4(v4) = ip { Some(v4) } else { None }
                }).collect())
            }
        }
    }

    async fn resolve_aaaa(&self, hostname: &str) -> Result<Vec<Ipv6Addr>> {
        let result = self.query_doh(hostname, "AAAA", CLOUDFLARE_DOH_URL).await;
        
        match result {
            Ok(ips) => Ok(ips.into_iter().filter_map(|ip| {
                if let IpAddr::V6(v6) = ip { Some(v6) } else { None }
            }).collect()),
            Err(e) => {
                warn!("Cloudflare DoH AAAA query failed for {}: {}, trying Google", hostname, e);
                let result = self.query_doh(hostname, "AAAA", GOOGLE_DOH_URL).await?;
                Ok(result.into_iter().filter_map(|ip| {
                    if let IpAddr::V6(v6) = ip { Some(v6) } else { None }
                }).collect())
            }
        }
    }

    async fn query_doh(&self, hostname: &str, record_type: &str, doh_url: &str) -> Result<Vec<IpAddr>> {
        let url = format!(
            "{}?name={}&type={}",
            doh_url,
            urlencoding::encode(hostname),
            record_type
        );

        let response = self.client
            .get(&url)
            .header("Accept", "application/dns-json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("DoH query failed with status {}", response.status()));
        }

        let dns_response: DohResponse = response.json().await?;

        if dns_response.status != 0 {
            return Err(anyhow!("DNS query returned status {}", dns_response.status));
        }

        let ips: Vec<IpAddr> = dns_response.answer
            .unwrap_or_default()
            .into_iter()
            .filter_map(|record| {
                if record.record_type == 1 {
                    record.data.parse::<Ipv4Addr>().ok().map(IpAddr::V4)
                } else if record.record_type == 28 {
                    record.data.parse::<Ipv6Addr>().ok().map(IpAddr::V6)
                } else {
                    None
                }
            })
            .collect();

        Ok(ips)
    }

    pub async fn resolve_with_fallback(&self, hostname: &str) -> Result<String> {
        match self.resolve(hostname).await {
            Ok(ips) => {
                if let Some(first) = ips.first() {
                    return Ok(first.to_string());
                }
                Err(anyhow!("No IPs resolved for {}", hostname))
            }
            Err(e) => {
                warn!("DoH resolution failed for {}: {}", hostname, e);
                Ok(hostname.to_string())
            }
        }
    }
}

impl Default for DohResolver {
    fn default() -> Self {
        Self::new().expect("Failed to create DoH resolver")
    }
}

#[derive(Debug, Deserialize)]
struct DohResponse {
    #[serde(rename = "Status")]
    status: i32,
    #[serde(rename = "Answer")]
    answer: Option<Vec<DohRecord>>,
}

#[derive(Debug, Deserialize)]
struct DohRecord {
    #[serde(rename = "type")]
    record_type: i32,
    #[serde(rename = "data")]
    data: String,
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_doh_resolve_google() {
        let resolver = DohResolver::new().expect("resolver");
        let ips = resolver.resolve("dns.google").await.expect("ips");
        assert!(!ips.is_empty());
    }

    #[tokio::test]
    async fn test_doh_resolve_cloudflare() {
        let resolver = DohResolver::new().expect("resolver");
        let ips = resolver.resolve("cloudflare-dns.com").await.expect("ips");
        assert!(!ips.is_empty());
    }
}
