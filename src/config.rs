use dotenvy::dotenv;
use std::env;

#[derive(Clone, Debug)]
pub struct Settings {
    pub base_url: String,
    pub bearer_token: Option<String>,
    pub use_stub: bool,
    pub http_timeout_ms: u64,
    pub max_pages: u32,
    pub default_page_size: u32,
    /// Якщо true — при збої апі повернемо stub (щоб UI не пустував)
    pub fail_open: bool,
}

impl Settings {
    pub fn from_env() -> Self {
        let _ = dotenv();

        let base_url = env::var("WM_BASE_URL")
            .unwrap_or_else(|_| "https://partnerapi.worldmobilelabs.com".to_string());
        let bearer_token = env::var("WM_BEARER_TOKEN").ok();
        let use_stub = env::var("WM_USE_STUB")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let http_timeout_ms = env::var("WM_HTTP_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15_000u64);
        let max_pages = env::var("WM_MAX_PAGES").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
        let default_page_size = env::var("WM_DEFAULT_PAGE_SIZE").ok().and_then(|v| v.parse().ok()).unwrap_or(100);
        let fail_open = env::var("WM_FAIL_OPEN")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(true); // за замовчуванням: вмикаємо fail-open

        Self {
            base_url,
            bearer_token,
            use_stub,
            http_timeout_ms,
            max_pages,
            default_page_size,
            fail_open,
        }
    }
}
