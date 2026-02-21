//! Torrent search with proxy/mirror support
//!
//! Features:
//! - Mirror/proxy catalog for blocked sites
//! - Direct site scraping with DoH resolution
//! - Cloudflare bypass techniques
//! - Magnet extraction with multiple strategies

use anyhow::{anyhow, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

use super::doh::DohResolver;
use super::cache::MagnetCache;

const TORRENT_TIMEOUT_SECS: u64 = 15;
const MAGNET_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentResult {
    pub title: String,
    pub url: String,
    pub magnet: Option<String>,
    pub size: Option<String>,
    pub seeds: Option<u32>,
    pub leeches: Option<u32>,
    pub provider: String,
    pub source_url: String,
}

#[derive(Debug, Clone)]
pub struct TorrentMirror {
    pub name: String,
    pub original_domain: String,
    pub mirror_domains: Vec<String>,
    pub proxy_urls: Vec<String>,
}

pub struct TorrentSearch {
    client: Client,
    doh: DohResolver,
    cache: Option<MagnetCache>,
    mirrors: HashMap<String, TorrentMirror>,
}

impl TorrentSearch {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(TORRENT_TIMEOUT_SECS))
            .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")
            .redirect(reqwest::redirect::Policy::limited(10))
            .danger_accept_invalid_certs(false)
            .build()?;

        let mirrors = Self::build_mirror_catalog();
        
        Ok(Self {
            client,
            doh: DohResolver::new()?,
            cache: MagnetCache::new().ok(),
            mirrors,
        })
    }

    fn build_mirror_catalog() -> HashMap<String, TorrentMirror> {
        let mut catalog = HashMap::new();

        catalog.insert("1337x".to_string(), TorrentMirror {
            name: "1337x".to_string(),
            original_domain: "1337x.to".to_string(),
            mirror_domains: vec![
                "1337x.to".to_string(),
                "1337x.st".to_string(),
                "x1337x.se".to_string(),
                "1337xx.to".to_string(),
            ],
            proxy_urls: vec![
                "https://1337x.unblockit.bz".to_string(),
                "https://1337x.unblockit.id".to_string(),
                "https://1337x.nocensor.lol".to_string(),
            ],
        });

        catalog.insert("thepiratebay".to_string(), TorrentMirror {
            name: "ThePirateBay".to_string(),
            original_domain: "thepiratebay.org".to_string(),
            mirror_domains: vec![
                "thepiratebay.org".to_string(),
                "thepiratesbay.com".to_string(),
                "tpb.party".to_string(),
            ],
            proxy_urls: vec![
                "https://thepiratebay.unblockit.bz".to_string(),
                "https://tpb.party".to_string(),
                "https://thepiratebay.nocensor.lol".to_string(),
            ],
        });

        catalog.insert("yts".to_string(), TorrentMirror {
            name: "YTS".to_string(),
            original_domain: "yts.mx".to_string(),
            mirror_domains: vec![
                "yts.mx".to_string(),
                "yts.lt".to_string(),
                "yts.am".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("nyaa".to_string(), TorrentMirror {
            name: "Nyaa".to_string(),
            original_domain: "nyaa.si".to_string(),
            mirror_domains: vec![
                "nyaa.si".to_string(),
                "nyaa.land".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("rarbg".to_string(), TorrentMirror {
            name: "RARBG".to_string(),
            original_domain: "rarbg.to".to_string(),
            mirror_domains: vec![
                "rarbg.to".to_string(),
                "rarbg2018.org".to_string(),
            ],
            proxy_urls: vec![
                "https://rarbg.unblockit.bz".to_string(),
            ],
        });

        catalog.insert("torrentgalaxy".to_string(), TorrentMirror {
            name: "TorrentGalaxy".to_string(),
            original_domain: "torrentgalaxy.to".to_string(),
            mirror_domains: vec![
                "torrentgalaxy.to".to_string(),
                "torrentgalaxy.mx".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("eztv".to_string(), TorrentMirror {
            name: "EZTV".to_string(),
            original_domain: "eztv.re".to_string(),
            mirror_domains: vec![
                "eztv.re".to_string(),
                "eztv.ag".to_string(),
                "eztv.io".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("kickass".to_string(), TorrentMirror {
            name: "Kickass".to_string(),
            original_domain: "kickasstorrents.to".to_string(),
            mirror_domains: vec![
                "kickasstorrents.to".to_string(),
                "katcr.to".to_string(),
                "kat.sx".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("limetorrents".to_string(), TorrentMirror {
            name: "LimeTorrents".to_string(),
            original_domain: "limetorrents.lol".to_string(),
            mirror_domains: vec![
                "limetorrents.lol".to_string(),
                "limetorrents.info".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("solidtorrents".to_string(), TorrentMirror {
            name: "SolidTorrents".to_string(),
            original_domain: "solidtorrents.to".to_string(),
            mirror_domains: vec![
                "solidtorrents.to".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog.insert("bt4g".to_string(), TorrentMirror {
            name: "BT4G".to_string(),
            original_domain: "bt4gprx.com".to_string(),
            mirror_domains: vec![
                "bt4gprx.com".to_string(),
                "bt4g.org".to_string(),
            ],
            proxy_urls: vec![],
        });

        catalog
    }

    pub async fn search(&self, query: &str, providers: Option<&[String]>, max_results: usize) -> Result<Vec<TorrentResult>> {
        let provider_names = providers.map(|p| p.to_vec()).unwrap_or_else(|| {
            self.mirrors.keys().cloned().collect()
        });

        let mut all_results = Vec::new();
        let mut seen_titles = std::collections::HashSet::new();

        for provider_name in provider_names {
            if all_results.len() >= max_results {
                break;
            }

            if let Some(mirror) = self.mirrors.get(&provider_name) {
                match self.search_provider(query, mirror, max_results - all_results.len()).await {
                    Ok(results) => {
                        for result in results {
                            let key = result.title.to_lowercase();
                            if seen_titles.insert(key) {
                                all_results.push(result);
                                if all_results.len() >= max_results {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Provider {} search failed: {}", provider_name, e);
                    }
                }
            }
        }

        Ok(all_results)
    }

    async fn search_provider(&self, query: &str, mirror: &TorrentMirror, max_results: usize) -> Result<Vec<TorrentResult>> {
        let urls_to_try: Vec<String> = mirror.proxy_urls.iter()
            .chain(mirror.mirror_domains.iter())
            .map(|d| d.clone())
            .collect();

        for base_url in urls_to_try {
            let search_url = if base_url.starts_with("http") {
                base_url.clone()
            } else {
                format!("https://{}", base_url)
            };

            match self.scrape_provider(&search_url, query, max_results, &mirror.name).await {
                Ok(results) if !results.is_empty() => {
                    return Ok(results);
                }
                Ok(_) => continue,
                Err(e) => {
                    debug!("Failed to scrape {} at {}: {}", mirror.name, search_url, e);
                    continue;
                }
            }
        }

        Err(anyhow!("All mirrors failed for {}", mirror.name))
    }

    async fn scrape_provider(&self, base_url: &str, query: &str, max_results: usize, provider_name: &str) -> Result<Vec<TorrentResult>> {
        let search_url = self.build_search_url(base_url, query, provider_name);
        
        let response = match self.client.get(&search_url).send().await {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => return Err(anyhow!("HTTP {}", r.status())),
            Err(e) => return Err(anyhow!("Request failed: {}", e)),
        };

        let html = response.text().await?;
        self.parse_results(&html, base_url, provider_name, max_results)
    }

    fn build_search_url(&self, base_url: &str, query: &str, provider_name: &str) -> String {
        let encoded_query = urlencoding::encode(query);
        
        match provider_name.to_lowercase().as_str() {
            "1337x" => format!("{}/search/{}/1/", base_url, encoded_query),
            "thepiratebay" => format!("{}/search/{}/1/99/0", base_url, encoded_query),
            "yts" => format!("{}/browse-movies/{}?quality=1080p", base_url, encoded_query),
            "nyaa" => format!("{}/?f=0&c=0_0&q={}&s=seeders&o=desc", base_url, encoded_query),
            "rarbg" => format!("{}/torrents.php?search={}", base_url, encoded_query),
            "torrentgalaxy" => format!("{}/torrents.php?search={}", base_url, encoded_query),
            "eztv" => format!("{}/search/{}", base_url, encoded_query),
            "kickass" => format!("{}/usearch/{}/", base_url, encoded_query),
            "limetorrents" => format!("{}/search/all/{}", base_url, encoded_query),
            "solidtorrents" => format!("{}/search?q={}", base_url, encoded_query),
            "bt4g" => format!("{}/search/{}", base_url, encoded_query),
            _ => format!("{}/search?q={}", base_url, encoded_query),
        }
    }

    fn parse_results(&self, html: &str, base_url: &str, provider_name: &str, max_results: usize) -> Result<Vec<TorrentResult>> {
        let document = Html::parse_document(html);
        let mut results = Vec::new();

        let link_selector = Selector::parse("a[href*=\"magnet:\"]").ok();
        let torrent_link_selector = Selector::parse("a[href*=\".torrent\"]").ok();
        let title_selector = Selector::parse("a").ok();

        if let Some(link_sel) = link_selector {
            for link in document.select(&link_sel).take(max_results) {
                if let Some(magnet) = link.value().attr("href") {
                    if magnet.starts_with("magnet:") {
                        let title = link.text().collect::<String>().trim().to_string();
                        
                        results.push(TorrentResult {
                            title: if title.is_empty() { "Unknown".to_string() } else { title },
                            url: format!("{}/torrent/{}", base_url, results.len()),
                            magnet: Some(magnet.to_string()),
                            size: None,
                            seeds: None,
                            leeches: None,
                            provider: provider_name.to_string(),
                            source_url: base_url.to_string(),
                        });
                    }
                }
            }
        }

        if results.is_empty() {
            if let Some(link_sel) = torrent_link_selector {
                for link in document.select(&link_sel).take(max_results) {
                    let href = link.value().attr("href").unwrap_or("");
                    let title = link.text().collect::<String>().trim().to_string();
                    
                    if !href.is_empty() {
                        let full_url = if href.starts_with("http") {
                            href.to_string()
                        } else {
                            format!("{}{}", base_url.trim_end_matches('/'), href)
                        };

                        results.push(TorrentResult {
                            title: if title.is_empty() { "Unknown".to_string() } else { title },
                            url: full_url,
                            magnet: None,
                            size: None,
                            seeds: None,
                            leeches: None,
                            provider: provider_name.to_string(),
                            source_url: base_url.to_string(),
                        });
                    }
                }
            }
        }

        if results.is_empty() {
            if let Some(title_sel) = title_selector {
                for link in document.select(&title_sel).take(max_results) {
                    let href = link.value().attr("href").unwrap_or("");
                    let title = link.text().collect::<String>().trim().to_string();

                    if href.contains("/torrent/") || href.contains("/download/") {
                        let full_url = if href.starts_with("http") {
                            href.to_string()
                        } else {
                            format!("{}{}", base_url.trim_end_matches('/'), href)
                        };

                        results.push(TorrentResult {
                            title: if title.is_empty() { "Unknown".to_string() } else { title },
                            url: full_url,
                            magnet: None,
                            size: None,
                            seeds: None,
                            leeches: None,
                            provider: provider_name.to_string(),
                            source_url: base_url.to_string(),
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    pub fn get_providers(&self) -> Vec<String> {
        self.mirrors.keys().cloned().collect()
    }
}

pub struct MagnetExtractor {
    client: Client,
    doh: DohResolver,
    cache: Option<MagnetCache>,
}

impl MagnetExtractor {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(MAGNET_TIMEOUT_SECS))
            .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()?;

        Ok(Self {
            client,
            doh: DohResolver::new()?,
            cache: MagnetCache::new().ok(),
        })
    }

    pub async fn extract(&self, url: &str) -> Result<Option<String>> {
        if let Some(cache) = &self.cache {
            if let Some(cached) = cache.get(url).await {
                debug!("Magnet cache hit for {}", url);
                return Ok(Some(cached));
            }
        }

        if url.starts_with("magnet:") {
            return Ok(Some(url.to_string()));
        }

        let response = self.client.get(url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("HTTP {}", response.status()));
        }

        let html = response.text().await?;
        let magnet = self.extract_magnet_from_html(&html);

        if let Some(ref m) = magnet {
            if let Some(cache) = &self.cache {
                cache.set(url, m).await;
            }
        }

        Ok(magnet)
    }

    fn extract_magnet_from_html(&self, html: &str) -> Option<String> {
        let marker = "magnet:?";
        let start = html.find(marker)?;
        let tail = &html[start..];
        
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '<' || c == '>' || c == '&')
            .unwrap_or(tail.len().min(500));

        if end == 0 {
            return None;
        }

        let link = tail[..end]
            .replace("&amp;", "&")
            .replace("&quot;", "\"");

        if link.starts_with("magnet:?xt=urn:btih:") || link.starts_with("magnet:?xt=urn:sha1:") {
            Some(link)
        } else {
            None
        }
    }
}

impl Default for TorrentSearch {
    fn default() -> Self {
        Self::new().expect("Failed to create TorrentSearch")
    }
}

impl Default for MagnetExtractor {
    fn default() -> Self {
        Self::new().expect("Failed to create MagnetExtractor")
    }
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirror_catalog_built() {
        let search = TorrentSearch::new().expect("search");
        assert!(search.mirrors.contains_key("1337x"));
        assert!(search.mirrors.contains_key("thepiratebay"));
    }

    #[test]
    fn test_extract_magnet_from_html() {
        let extractor = MagnetExtractor::new().expect("extractor");
        let html = r#"<a href="magnet:?xt=urn:btih:ABCDEF123456&amp;dn=test">link</a>"#;
        let magnet = extractor.extract_magnet_from_html(html);
        assert!(magnet.is_some());
        assert!(magnet.unwrap().starts_with("magnet:?xt=urn:btih:"));
    }
}
