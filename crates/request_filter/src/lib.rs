mod filter;
mod signal;

use std::sync::OnceLock;

pub use filter::*;

static WH_URL: OnceLock<String> = OnceLock::new();

#[inline(always)]
pub fn init_wh_url(url: String) -> Result<(), String> {
    WH_URL.set(url)
}

pub(crate) fn wh_url() -> &'static str {
    WH_URL.get().map(|s| s.as_str()).unwrap_or_default()
}
