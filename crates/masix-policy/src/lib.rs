//! Masix Policy Engine
//!
//! Allowlist, denylist, and rate limiting

use masix_config::PolicyConfig;
use std::collections::HashSet;

#[derive(Clone)]
pub struct PolicyEngine {
    allowlist: HashSet<String>,
    denylist: HashSet<String>,
    rate_limit: Option<u32>,
}

impl PolicyEngine {
    pub fn new(config: Option<&PolicyConfig>) -> Self {
        let allowlist = config
            .and_then(|c| c.allowlist.clone())
            .map(|list| list.into_iter().collect())
            .unwrap_or_default();

        let denylist = config
            .and_then(|c| c.denylist.clone())
            .map(|list| list.into_iter().collect())
            .unwrap_or_default();

        let rate_limit = config.and_then(|c| c.rate_limit.as_ref().map(|r| r.messages_per_minute));

        Self {
            allowlist,
            denylist,
            rate_limit,
        }
    }

    pub fn is_allowed(&self, chat_id: &str) -> bool {
        if self.denylist.contains(chat_id) {
            return false;
        }

        if self.allowlist.is_empty() {
            return true;
        }

        self.allowlist.contains(chat_id)
    }

    pub fn check_rate_limit(&self, count: u32) -> bool {
        self.rate_limit.is_none_or(|limit| count <= limit)
    }
}
