//! PostgREST reverse proxy with TAP receipt validation.
//!
//! Every request must carry a valid `TAP-Receipt` header. The receipt is
//! validated (EIP-712 sig + staleness + authorized sender), persisted to
//! `tap_receipts`, and then the request is forwarded to the PostgREST backend.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};

use crate::{db, tap, AppState};

pub async fn handler(
    State(state): State<AppState>,
    req: Request<Body>,
) -> Result<Response<Body>, (StatusCode, String)> {
    // ── 1. Extract TAP-Receipt header ─────────────────────────────────────────
    let tap_header = req
        .headers()
        .get("tap-receipt")
        .ok_or_else(|| (StatusCode::PAYMENT_REQUIRED, "TAP-Receipt header required".into()))?;

    let header_str = tap_header
        .to_str()
        .map_err(|_| (StatusCode::BAD_REQUEST, "TAP-Receipt is not valid UTF-8".into()))?;

    // ── 2. Validate receipt ───────────────────────────────────────────────────
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let validated = tap::validate_receipt(
        header_str,
        state.domain_sep,
        &state.config.tap.authorized_senders,
        state.config.tap.data_service_address,
        state.config.indexer.service_provider_address,
        state.config.tap.max_receipt_age_ns,
        now_ns,
    )
    .map_err(|e| (StatusCode::PAYMENT_REQUIRED, e.to_string()))?;

    // ── 3. Persist receipt ────────────────────────────────────────────────────
    db::insert_receipt(&state.pool, &validated)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // ── 4. Proxy to PostgREST ─────────────────────────────────────────────────
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    let backend_url = format!(
        "{}{}",
        state.config.backend.postgrest_url.trim_end_matches('/'),
        path_and_query
    );

    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    // Forward Content-Type if present (for POST/PATCH).
    let mut builder = state.http_client.request(method, &backend_url);
    if let Some(ct) = req.headers().get("content-type") {
        if let Ok(ct_str) = ct.to_str() {
            builder = builder.header("content-type", ct_str);
        }
    }

    let body_bytes = axum::body::to_bytes(req.into_body(), 4 * 1024 * 1024)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes);
    }

    let resp = builder
        .send()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    let status = StatusCode::from_u16(resp.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();

    let resp_bytes = resp
        .bytes()
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))?;

    Ok(Response::builder()
        .status(status)
        .header("content-type", content_type)
        .body(Body::from(resp_bytes))
        .unwrap())
}
