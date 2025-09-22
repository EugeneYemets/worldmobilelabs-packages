use crate::config::Settings;
use axum::http::StatusCode;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WmError {
    #[error("config: bearer token is missing")]
    MissingToken,
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("upstream error {status}: {body}")]
    Upstream { status: StatusCode, body: String },
    #[error("invalid json from upstream")]
    BadJson,
}

#[derive(Clone)]
pub struct WmClient {
    pub http: Client,
    pub settings: Settings,
}

impl WmClient {
    pub fn new(settings: Settings) -> Self {
        let http = Client::builder()
            .timeout(std::time::Duration::from_millis(settings.http_timeout_ms))
            .build()
            .expect("reqwest client");
        Self { http, settings }
    }

    pub async fn fetch_available_packages(
        &self,
        params: &HashMap<&str, String>,
    ) -> Result<Value, WmError> {
        if self.settings.use_stub {
            let v: Value = serde_json::from_str(include_str!("./stub.json")).expect("valid stub.json");
            return Ok(v);
        }
        let token = self.settings.bearer_token.clone().ok_or(WmError::MissingToken)?;
        let url = format!("{}/v1/esim-packages/available", self.settings.base_url.trim_end_matches('/'));

        let resp = self.http
            .get(&url)
            .query(&params)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).to_string();
            return Err(WmError::Upstream { status, body });
        }
        let val: Value = serde_json::from_slice(&bytes).map_err(|_| WmError::BadJson)?;
        Ok(val)
    }
}

/// універсальний парсер країн
pub fn extract_countries(payload: &Value) -> Vec<(String, Option<String>)> {
    fn push_pair(v: &mut Vec<(String, Option<String>)>, code: Option<&str>, name: Option<&str>) {
        if let Some(c) = code {
            if !c.trim().is_empty() {
                v.push((c.trim().to_string(), name.map(|n| n.trim().to_string())))
            }
        }
    }

    fn normalize_entry(entry: &Value, out: &mut Vec<(String, Option<String>)>) {
        if let Some(obj) = entry.as_object() {
            let code_keys = ["country_code","countryCode","countryCodeISO","isoCountry","country","code"];
            let name_keys = ["country_name","countryName","name","countryLabel","country"];
            let code = code_keys.iter().find_map(|k| obj.get(*k).and_then(|v| v.as_str()));
            let name = name_keys.iter().find_map(|k| obj.get(*k).and_then(|v| v.as_str()));
            push_pair(out, code, name);

            for list_key in ["countries","supportedCountries","availableCountries"] {
                if let Some(arr) = obj.get(list_key).and_then(|v| v.as_array()) {
                    for sub in arr {
                        if let Some(subobj) = sub.as_object() {
                            let scode = subobj.get("code").and_then(|v| v.as_str())
                                .or_else(|| subobj.get("country_code").and_then(|v| v.as_str()))
                                .or_else(|| subobj.get("countryCode").and_then(|v| v.as_str()))
                                .or_else(|| subobj.get("country").and_then(|v| v.as_str()));
                            let sname = subobj.get("name").and_then(|v| v.as_str())
                                .or_else(|| subobj.get("country_name").and_then(|v| v.as_str()))
                                .or_else(|| subobj.get("countryName").and_then(|v| v.as_str()));
                            push_pair(out, scode, sname);
                        }
                    }
                }
            }
        }
    }

    let mut pairs: Vec<(String, Option<String>)> = Vec::new();
    match payload {
        Value::Array(arr) => for item in arr { normalize_entry(item, &mut pairs); },
        Value::Object(_) => {
            for key in ["data","packages","items","results"] {
                if let Some(arr) = payload.get(key).and_then(|v| v.as_array()) {
                    for item in arr { normalize_entry(item, &mut pairs); }
                }
            }
            normalize_entry(payload, &mut pairs);
        }
        _ => {}
    }

    let mut map: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for (code, name) in pairs {
        map.entry(code).and_modify(|v| if v.is_none() { *v = name.clone(); }).or_insert(name);
    }
    let mut out: Vec<(String, Option<String>)> = map.into_iter().collect();
    out.sort_by(|a,b| a.0.cmp(&b.0));
    out
}

/* ==================== ПАГІНАЦІЯ ==================== */

fn next_page_number(payload: &Value, current_page: u32) -> Option<u32> {
    if let Some(m) = payload.get("meta").and_then(|m| m.as_object()) {
        let total_pages = m.get("total_pages").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let page = m.get("page").and_then(|v| v.as_u64()).unwrap_or(current_page as u64) as u32;
        if total_pages > 0 && page < total_pages { return Some(page + 1); }
    }
    if let Some(np) = payload.get("next_page").and_then(|v| v.as_u64()) { return Some(np as u32); }
    if let Some(next_url) = payload.get("links").and_then(|l| l.get("next")).and_then(|v| v.as_str()) {
        if let Some(idx) = next_url.find("page=") {
            let tail = &next_url[idx+5..];
            let end = tail.find('&').unwrap_or(tail.len());
            if let Ok(p) = tail[..end].parse::<u32>() { return Some(p); }
        }
    }
    None
}

fn next_token(payload: &Value) -> Option<String> {
    for k in ["next","nextToken","next_token","nextPageToken"] {
        if let Some(s) = payload.get(k).and_then(|v| v.as_str()) {
            if !s.is_empty() { return Some(s.to_string()); }
        }
    }
    if let Some(s) = payload.get("meta").and_then(|m| m.get("next_token")).and_then(|v| v.as_str()) {
        if !s.is_empty() { return Some(s.to_string()); }
    }
    if let Some(s) = payload.get("links").and_then(|l| l.get("next")).and_then(|v| v.as_str()) {
        if !s.is_empty() { return Some(s.to_string()); }
    }
    None
}

pub async fn fetch_all_pages(
    client: &WmClient,
    base_params: &HashMap<&str, String>,
    fetch_all: bool,
    page_size: Option<u32>,
) -> Result<Vec<Value>, WmError> {
    if client.settings.use_stub || !fetch_all {
        return Ok(vec![client.fetch_available_packages(base_params).await?]);
    }

    let mut pages = Vec::new();
    let mut params = base_params.clone();
    let mut page = 1u32;

    if let Some(ps) = page_size.or(Some(client.settings.default_page_size)) {
        params.insert("page", page.to_string());
        params.insert("page_size", ps.to_string());
    }

    for _ in 0..client.settings.max_pages {
        let payload = client.fetch_available_packages(&params).await?;
        pages.push(payload.clone());

        if let Some(tok) = next_token(&payload) { params.insert("next", tok); continue; }
        if let Some(np) = next_page_number(&payload, page) { page = np; params.insert("page", page.to_string()); continue; }

        break;
    }

    Ok(pages)
}
