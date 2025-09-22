mod config;
mod models;
mod worldmobile;

use axum::{
    error_handling::HandleErrorLayer,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::{collections::HashMap, time::Duration};
use tower::{timeout::TimeoutLayer, BoxError, ServiceBuilder};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{
    config::Settings,
    models::{Country, CountryListResponse, CountryQuery},
    worldmobile::{extract_countries, fetch_all_pages, WmClient, WmError},
};

#[derive(OpenApi)]
#[openapi(
    paths(get_countries, health),
    components(schemas(Country, CountryListResponse, CountryQuery)),
    tags((name = "worldmobile", description = "Proxy endpoints for World Mobile"))
)]
struct ApiDoc;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let settings = Settings::from_env();
    let client = WmClient::new(settings.clone());

    // Порядок шарів ВАЖЛИВИЙ:
    // 1) Timeout (може падати)
    // 2) Trace
    // 3) HandleError (зовнішній до фейлячих шарів — конвертує помилки у HTTP-відповідь)
    // 4) CORS (найзовнішній — для зручності)
    let middleware = ServiceBuilder::new()
        .layer(TimeoutLayer::new(Duration::from_secs(20))) // фейлячий шар
        .layer(TraceLayer::new_for_http())
        .layer(HandleErrorLayer::new(|e: BoxError| async move {
            if e.is::<tower::timeout::error::Elapsed>() {
                (StatusCode::REQUEST_TIMEOUT, "request timed out")
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("internal error: {e}"))
            }
        }))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .into_inner();

    let app = Router::new()
        .route("/health", get(health))
        .route("/countries", get(get_countries))
        .with_state(client)
        .layer(middleware)
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8000));
    tracing::info!("listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Healthcheck
#[utoipa::path(get, path = "/health", tag = "meta")]
pub async fn health(State(state): State<WmClient>) -> impl IntoResponse {
    let src = if state.settings.use_stub { "stub" } else { "worldmobile" };
    let json = serde_json::json!({
        "status": "ok",
        "base_url": state.settings.base_url,
        "use_stub": state.settings.use_stub,
        "fail_open": state.settings.fail_open,
        "source": src,
    });
    (StatusCode::OK, Json(json))
}

/// /countries з підтримкою fetch_all і fail-open fallback
#[utoipa::path(
    get,
    path = "/countries",
    tag = "worldmobile",
    params(CountryQuery),
    responses(
        (status = 200, description = "List of countries", body = CountryListResponse),
        (status = 4XX, description = "Client error"),
        (status = 5XX, description = "Server / upstream error"),
    )
)]
pub async fn get_countries(
    State(client): State<WmClient>,
    Query(q): Query<CountryQuery>,
) -> Result<Json<CountryListResponse>, (StatusCode, String)> {
    let mut params: HashMap<&str, String> = HashMap::new();
    if let Some(v) = q.country_code { params.insert("country_code", v); }
    if let Some(v) = q.scope { params.insert("scope", v); }
    if let Some(v) = q.esim_id { params.insert("esim_id", v); }

    // пробуємо отримати реальні сторінки
    let result = fetch_all_pages(&client, &params, q.fetch_all.unwrap_or(false), q.page_size).await;

    // формуємо сторінки та позначку, чи спрацював fail-open
    let (pages, used_fail_open) = match result {
        Ok(p) => (p, false),
        Err(e) => {
            if client.settings.fail_open {
                tracing::warn!("upstream failed, serving stub (fail-open): {}", e);
                let stub = serde_json::from_str::<serde_json::Value>(include_str!("./stub.json")).unwrap();
                (vec![stub], true)
            } else {
                return Err(map_err(e));
            }
        }
    };

    // парсимо всі сторінки
    let mut tmp: Vec<(String, Option<String>)> = Vec::new();
    for p in &pages {
        tmp.extend(extract_countries(p));
    }
    let mut dedup: std::collections::HashMap<String, Option<String>> = std::collections::HashMap::new();
    for (code, name) in tmp {
        dedup.entry(code).and_modify(|v| if v.is_none() { *v = name.clone(); }).or_insert(name);
    }
    let mut countries: Vec<Country> = dedup.into_iter().map(|(code, name)| Country { code, name }).collect();
    countries.sort_by(|a, b| a.code.cmp(&b.code));

    let src = if client.settings.use_stub {
        "stub"
    } else if used_fail_open {
        "fail_open_stub"
    } else {
        "worldmobile"
    }.to_string();

    let resp = CountryListResponse { count: countries.len(), countries, source: src };
    Ok(Json(resp))
}

fn map_err(err: WmError) -> (StatusCode, String) {
    match err {
        WmError::MissingToken => (StatusCode::INTERNAL_SERVER_ERROR, "WM_BEARER_TOKEN is not set".into()),
        WmError::Http(e) => (StatusCode::BAD_GATEWAY, format!("request error: {}", e)),
        WmError::Upstream { status, body } => (status, body),
        WmError::BadJson => (StatusCode::BAD_GATEWAY, "invalid JSON from upstream".into()),
    }
}
