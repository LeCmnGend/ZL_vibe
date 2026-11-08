//! Additional Hub actions backed by the existing chat protocol.
use std::{collections::HashMap, sync::Arc};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use crate::{api_hub_ext::{is_live, msg_from, sync_and_persist_session}, chat, session::Session};

fn text<'a>(p: &'a Value, key: &str) -> &'a str { p.get(key).and_then(Value::as_str).unwrap_or("") }
fn response(r: anyhow::Result<Value>) -> (u16, Value) {
    match r { Ok(v) => (200, json!({"ok": true, "response": v})), Err(e) => (502, json!({"ok": false, "error": e.to_string()})) }
}
fn known(method: &str, path: &str) -> bool {
    matches!((method, path), ("POST", "/api/v1/links/parse" | "/api/v1/messages/send-link" | "/api/v1/messages/forward" | "/api/v1/messages/delete" | "/api/v1/conversations/action" | "/api/v1/friends/action"))
        || (method == "GET" && path.starts_with("/api/v1/groups/") && path.ends_with("/history"))
}

async fn send_link(session: &Arc<Mutex<Session>>, s: &Session, p: &Value) -> (u16, Value) {
    let url = p.get("url").or_else(|| p.get("link")).and_then(Value::as_str).unwrap_or("");
    let to = text(p, "to_id");
    let mut meta = chat::LinkMetadata { url: url.into(), title: text(p, "title").into(), desc: text(p, "desc").into(), thumb: text(p, "thumb").into(), src: text(p, "src").into() };
    if meta.title.is_empty() {
        match chat::parse_link_metadata(s, url).await {
            Ok(m) => { if meta.title.is_empty() { meta.title = m.title; } if meta.desc.is_empty() { meta.desc = m.desc; } if meta.thumb.is_empty() { meta.thumb = m.thumb; } if meta.src.is_empty() { meta.src = m.src; } },
            Err(e) => return response(Err(e)),
        }
    }
    let r = chat::send_link_message(s, to, text(p, "text"), &meta, p.get("is_group").and_then(Value::as_bool).unwrap_or(false)).await;
    sync_and_persist_session(session).await;
    match r { Ok(v) => (200, json!({"ok": true, "to_id": to, "response": v})), Err(e) => response(Err(e)) }
}

fn pick_msg_id(v: &Value, fallback: &str) -> String {
    v.get("data").and_then(|d| d.get("msgId")).or_else(|| v.get("msgId"))
        .and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())))
        .filter(|s| !s.is_empty()).unwrap_or_else(|| fallback.to_string())
}

async fn forward(session: &Arc<Mutex<Session>>, s: &Session, p: &Value) -> (u16, Value) {
    let mut targets: Vec<&str> = p.get("targets").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if targets.is_empty() && !text(p, "to_id").is_empty() { targets.push(text(p, "to_id")); }
    let m = msg_from(p);
    let group = p.get("is_group").and_then(Value::as_bool).unwrap_or(false);
    let (mut ok, mut errors) = (Vec::new(), Vec::new());
    for target in targets {
        match chat::forward_message(s, target, group, &m).await {
            Ok(v) => ok.push(json!({"to_id": target, "msgId": pick_msg_id(&v, &m.id), "response": v})),
            Err(e) => errors.push(json!({"to_id": target, "error": e.to_string()})),
        }
    }
    sync_and_persist_session(session).await;
    let all_failed = ok.is_empty() && !errors.is_empty();
    (if all_failed { 502 } else { 200 }, json!({"ok": !all_failed, "success": ok, "errors": errors}))
}

async fn action(session: &Arc<Mutex<Session>>, s: &Session, path: &str, p: &Value) -> (u16, Value) {
    let action = text(p, "action");
    let id = if path.ends_with("friends/action") { p.get("to_id").or_else(|| p.get("uid")) } else { p.get("thread_id").or_else(|| p.get("to_id")) }.and_then(Value::as_str).unwrap_or("");
    let group = p.get("is_group").and_then(Value::as_bool).unwrap_or(false);
    let value = p.get("value").and_then(Value::as_bool).unwrap_or(true);
    let r = if path.ends_with("friends/action") {
        match action {
            "accept" => chat::accept_friend_request(s, id).await,
            "undo" => chat::undo_friend_request(s, id).await,
            "reject" => chat::reject_friend_request(s, id).await,
            "request" => chat::send_friend_request(s, id, text(p, "msg")).await,
            _ => return (400, json!({"ok": false, "error": "unknown action"})),
        }
    } else {
        match action {
            "pin" => chat::update_pinned_conversation(s, id, group, value).await,
            "mute" => chat::set_mute_conversation(s, id, group, p.get("duration").and_then(Value::as_i64).unwrap_or(-1), value).await,
            "unread" => chat::set_unread_mark(s, id, group, value).await,
            "hide" => chat::set_hidden_conversation(s, id, group, value).await,
            _ => return (400, json!({"ok": false, "error": "unknown action"})),
        }
    };
    sync_and_persist_session(session).await;
    response(r)
}

pub async fn route(session: &Arc<Mutex<Session>>, method: &str, path: &str, q: &HashMap<String, String>, body: &str) -> Option<(u16, Value)> {
    if !known(method, path) { return None; }
    let s = session.lock().await.clone();
    if !is_live(&s) { return Some((401, json!({"ok": false, "error": "chưa đăng nhập"}))); }
    let p: Value = serde_json::from_str(body).unwrap_or_else(|_| json!({}));
    Some(match (method, path) {
        ("POST", "/api/v1/links/parse") => match chat::parse_link_metadata(&s, text(&p, "url")).await {
            Ok(m) => (200, json!({"ok": true, "data": {"url": m.url, "title": m.title, "desc": m.desc, "thumb": m.thumb, "src": m.src}})),
            Err(e) => response(Err(e)),
        },
        ("POST", "/api/v1/messages/send-link") => send_link(session, &s, &p).await,
        ("POST", "/api/v1/messages/forward") => forward(session, &s, &p).await,
        ("POST", "/api/v1/messages/delete") => {
            let r = chat::delete_message_for_me(&s, text(&p, "to_id"), p.get("is_group").and_then(Value::as_bool).unwrap_or(false), &msg_from(&p)).await;
            sync_and_persist_session(session).await;
            response(r)
        },
        ("GET", _) => {
            let gid = path.trim_start_matches("/api/v1/groups/").trim_end_matches("/history").trim_matches('/');
            // This endpoint only exposes a count; request the largest supported history by default.
            let count = q.get("limit").and_then(|n| n.parse().ok()).unwrap_or(u32::MAX);
            match chat::get_group_history_raw(&s, gid, count).await {
                Ok(v) => { let msgs = v.get("groupMsgs").or_else(|| v.get("data").and_then(|d| d.get("groupMsgs"))).cloned().unwrap_or(v); (200, json!({"ok": true, "group_id": gid, "groupMsgs": msgs})) },
                Err(e) => response(Err(e)),
            }
        },
        _ => action(session, &s, path, &p).await,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unauthenticated_and_unknown() {
        let session = Arc::new(Mutex::new(Session::new(false, false, false).unwrap()));
        let q = HashMap::new();
        for (method, path) in [("POST", "/api/v1/links/parse"), ("POST", "/api/v1/messages/send-link"), ("POST", "/api/v1/messages/forward"), ("POST", "/api/v1/messages/delete"), ("GET", "/api/v1/groups/1/history"), ("POST", "/api/v1/conversations/action"), ("POST", "/api/v1/friends/action")] {
            assert_eq!(route(&session, method, path, &q, "{}").await, Some((401, json!({"ok": false, "error": "chưa đăng nhập"}))));
        }
        assert_eq!(route(&session, "GET", "/unknown", &q, "{}").await, None);
        assert_eq!(pick_msg_id(&json!({"data": {"msgId": 98765}}), "old"), "98765");
        assert_eq!(pick_msg_id(&json!({"msgId": "m_new"}), "old"), "m_new");
        assert_eq!(pick_msg_id(&json!({}), "old"), "old");
    }
}
