use std::{
    collections::HashMap,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncWriteExt, process::Command, time::timeout};
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

const TASK_TTL: Duration = Duration::from_secs(3600); // 1 hour

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SkillRequest {
    #[serde(default)]
    args: Vec<String>,
    /// Only SKILL_* keys accepted
    #[serde(default)]
    env: HashMap<String, String>,
    stdin: Option<String>,
    /// Timeout in seconds (default 30, max 300)
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 { 30 }

#[derive(Serialize, Clone)]
#[serde(tag = "status", rename_all = "lowercase")]
enum SkillResponse {
    Ok    { stdout: String, stderr: String, exit_code: i32 },
    Error { stdout: String, stderr: String, exit_code: i32 },
    Pending { task_id: String },
}

type TaskStore = Arc<DashMap<String, (SkillResponse, Instant)>>;

#[derive(Clone)]
struct AppState {
    tasks: TaskStore,
    skill_token: Option<String>,
}

// ── Auth ─────────────────────────────────────────────────────────────────────

fn check_auth(headers: &HeaderMap, token: &Option<String>) -> bool {
    match token {
        None => true,
        Some(t) => headers
            .get("x-skill-token")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == t)
            .unwrap_or(false),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn healthz() -> &'static str { "ok" }

async fn run_skill(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<SkillRequest>,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.skill_token) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"unauthorized"}))).into_response();
    }

    // Validate env keys
    for key in req.env.keys() {
        if !key.starts_with("SKILL_") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("env key '{}' not allowed; only SKILL_* permitted", key)})),
            ).into_response();
        }
    }

    let secs = req.timeout.min(300);
    let binary = format!("/usr/local/bin/{}", name);
    let env = req.env.clone();
    let args = req.args.clone();
    let stdin_data = req.stdin.clone();

    // Async if timeout > 30s
    if secs > 30 {
        let task_id = Uuid::new_v4().to_string();
        let tasks = state.tasks.clone();
        let tid = task_id.clone();
        tokio::spawn(async move {
            let result = exec(&binary, &args, &env, stdin_data, secs).await;
            tasks.insert(tid, (result, Instant::now()));
        });
        return (StatusCode::ACCEPTED, Json(SkillResponse::Pending { task_id })).into_response();
    }

    let result = exec(&binary, &args, &env, stdin_data, secs).await;
    (StatusCode::OK, Json(result)).into_response()
}

async fn poll_task(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !check_auth(&headers, &state.skill_token) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error":"unauthorized"}))).into_response();
    }
    match state.tasks.get(&id) {
        Some(r) if r.1.elapsed() < TASK_TTL => (StatusCode::OK, Json(r.0.clone())).into_response(),
        Some(_) => { state.tasks.remove(&id); (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"task expired"}))).into_response() }
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"task not found"}))).into_response(),
    }
}

// ── Core exec ────────────────────────────────────────────────────────────────

async fn exec(
    binary: &str,
    args: &[String],
    env: &HashMap<String, String>,
    stdin_data: Option<String>,
    secs: u64,
) -> SkillResponse {
    let mut cmd = Command::new(binary);
    cmd.args(args)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin_data.is_some() { Stdio::piped() } else { Stdio::null() });

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return SkillResponse::Error {
            stdout: String::new(),
            stderr: e.to_string(),
            exit_code: -1,
        },
    };

    if let (Some(data), Some(mut stdin)) = (stdin_data, child.stdin.take()) {
        let _ = stdin.write_all(data.as_bytes()).await;
    }

    match timeout(Duration::from_secs(secs), child.wait_with_output()).await {
        Ok(Ok(out)) => {
            let code = out.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if code == 0 {
                SkillResponse::Ok { stdout, stderr, exit_code: code }
            } else {
                SkillResponse::Error { stdout, stderr, exit_code: code }
            }
        }
        Ok(Err(e)) => SkillResponse::Error { stdout: String::new(), stderr: e.to_string(), exit_code: -1 },
        Err(_) => SkillResponse::Error { stdout: String::new(), stderr: "timeout".into(), exit_code: -1 },
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let state = AppState {
        tasks: Arc::new(DashMap::new()),
        skill_token: std::env::var("SKILL_TOKEN").ok(),
    };

    // Background task reaper — evict expired entries every 10 minutes
    let reaper_tasks = state.tasks.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(600)).await;
            reaper_tasks.retain(|_, (_, ts)| ts.elapsed() < TASK_TTL);
        }
    });

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/skill/:name", post(run_skill))
        .route("/task/:id", get(poll_task))
        .layer(RequestBodyLimitLayer::new(1024 * 1024)) // 1 MB
        .with_state(state);

    let addr = "127.0.0.1:8080";
    println!("skill-sidecar listening on {addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn app() -> Router {
        let state = AppState { tasks: Arc::new(DashMap::new()), skill_token: None };
        Router::new()
            .route("/healthz", get(healthz))
            .route("/skill/:name", post(run_skill))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_healthz() {
        let res = app()
            .oneshot(Request::get("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_invalid_env_key_rejected() {
        let body = serde_json::json!({
            "args": [],
            "env": { "AWS_SECRET_ACCESS_KEY": "leak" }
        });
        let res = app()
            .oneshot(
                Request::post("/skill/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_skill_env_key_accepted() {
        let body = serde_json::json!({
            "args": ["hello"],
            "env": { "SKILL_FOO": "bar" }
        });
        let res = app()
            .oneshot(
                Request::post("/skill/echo")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        // echo exists on the system; env key should pass validation (not 400)
        assert_ne!(res.status(), StatusCode::BAD_REQUEST);
    }
}
