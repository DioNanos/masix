//! Multi-engine web search
//!
//! Aggregates results from multiple search engines via SearXNG, DuckDuckGo, Brave, Qwant

use anyhow::{anyhow, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

const SEARXNG_INSTANCES: &[&str] = &[
    "https://search.sapti.me",
    "https://searx.be",
    "https://search.bus-hit.me",
    "https://northboot.xyz",
    "https://search.gcomm.ch",
];

const SEARCH_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub engine: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SearchEngine {
    SearXNG,
    DuckDuckGo,
    Brave,
    Qwant,
}

impl std::fmt::Display for SearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchEngine::SearXNG => write!(f, "SearXNG"),
            SearchEngine::DuckDuckGo => write!(f, "DuckDuckGo"),
            SearchEngine::Brave => write!(f, "Brave"),
            SearchEngine::Qwant => write!(f, "Qwant"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MultiEngineSearch {
    client: Client,
    primary_engine: SearchEngine,
    fallback_engines: Vec<SearchEngine>,
}

impl MultiEngineSearch {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .user_agent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Mobile Safari/537.36")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;

        Ok(Self {
            client,
            primary_engine: SearchEngine::SearXNG,
            fallback_engines: vec![SearchEngine::DuckDuckGo, SearchEngine::Brave],
        })
    }

    pub async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let mut all_results = Vec::new();
        let mut seen_urls = std::collections::HashSet::new();

        let primary_results = self.search_engine(self.primary_engine, query, max_results).await;
        if let Ok(results) = primary_results {
            for result in results {
                if seen_urls.insert(result.url.clone()) {
                    all_results.push(result);
                }
            }
        }

        if all_results.len() < max_results {
            for engine in &self.fallback_engines {
                if all_results.len() >= max_results {
                    break;
                }
                
                let remaining = max_results - all_results.len();
                match self.search_engine(*engine, query, remaining).await {
                    Ok(results) => {
                        for result in results {
                            if seen_urls.insert(result.url.clone()) {
                                all_results.push(result);
                                if all_results.len() >= max_results {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("{} search failed: {}", engine, e);
                    }
                }
            }
        }

        if all_results.is_empty() {
            debug!("No results from any engine for query: {}", query);
        }

        Ok(all_results)
    }

    async fn search_engine(&self, engine: SearchEngine, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        match engine {
            SearchEngine::SearXNG => self.search_searxng(query, max_results).await,
            SearchEngine::DuckDuckGo => self.search_duckduckgo(query, max_results).await,
            SearchEngine::Brave => self.search_brave(query, max_results).await,
            SearchEngine::Qwant => self.search_qwant(query, max_results).await,
        }
    }

    async fn search_searxng(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        for instance in SEARXNG_INSTANCES {
            match self.try_searxng_instance(instance, query, max_results).await {
                Ok(results) if !results.is_empty() => {
                    debug!("SearXNG instance {} returned {} results", instance, results.len());
                    return Ok(results);
                }
                Ok(_) => continue,
                Err(e) => {
                    debug!("SearXNG instance {} failed: {}", instance, e);
                    continue;
                }
            }
        }
        Err(anyhow!("All SearXNG instances failed"))
    }

    async fn try_searxng_instance(&self, instance: &str, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "{}/search?q={}&format=json&engines=google,bing,duckduckgo,brave&qwant",
            instance,
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await?;
        
        if !response.status().is_success() {
            return Err(anyhow!("SearXNG returned {}", response.status()));
        }

        let json: SearXngResponse = response.json().await?;

        let results: Vec<SearchResult> = json.results
            .into_iter()
            .take(max_results)
            .map(|r| SearchResult {
                title: r.title.unwrap_or_default(),
                url: r.url.unwrap_or_default(),
                snippet: r.content.unwrap_or_default(),
                engine: "SearXNG".to_string(),
            })
            .collect();

        Ok(results)
    }

    async fn search_duckduckgo(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        let document = Html::parse_document(&html);
        let result_selector = Selector::parse(".result").ok();
        let title_selector = Selector::parse(".result__a").ok();
        let snippet_selector = Selector::parse(".result__snippet").ok();

        let mut results = Vec::new();

        if let (Some(result_sel), Some(title_sel), Some(snippet_sel)) =
            (result_selector, title_selector, snippet_selector)
        {
            for result in document.select(&result_sel).take(max_results) {
                let title = result
                    .select(&title_sel)
                    .next()
                    .map(|e| e.text().collect::<String>())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let snippet = result
                    .select(&snippet_sel)
                    .next()
                    .map(|e| e.text().collect::<String>())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let raw_link = result
                    .select(&title_sel)
                    .next()
                    .and_then(|e| e.value().attr("href"))
                    .unwrap_or("")
                    .to_string();

                let url = normalize_ddg_link(&raw_link);

                if !title.is_empty() && !url.is_empty() {
                    results.push(SearchResult {
                        title,
                        url,
                        snippet,
                        engine: "DuckDuckGo".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_brave(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://search.brave.com/search?q={}",
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        let document = Html::parse_document(&html);
        let snippet_selector = Selector::parse(".snippet").ok();
        let title_selector = Selector::parse(".snippet-title").ok();
        let url_selector = Selector::parse("a[href]").ok();

        let mut results = Vec::new();

        if let (Some(snippet_sel), Some(title_sel), Some(url_sel)) =
            (snippet_selector, title_selector, url_selector)
        {
            for snippet in document.select(&snippet_sel).take(max_results) {
                let title = snippet
                    .select(&title_sel)
                    .next()
                    .map(|e| e.text().collect::<String>())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let link = snippet
                    .select(&url_sel)
                    .next()
                    .and_then(|e| e.value().attr("href"))
                    .unwrap_or("")
                    .to_string();

                let desc = snippet.text().collect::<String>();
                let snippet_text = desc.trim().to_string();

                if !title.is_empty() && !link.is_empty() {
                    results.push(SearchResult {
                        title,
                        url: link,
                        snippet: snippet_text,
                        engine: "Brave".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_qwant(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>> {
        let url = format!(
            "https://www.qwant.com/?q={}&t=web",
            urlencoding::encode(query)
        );

        let response = self.client.get(&url).send().await?;
        let html = response.text().await?;

        let document = Html::parse_document(&html);
        let result_selector = Selector::parse("[data-testid=\"webResult\"]").ok();
        let link_selector = Selector::parse("a[href]").ok();

        let mut results = Vec::new();

        if let (Some(result_sel), Some(link_sel)) = (result_selector, link_selector) {
            for result in document.select(&result_sel).take(max_results) {
                let link = result
                    .select(&link_sel)
                    .next()
                    .and_then(|e| e.value().attr("href"))
                    .unwrap_or("")
                    .to_string();

                let title = result
                    .select(&link_sel)
                    .next()
                    .map(|e| e.text().collect::<String>())
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let snippet = result.text().collect::<String>();
                let snippet_text = snippet.trim().to_string();

                if !title.is_empty() && !link.is_empty() {
                    results.push(SearchResult {
                        title,
                        url: link,
                        snippet: snippet_text,
                        engine: "Qwant".to_string(),
                    });
                }
            }
        }

        Ok(results)
    }
}

impl Default for MultiEngineSearch {
    fn default() -> Self {
        Self::new().expect("Failed to create MultiEngineSearch")
    }
}

fn normalize_ddg_link(raw_link: &str) -> String {
    if raw_link.trim().is_empty() {
        return String::new();
    }

    if let Ok(parsed) = url::Url::parse(raw_link) {
        for (key, value) in parsed.query_pairs() {
            if key == "uddg" {
                return value.into_owned();
            }
        }
        return raw_link.to_string();
    }

    raw_link.to_string()
}

#[derive(Debug, Deserialize)]
struct SearXngResponse {
    results: Vec<SearXngResult>,
}

#[derive(Debug, Deserialize)]
struct SearXngResult {
    title: Option<String>,
    url: Option<String>,
    content: Option<String>,
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
    async fn test_multi_engine_search() {
        let searcher = MultiEngineSearch::new().expect("searcher");
        let results = searcher.search("rust programming language", 5).await.expect("results");
        assert!(!results.is_empty());
    }
}
