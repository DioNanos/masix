//! Advanced search module for Masix
//!
//! Features:
//! - DNS-over-HTTPS (DoH) resolver bypassing ISP DNS
//! - Multi-engine web search aggregation
//! - Torrent proxy/mirror support
//! - Magnet extraction with Cloudflare bypass
//! - Local magnet cache

mod doh;
mod engines;
mod torrent;
mod cache;

pub use doh::DohResolver;
pub use engines::{MultiEngineSearch, SearchEngine, SearchResult};
pub use torrent::{TorrentSearch, TorrentMirror, MagnetExtractor};
pub use cache::MagnetCache;

pub const DEFAULT_SEARCH_TIMEOUT_SECS: u64 = 15;
pub const DEFAULT_TORRENT_TIMEOUT_SECS: u64 = 12;
pub const MAGNET_CACHE_MAX_AGE_HOURS: u64 = 72;
