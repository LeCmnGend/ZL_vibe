//! PIN-gated access to server-hidden conversations.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

/// Fetch the server membership and verify its four-digit PIN without persisting the verifier.
pub async fn unlock(
    session: &crate::session::Session,
    pin: &mut Zeroizing<String>,
) -> Result<HashSet<String>> {
    ensure!(
        pin.len() == 4 && pin.bytes().all(|byte| byte.is_ascii_digit()),
        "PIN phải gồm 4 chữ số"
    );

    let mut private = session.clone();
    private.verbose = false;
    private.dump = false;
    private.dump_raw = false;
    let params = private.encoded_params(&json!({ "imei": private.imei }))?;
    let url = format!(
        "{}/api/hiddenconvers/get-all?{}&params={params}",
        private.domains.convers_api.trim_end_matches('/'),
        private.common_params(),
    );
    let response = private.get_sensitive(&url).await?;
    let mut response = private
        .maybe_decrypt(response, private.session_key_bytes())
        .await;
    let mut acknowledged = false;
    for _ in 0..=4 {
        if let Value::String(text) = &response {
            ensure!(
                text.len() <= 4 * 1024 * 1024,
                "Phản hồi hội thoại ẩn quá lớn"
            );
            response =
                serde_json::from_str(text).context("Không đọc được phản hồi hội thoại ẩn")?;
        }
        let Some(code_value) = response.get("error_code") else {
            break;
        };
        let code = code_value
            .as_i64()
            .or_else(|| code_value.as_str()?.parse().ok())
            .context("Mã phản hồi hội thoại ẩn không hợp lệ")?;
        ensure!(code == 0, "Máy chủ từ chối tải hội thoại ẩn ({code})");
        acknowledged = true;
        response = response
            .as_object_mut()
            .and_then(|object| object.remove("data"))
            .context("Phản hồi hội thoại ẩn thiếu dữ liệu")?;
    }
    ensure!(acknowledged, "Phản hồi hội thoại ẩn thiếu xác nhận máy chủ");

    let verifier = response
        .as_object_mut()
        .and_then(|object| object.remove("pin"))
        .unwrap_or(Value::Null);
    let verifier = match verifier {
        Value::String(value) => Zeroizing::new(value),
        Value::Null => Zeroizing::new(String::new()),
        _ => anyhow::bail!("Định dạng PIN máy chủ chưa được hỗ trợ"),
    };
    let mut verifier = verifier;
    ensure!(
        verifier.is_empty()
            || (verifier.len() == 32 && verifier.bytes().all(|byte| byte.is_ascii_hexdigit())),
        "Định dạng PIN máy chủ chưa được hỗ trợ"
    );

    let rows = response
        .get("threads")
        .and_then(Value::as_array)
        .context("Thiếu danh sách hội thoại ẩn")?;
    ensure!(rows.len() <= 20_000, "Danh sách hội thoại ẩn quá lớn");
    let mut ids = HashSet::with_capacity(rows.len());
    for row in rows {
        let id = row
            .get("thread_id")
            .and_then(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| value.as_u64().map(|id| id.to_string()))
            })
            .context("Thiếu định danh hội thoại ẩn")?;
        let group = match row
            .get("is_group")
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        {
            Some(0) => false,
            Some(1) => true,
            _ => anyhow::bail!("Thiếu loại hội thoại ẩn"),
        };
        ensure!(
            ids.insert(identity(&id, group)?),
            "Danh sách hội thoại ẩn bị trùng"
        );
    }
    ensure!(
        !verifier.is_empty() || ids.is_empty(),
        "Danh sách ẩn có dữ liệu nhưng máy chủ thiếu PIN"
    );
    if !verifier.is_empty() {
        verifier.make_ascii_lowercase();
        let mut digest = Zeroizing::new(crate::crypto::md5_hex(pin));
        digest.make_ascii_lowercase();
        let matches: bool = verifier.as_bytes().ct_eq(digest.as_bytes()).into();
        ensure!(matches, "PIN không đúng");
        digest.zeroize();
    }
    pin.zeroize();
    Ok(ids)
}

/// Normalize the server's direct/group identifiers for matching loaded threads.
pub fn identity(id: &str, group: bool) -> Result<String> {
    let mut clean = id.trim();
    clean = clean.strip_prefix("group_").unwrap_or(clean);
    clean = clean.strip_prefix("GROUP_").unwrap_or(clean);
    if group {
        clean = clean
            .strip_prefix('g')
            .or_else(|| clean.strip_prefix('G'))
            .unwrap_or(clean);
    } else {
        clean = clean
            .strip_prefix('u')
            .or_else(|| clean.strip_prefix('U'))
            .unwrap_or(clean);
    }
    ensure!(
        !clean.is_empty()
            && clean.len() <= 128
            && clean
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)),
        "Định danh hội thoại ẩn không hợp lệ"
    );
    Ok(format!("{}{clean}", if group { 'g' } else { 'u' }))
}

pub fn thread_identity(thread: &crate::chat::Thread) -> Option<String> {
    identity(&thread.id, thread.is_group).ok()
}

pub fn thread_from_identity(id: &str) -> Option<crate::chat::Thread> {
    let (group, raw) = match id.as_bytes().first().copied()? {
        b'g' => (true, &id[1..]),
        b'u' => (false, &id[1..]),
        _ => return None,
    };
    let canonical = identity(raw, group).ok()?;
    (canonical == id).then(|| crate::chat::Thread {
        id: if group {
            format!("group_{raw}")
        } else {
            raw.to_owned()
        },
        name: raw.to_owned(),
        is_group: group,
        ..Default::default()
    })
}
