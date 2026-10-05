//! API HTTP appelée par n8n, et tableau de bord.

use crate::model::{Case, Email};
use crate::pipeline::{Engines, finalize};
use crate::store::Store;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};
use std::sync::Arc;

const DASHBOARD: &str = include_str!("../static/dashboard.html");

pub struct AppState {
    pub engines: Engines,
    pub store: Store,
    pub samples: Value,
}

type Shared = Arc<AppState>;

struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        eprintln!("erreur : {:#}", self.0);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{:#}", self.0) })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        ApiError(e.into())
    }
}

type ApiResult<T> = Result<T, ApiError>;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/", get(|| async { Html(DASHBOARD) }))
        .route("/healthz", get(|| async { "ok" }))
        .route("/samples", get(samples))
        .route("/triage/rules", post(rules))
        .route("/triage/jev", post(jev))
        .route("/triage/claude", post(claude))
        .route("/triage/full", post(full))
        .route("/results", post(save))
        .route("/demo/alerts", post(alert_sink))
        .route("/api/results", get(results))
        .route("/api/stats", get(stats))
        .route("/api/alerts", get(alerts))
        .with_state(state)
}

async fn samples(State(s): State<Shared>) -> Json<Value> {
    Json(s.samples.clone())
}

async fn rules(State(s): State<Shared>, Json(mut case): Json<Case>) -> ApiResult<Json<Case>> {
    let seen = s.store.exists(&case.email.message_id).await?;
    s.engines.apply_rules(&mut case, seen);
    Ok(Json(case))
}

async fn jev(State(s): State<Shared>, Json(mut case): Json<Case>) -> Json<Case> {
    s.engines.apply_jev(&mut case).await;
    Json(case)
}

async fn claude(State(s): State<Shared>, Json(mut case): Json<Case>) -> Json<Case> {
    s.engines.apply_claude(&mut case).await;
    Json(case)
}

async fn full(State(s): State<Shared>, Json(email): Json<Email>) -> Json<Value> {
    Json(json!(s.engines.run_full(email).await))
}

async fn save(State(s): State<Shared>, Json(case): Json<Case>) -> ApiResult<Json<Value>> {
    let record = finalize(&case);
    let inserted = s.store.insert(&record).await?;
    let mut out = serde_json::to_value(&record)?;
    out["inserted"] = json!(inserted);
    Ok(Json(out))
}

/// Récepteur d'alertes du mode démo (remplacez ALERT_WEBHOOK_URL par Slack, Teams, ntfy…).
async fn alert_sink(State(s): State<Shared>, Json(payload): Json<Value>) -> ApiResult<StatusCode> {
    s.store.insert_alert(&payload).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn results(State(s): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(s.store.results().await?))
}

async fn stats(State(s): State<Shared>) -> ApiResult<Json<Value>> {
    let mut v = s.store.stats().await?;
    v["mode"] = json!(match s.engines.cfg.mode {
        crate::config::Mode::Demo => "demo",
        crate::config::Mode::Live => "live",
    });
    v["claude_model"] = json!(s.engines.cfg.claude_model);
    v["jev_threshold"] = json!(s.engines.cfg.jev_threshold);
    Ok(Json(v))
}

async fn alerts(State(s): State<Shared>) -> ApiResult<Json<Value>> {
    Ok(Json(s.store.alerts().await?))
}
