//! WebSocket and job-control endpoints.
//!
//! - /ws for subscribing to job-scoped progress events.
//! - /cancel-job to request cancellation of a running job.

use actix_web::{get, post, HttpRequest, HttpResponse, web};
use actix_web_actors::ws;
use std::collections::HashMap;

use crate::utils::{self, get_sender};

/// WebSocket endpoint used to stream progress/events to the Flutter UI.
///
/// Query params:
/// - jobId or job_id: logical job identifier; messages are broadcast per job.
///
/// Behavior:
/// - Subscribes client to a per-job broadcast channel.
/// - Flushes buffered events for late subscribers, then streams live updates.
#[get("/ws")]
pub async fn websocket_upgrade_endpoint(
    req: HttpRequest,
    stream: web::Payload,
    query: web::Query<HashMap<String, String>>,
) -> Result<HttpResponse, actix_web::Error> {
    let job_id = query
        .get("jobId")
        .cloned()
        .or_else(|| query.get("job_id").cloned())
        .unwrap_or_else(|| "default".to_string());
    println!(
        "[WS] connect: job_id={}, peer={}",
        job_id,
        req
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unknown".into())
    );
    let rx = get_sender(&job_id).subscribe();
    let resp = ws::start(utils::WsSession { rx, job_id }, &req, stream);
    resp
}

/// Request cancellation of a background job. Emits a final Cancelled event.
#[post("/cancel-job")]
pub async fn cancel_background_job_endpoint(query: web::Query<HashMap<String, String>>) -> HttpResponse {
    let job_id = query.get("jobId").cloned().or_else(|| query.get("job_id").cloned());
    if let Some(job_id_value) = job_id {
        utils::cancel_job(&job_id_value);
        utils::emit_event(
            Some(&job_id_value),
            crate::models::Phase::Cancelled,
            "Job cancelled",
            None,
            None,
        );
        return HttpResponse::Ok().json(serde_json::json!({"ok": true, "message": "cancelled"}));
    }
    HttpResponse::BadRequest().body("missing jobId")
}
