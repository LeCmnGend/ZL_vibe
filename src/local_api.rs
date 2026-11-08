//! Local HTTP REST API and Auto-Reply rules (#35).
//!
//! Enables local automation scripts and integrations
//! to query threads/messages, search local history, send or schedule messages, configure auto-reply
//! rules, and export/sync conversation data over `127.0.0.1`.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use subtle::ConstantTimeEq;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

use crate::chat;
use crate::scheduler;
use crate::session::Session;
use crate::store::{self, MessageStore};

pub const AUTOREPLY_FILE: &str = "autoreply_rules.json";
pub const DEFAULT_API_PORT: u16 = 17890;

fn local_api_origin_allowed(origin: &str, port: u16) -> bool {
    origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}")
}

fn bearer_token_matches(headers: &str, expected: &str) -> bool {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if !name.trim().eq_ignore_ascii_case("authorization") {
            return None;
        }
        let (scheme, token) = value.trim().split_once(' ')?;
        scheme
            .eq_ignore_ascii_case("bearer")
            .then(|| bool::from(token.trim().as_bytes().ct_eq(expected.as_bytes())))
    }) == Some(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutoReplyRule {
    pub id: String,
    pub keyword: String,
    /// `"contains"` (default), `"exact"`, or `"prefix"`.
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    pub reply_text: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional filter: `"all"`, `"direct"`, or `"group"`.
    #[serde(default = "default_scope")]
    pub scope: String,
}

fn default_match_mode() -> String {
    "contains".into()
}

fn default_true() -> bool {
    true
}

fn default_scope() -> String {
    "all".into()
}

pub fn load_autoreply_rules() -> Vec<AutoReplyRule> {
    std::fs::read_to_string(AUTOREPLY_FILE)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<AutoReplyRule>>(&s).ok())
        .unwrap_or_default()
}

pub fn save_autoreply_rules(rules: &[AutoReplyRule]) -> Result<()> {
    let text = serde_json::to_string_pretty(rules)?;
    crate::data_dir::write_private_atomic(std::path::Path::new(AUTOREPLY_FILE), text.as_bytes())?;
    Ok(())
}

/// Match an incoming message against active auto-reply rules.
pub fn match_autoreply_rule(
    rules: &[AutoReplyRule],
    incoming_text: &str,
    is_group: bool,
) -> Option<AutoReplyRule> {
    let msg_lower = incoming_text.trim().to_lowercase();
    if msg_lower.is_empty() {
        return None;
    }
    for rule in rules {
        if !rule.enabled {
            continue;
        }
        let kw = rule.keyword.trim().to_lowercase();
        if kw.is_empty() || rule.reply_text.trim().is_empty() {
            continue;
        }
        match rule.scope.as_str() {
            "direct" if is_group => continue,
            "group" if !is_group => continue,
            _ => {}
        }
        let matched = match rule.match_mode.as_str() {
            "exact" => msg_lower == kw,
            "prefix" => msg_lower.starts_with(&kw),
            _ => msg_lower.contains(&kw),
        };
        if matched {
            return Some(rule.clone());
        }
    }
    None
}

async fn execute_api_action(
    session: &Arc<Mutex<Session>>,
    name: &str,
    args: &Value,
    db_path: &str,
) -> (bool, Value) {
    match name {
        "zalo_status" => {
            let s = session.lock().await;
            let logged_in = s.login.is_some() && s.zpw_sek.is_some();
            let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or("");
            let display_name = s
                .login
                .as_ref()
                .map(|l| l.display_name.as_str())
                .unwrap_or("");
            let thread_count = MessageStore::open(db_path)
                .and_then(|st| st.all_threads())
                .map(|v| v.len())
                .unwrap_or(0);
            let scheduled_count = scheduler::list_pending_scheduled(db_path, None)
                .map(|v| v.len())
                .unwrap_or(0);
            (
                false,
                json!({
                    "ok": true,
                    "app": "ctdozlite",
                    "version": env!("ZL_VIBE_DISPLAY_VERSION"),
                    "logged_in": logged_in,
                    "uin": uin,
                    "display_name": display_name,
                    "threads_in_db": thread_count,
                    "pending_scheduled": scheduled_count,
                }),
            )
        }
        "zalo_list_threads" => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500) as usize;
            let mut threads = chat::load_threads_cache();
            store::load_history_into_threads(db_path, &mut threads, 5);
            let items: Vec<Value> = threads
                .into_iter()
                .take(limit)
                .map(|t| {
                    json!({
                        "id": t.id,
                        "name": t.name,
                        "is_group": t.is_group,
                        "preview": t.preview,
                        "time": t.time,
                        "message_count": t.messages.len(),
                    })
                })
                .collect();
            (false, json!({ "threads": items }))
        }
        "zalo_get_messages" => {
            let Some(tid) = args.get("thread_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: thread_id" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(500) as usize;
            match MessageStore::open(db_path).and_then(|st| st.messages_for(tid, limit)) {
                Ok(msgs) => (false, json!({ "thread_id": tid, "messages": msgs })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_search_messages" => {
            let Some(q) = args.get("query").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: query" }));
            };
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(200) as usize;
            match MessageStore::open(db_path).and_then(|st| st.search_messages(q, limit)) {
                Ok(hits) => (false, json!({ "query": q, "results": hits })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_send_message" => {
            let Some(to_id) = args.get("to_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: to_id" }));
            };
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: text" }));
            };
            let is_group = args.get("is_group").and_then(Value::as_bool).unwrap_or(false);
            let s = session.lock().await.clone();
            let res = if is_group {
                chat::send_group_message(&s, to_id, text).await
            } else {
                chat::send_message(&s, to_id, text).await
            };
            match res {
                Ok(resp) => (false, json!({ "ok": true, "to_id": to_id, "response": resp })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_schedule_message" => {
            let Some(to_id) = args.get("to_id").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: to_id" }));
            };
            let Some(text) = args.get("text").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: text" }));
            };
            let Some(when) = args.get("when").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: when" }));
            };
            let thread_name = args
                .get("thread_name")
                .and_then(Value::as_str)
                .unwrap_or(to_id);
            let is_group = args.get("is_group").and_then(Value::as_bool).unwrap_or(false);
            let now_ts = crate::login::unix_now();
            let deliver_at = match when.parse::<i64>() {
                Ok(ts) if ts > now_ts => ts,
                _ => match scheduler::parse_custom_schedule_input(when, now_ts) {
                    Ok(ts) => ts,
                    Err(e) => return (true, json!({ "error": e })),
                },
            };
            match scheduler::schedule_message(
                db_path,
                to_id,
                thread_name,
                is_group,
                text,
                deliver_at,
                now_ts,
            ) {
                Ok(item) => (false, json!({ "ok": true, "scheduled": item })),
                Err(e) => (true, json!({ "error": e.to_string() })),
            }
        }
        "zalo_configure_autoreply" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("list");
            let mut rules = load_autoreply_rules();
            match action {
                "list" => (false, json!({ "rules": rules })),
                "clear" => {
                    rules.clear();
                    let _ = save_autoreply_rules(&rules);
                    (false, json!({ "ok": true, "rules": rules }))
                }
                "add" => {
                    let kw = args
                        .get("keyword")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    let reply = args
                        .get("reply_text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim();
                    if kw.is_empty() || reply.is_empty() {
                        return (
                            true,
                            json!({ "error": "Cần cung cấp cả keyword và reply_text" }),
                        );
                    }
                    let mode = args
                        .get("match_mode")
                        .and_then(Value::as_str)
                        .unwrap_or("contains")
                        .to_string();
                    let rule = AutoReplyRule {
                        id: format!("rule_{}", rules.len() + 1),
                        keyword: kw.to_string(),
                        match_mode: mode,
                        reply_text: reply.to_string(),
                        enabled: true,
                        scope: "all".into(),
                    };
                    rules.push(rule);
                    if let Err(e) = save_autoreply_rules(&rules) {
                        return (true, json!({ "error": e.to_string() }));
                    }
                    (false, json!({ "ok": true, "rules": rules }))
                }
                _ => (true, json!({ "error": format!("Unsupported action: {action}") })),
            }
        }
        "zalo_list_friends" => {
            let s = session.lock().await.clone();
            let send2me_id = s.login.as_ref().map(|l| l.send2me_id.clone());
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(100)
                .clamp(1, 500) as usize;
            let requested_offset = args
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            match chat::get_friends(&s).await {
                Ok(threads) => {
                    // get_friends also merges groups and the "Cloud của tôi" thread; keep people only.
                    let items: Vec<Value> = threads
                        .into_iter()
                        .filter(|t| !t.is_group && Some(&t.id) != send2me_id.as_ref())
                        .map(|t| json!({ "id": t.id, "name": t.name }))
                        .collect();
                    let total = items.len();
                    let offset = requested_offset.min(total as u64) as usize;
                    let end = offset.saturating_add(limit).min(total);
                    (
                        false,
                        json!({
                            "friends": &items[offset..end],
                            "total": total,
                            "limit": limit,
                            "offset": offset,
                            "has_more": end < total
                        }),
                    )
                }
                Err(e) => (true, json!({ "error": format!("{e:#}") })),
            }
        }
        "zalo_find_user_by_phone" => {
            let Some(phone) = args.get("phone").and_then(Value::as_str) else {
                return (true, json!({ "error": "Missing required argument: phone" }));
            };
            let s = session.lock().await.clone();
            match chat::find_user_by_phone(&s, phone).await {
                Ok(user) => (false, json!(user)),
                Err(e) => (true, json!({ "error": format!("{e:#}") })),
            }
        }
        _ => (true, json!({ "error": format!("Unknown tool: {name}") })),
    }
}

/// Handle a REST API request and return `(status_code, json_body)`.
pub async fn handle_rest_request(
    session: &Arc<Mutex<Session>>,
    method: &str,
    path_and_query: &str,
    body: &str,
    db_path: &str,
) -> (u16, Value) {
    let (path, query_str) = path_and_query
        .split_once('?')
        .unwrap_or((path_and_query, ""));
    let query_params: HashMap<String, String> = query_str
        .split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((url_decode(k), url_decode(v)))
        })
        .collect();
    if let Some(r) =
        crate::api_hub_ext::route(session, method, path, &query_params, body, db_path).await
    {
        return r;
    }

    match (method, path) {
        ("GET", "/api/v1/status") | ("GET", "/health") => {
            let (_, val) = execute_api_action(session, "zalo_status", &json!({}), db_path).await;
            (200, val)
        }
        ("GET", "/api/v1/threads") => {
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(100);
            let (_, val) = execute_api_action(
                session,
                "zalo_list_threads",
                &json!({ "limit": limit }),
                db_path,
            )
            .await;
            (200, val)
        }
        ("GET", p) if p.starts_with("/api/v1/threads/") && p.ends_with("/messages") => {
            let tid = p
                .trim_start_matches("/api/v1/threads/")
                .trim_end_matches("/messages")
                .trim_matches('/');
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(100);
            let (err, val) = execute_api_action(
                session,
                "zalo_get_messages",
                &json!({ "thread_id": tid, "limit": limit }),
                db_path,
            )
            .await;
            (if err { 400 } else { 200 }, val)
        }
        ("GET", "/api/v1/search") => {
            let q = query_params.get("q").cloned().unwrap_or_default();
            let limit = query_params
                .get("limit")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(50);
            let (err, val) = execute_api_action(
                session,
                "zalo_search_messages",
                &json!({ "query": q, "limit": limit }),
                db_path,
            )
            .await;
            (if err { 400 } else { 200 }, val)
        }
        ("POST", "/api/v1/messages/send") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let (err, val) =
                execute_api_action(session, "zalo_send_message", &payload, db_path).await;
            (if err { 400 } else { 200 }, val)
        }
        ("POST", "/api/v1/messages/schedule") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            let mut normalized = payload.clone();
            if normalized.get("when").is_none() {
                if let Some(ts) = normalized.get("deliver_at") {
                    normalized["when"] = json!(ts.to_string());
                }
            }
            let (err, val) =
                execute_api_action(session, "zalo_schedule_message", &normalized, db_path).await;
            (if err { 400 } else { 200 }, val)
        }
        ("GET", "/api/v1/scheduled") => {
            let tid = query_params.get("thread_id").map(String::as_str);
            match scheduler::list_pending_scheduled(db_path, tid) {
                Ok(list) => (200, json!({ "ok": true, "scheduled": list })),
                Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
            }
        }
        ("DELETE", p) if p.starts_with("/api/v1/scheduled/") => {
            let id_str = p.trim_start_matches("/api/v1/scheduled/").trim();
            let Ok(id) = id_str.parse::<i64>() else {
                return (400, json!({ "ok": false, "error": "Invalid scheduled id" }));
            };
            match scheduler::cancel_scheduled_message(db_path, id) {
                Ok(cancelled) => (200, json!({ "ok": true, "cancelled": cancelled })),
                Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
            }
        }
        ("GET", "/api/v1/autoreply") => (200, json!({ "ok": true, "rules": load_autoreply_rules() })),
        ("POST", "/api/v1/autoreply") => {
            let payload: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
            if let Some(arr) = payload.get("rules").and_then(Value::as_array) {
                let rules: Vec<AutoReplyRule> = arr
                    .iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect();
                match save_autoreply_rules(&rules) {
                    Ok(()) => (200, json!({ "ok": true, "rules": rules })),
                    Err(e) => (500, json!({ "ok": false, "error": e.to_string() })),
                }
            } else {
                let (err, val) =
                    execute_api_action(session, "zalo_configure_autoreply", &payload, db_path).await;
                (if err { 400 } else { 200 }, val)
            }
        }
        ("GET", "/api/v1/export") => {
            let mut threads = chat::load_threads_cache();
            store::load_history_into_threads(db_path, &mut threads, 1000);
            let scheduled = scheduler::list_pending_scheduled(db_path, None).unwrap_or_default();
            let autoreply = load_autoreply_rules();
            (
                200,
                json!({
                    "ok": true,
                    "exported_at": crate::login::unix_now(),
                    "version": env!("ZL_VIBE_DISPLAY_VERSION"),
                    "threads": threads,
                    "scheduled_messages": scheduled,
                    "autoreply_rules": autoreply,
                }),
            )
        }
        ("GET", "/api/v1/friends") => {
            let limit = query_params
                .get("limit")
                .and_then(|value| value.parse::<u64>().ok());
            let offset = query_params
                .get("offset")
                .and_then(|value| value.parse::<u64>().ok());
            let args = json!({ "limit": limit, "offset": offset });
            let (err, val) = execute_api_action(session, "zalo_list_friends", &args, db_path).await;
            (if err { 502 } else { 200 }, val)
        }
        ("GET", "/api/v1/users/lookup") => {
            let phone = query_params.get("phone").map(|p| p.trim()).unwrap_or("");
            if phone.is_empty() {
                return (
                    400,
                    json!({ "error": "Missing required query parameter: phone" }),
                );
            }
            let (err, val) = execute_api_action(
                session,
                "zalo_find_user_by_phone",
                &json!({ "phone": phone }),
                db_path,
            )
            .await;
            (if err { 502 } else { 200 }, val)
        }
        _ => (
            404,
            json!({
                "ok": false,
                "error": format!("Endpoint not found: {method} {path}"),
                "available_endpoints": [
                    "GET /api/v1/status",
                    "GET /api/v1/threads",
                    "GET /api/v1/threads/{id}/messages",
                    "GET /api/v1/search?q=<keyword>",
                    "POST /api/v1/messages/send",
                    "POST /api/v1/messages/schedule",
                    "GET /api/v1/scheduled",
                    "DELETE /api/v1/scheduled/{id}",
                    "GET /api/v1/autoreply",
                    "POST /api/v1/autoreply",
                    "GET /api/v1/export",
                    "GET /api/v1/friends",
                    "GET /api/v1/users/lookup?phone=<number>",
                ]
            }),
        ),
    }
}

/// Spawn the local HTTP REST server on `127.0.0.1:<port>`.
pub async fn run_local_http_server(
    session: Arc<Mutex<Session>>,
    port: u16,
    db_path: String,
) -> Result<()> {
    let expected_token = std::env::var("CTDOZLITE_API_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("Chưa đặt CTDOZLITE_API_TOKEN để khởi động HTTP API cục bộ")
        })?;
    let bind_addr = format!("127.0.0.1:{port}");
    let listener = TcpListener::bind(&bind_addr).await?;
    crate::api_hub_ext::spawn_background(session.clone());

    loop {
        let (mut stream, _) = listener.accept().await?;
        let sess = session.clone();
        let db = db_path.clone();
        let tok = expected_token.clone();
        tokio::spawn(async move {
            let mut buf: Vec<u8> = Vec::with_capacity(8192);
            let mut chunk = [0u8; 8192];
            const MAX_REQ: usize = 2 * 1024 * 1024;
            let mut header_end: Option<usize> = None;
            let mut need_total = 0usize;
            loop {
                if header_end.is_none() {
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let he = pos + 4;
                        let head_txt = String::from_utf8_lossy(&buf[..pos]).to_ascii_lowercase();
                        let cl = head_txt
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        header_end = Some(he);
                        need_total = he + cl.min(MAX_REQ);
                    }
                }
                if header_end.is_some() && buf.len() >= need_total {
                    break;
                }
                if buf.len() >= MAX_REQ + 65536 {
                    break;
                }
                match stream.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            if buf.is_empty() {
                return;
            }
            let raw = String::from_utf8_lossy(&buf);
            let (head, body_part) = raw.split_once("\r\n\r\n").unwrap_or((&raw, ""));
            let mut lines = head.lines();
            let req_line = lines.next().unwrap_or("");
            let mut parts = req_line.split_whitespace();
            let method = parts.next().unwrap_or("GET");
            let path = parts.next().unwrap_or("/");
            let origin = head.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("origin")
                    .then(|| value.trim().to_string())
            });

            if origin
                .as_deref()
                .is_some_and(|value| !local_api_origin_allowed(value, port))
            {
                let body = json!({ "ok": false, "error": "Forbidden: origin not allowed" }).to_string();
                let response = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }

            if method == "OPTIONS" {
                let Some(origin) = origin.as_deref() else {
                    let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                    return;
                };
                let cors = format!(
                    "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: {origin}\r\nAccess-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nVary: Origin\r\nContent-Length: 0\r\n\r\n"
                );
                let _ = stream.write_all(cors.as_bytes()).await;
                return;
            }

            if !bearer_token_matches(head, &tok) {
                let body = json!({ "ok": false, "error": "Unauthorized: invalid or missing Bearer token" }).to_string();
                let response = format!(
                    "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Bearer\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                return;
            }

            let (status, json_val) =
                handle_rest_request(&sess, method, path, body_part, &db).await;
            let status_text = match status {
                200 => "200 OK",
                202 => "202 Accepted",
                400 => "400 Bad Request",
                404 => "404 Not Found",
                502 => "502 Bad Gateway",
                _ => "500 Internal Server Error",
            };
            let body_str = serde_json::to_string(&json_val).unwrap_or_else(|_| "{}".into());
            let cors_header = origin
                .as_deref()
                .map(|value| format!("Access-Control-Allow-Origin: {value}\r\nVary: Origin\r\n"))
                .unwrap_or_default();
            let resp = format!(
                "HTTP/1.1 {status_text}\r\nContent-Type: application/json; charset=utf-8\r\n{cors_header}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                body_str.len(),
                body_str
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let raw = s.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'+' {
            bytes.push(b' ');
            i += 1;
        } else if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(hex) = std::str::from_utf8(&raw[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    bytes.push(val);
                    i += 3;
                    continue;
                }
            }
            bytes.push(raw[i]);
            i += 1;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoreply_rules_match_contains_exact_prefix_and_scope() {
        let rules = vec![
            AutoReplyRule {
                id: "r1".into(),
                keyword: "báo giá".into(),
                match_mode: "contains".into(),
                reply_text: "Cảm ơn bạn, mình sẽ gửi báo giá ngay.".into(),
                enabled: true,
                scope: "direct".into(),
            },
            AutoReplyRule {
                id: "r2".into(),
                keyword: "/ping".into(),
                match_mode: "prefix".into(),
                reply_text: "Pong từ ctdozlite!".into(),
                enabled: true,
                scope: "all".into(),
            },
        ];

        assert!(match_autoreply_rule(&rules, "Cho mình xin BÁO GIÁ nhé", false).is_some());
        // Scope is `direct`, so group messages do not trigger r1
        assert!(match_autoreply_rule(&rules, "Cho mình xin báo giá nhé", true).is_none());
        // r2 has scope `all` and prefix `/ping`
        let hit = match_autoreply_rule(&rules, "/ping server", true).unwrap();
        assert_eq!(hit.id, "r2");
    }

    #[tokio::test]
    async fn rest_handlers_expose_search_and_schedule() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_file = std::env::temp_dir().join(format!("ctdozlite-rest-test-{nonce}.db"));
        let db_path = db_file.to_str().unwrap();

        {
            let mut store = MessageStore::open(db_path).unwrap();
            store
                .upsert_message(&store::StoredMessage {
                    msg_id: "m_rest_1".into(),
                    thread_id: "t_rest".into(),
                    sender_id: "u1".into(),
                    sender_name: "Alice".into(),
                    text: "Tích hợp REST API thành công".into(),
                    msg_type: "chat.text".into(),
                    ts: 123456,
                    is_outgoing: false,
                    raw_json: String::new(),
                })
                .unwrap();
        }

        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));

        // 1. REST search
        let (status, search_val) = handle_rest_request(
            &session,
            "GET",
            "/api/v1/search?q=REST",
            "",
            db_path,
        )
        .await;
        assert_eq!(status, 200);
        assert_eq!(search_val["results"].as_array().unwrap().len(), 1);

        // 2. REST schedule message
        let (sched_status, sched_val) = handle_rest_request(
            &session,
            "POST",
            "/api/v1/messages/schedule",
            &json!({
                "to_id": "t_rest",
                "thread_name": "Alice",
                "text": "Tin nhắn hẹn giờ qua REST API",
                "when": "+30m"
            })
            .to_string(),
            db_path,
        )
        .await;
        assert_eq!(sched_status, 200);
        assert_eq!(sched_val["ok"], true);

        let _ = std::fs::remove_file(&db_file);
    }

    #[tokio::test]
    async fn rest_users_lookup_returns_400_when_phone_missing() {
        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));
        for path in [
            "/api/v1/users/lookup",
            "/api/v1/users/lookup?phone=",
            "/api/v1/users/lookup?phone=%20",
        ] {
            let (status, val) = handle_rest_request(&session, "GET", path, "", ":memory:").await;
            assert_eq!(status, 400, "{path}");
            assert!(val["error"].as_str().unwrap().contains("phone"));
        }
    }

    #[tokio::test]
    async fn rest_friends_and_lookup_surface_errors_as_502_when_logged_out() {
        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));
        for path in ["/api/v1/friends", "/api/v1/users/lookup?phone=0912345678"] {
            let (status, val) = handle_rest_request(&session, "GET", path, "", ":memory:").await;
            // Routed to the chat call (not 404) and its error is not swallowed.
            assert_eq!(status, 502, "{path}");
            assert_eq!(val["error"], "chưa đăng nhập");
        }

        let (status, val) =
            handle_rest_request(&session, "GET", "/api/v1/nope", "", ":memory:").await;
        assert_eq!(status, 404);
        let endpoints = val["available_endpoints"].as_array().unwrap();
        assert!(endpoints.iter().any(|e| e == "GET /api/v1/friends"));
    }
}
