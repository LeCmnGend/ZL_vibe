//! Chat data: friend/conversation list + last messages, all over REST.
//!
//! Notes: `re-notes/02_chat_data.md`.

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::session::{obj, Session};

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    #[serde(default)]
    pub kind: crate::message_model::MessageKind,
    #[serde(default)]
    pub wire_ref: crate::message_model::WireMessageRef,
    #[serde(default)]
    pub unread_policy: crate::message_model::UnreadPolicy,
    #[serde(default)]
    pub delivery: crate::message_model::DeliveryState,
    pub id: String,
    pub sender: String,
    pub text: String,
    pub time: String,
    pub is_outgoing: bool,
    pub ts: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_cate_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker_type: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_sender: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reactions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaction_summaries: Vec<crate::reactions::ReactionSummary>,
    #[serde(default)]
    pub reaction_snapshot: bool,
    #[serde(default)]
    pub reaction_authoritative_snapshot: bool,
    #[serde(skip)]
    pub reaction_snapshot_fresh: bool,
    #[serde(default)]
    pub reaction_snapshot_provenance: crate::reactions::ReactionSnapshotProvenance,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub reaction_versions: std::collections::BTreeMap<String, crate::reactions::ReactionVersion>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub client_message_id: String,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub is_starred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_desc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_thumb: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_source: Option<Value>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub resolved_mention_uids: std::collections::BTreeMap<String, String>,
}

impl ChatMessage {
    pub fn merge_message_metadata(&mut self, incoming: &Self) -> bool {
        let previous_mentions = self.resolved_mention_uids.clone();
        self.resolved_mention_uids
            .extend(incoming.resolved_mention_uids.clone());
        let before = (
            self.kind,
            self.wire_ref.clone(),
            self.unread_policy.clone(),
            self.delivery,
            self.history_source.clone(),
        );
        if incoming.kind != crate::message_model::MessageKind::Unknown {
            self.kind = incoming.kind;
        }
        if !incoming.wire_ref.fields.is_empty() {
            let mut fields = self.wire_ref.fields.clone();
            fields.extend(incoming.wire_ref.fields.clone());
            self.wire_ref = crate::message_model::WireMessageRef::from_raw(&Value::Object(
                fields.into_iter().collect(),
            ));
            if incoming.unread_policy.params_ext_present {
                self.unread_policy = incoming.unread_policy.clone();
            }
        }
        if incoming.history_source.is_some() {
            self.history_source = incoming.history_source.clone();
        }
        self.delivery = self.delivery.merge(incoming.delivery);
        previous_mentions != self.resolved_mention_uids
            || before
                != (
                    self.kind,
                    self.wire_ref.clone(),
                    self.unread_policy.clone(),
                    self.delivery,
                    self.history_source.clone(),
                )
    }

    /// Keep aggregate counts and event replay cursors across cache/network merges.
    pub fn merge_reaction_metadata(&mut self, incoming: &Self) -> bool {
        let model_changed = self.merge_message_metadata(incoming);
        let previous_provenance = self.reaction_snapshot_provenance.clone();
        let before = (
            self.reaction_summaries.clone(),
            self.reactions.clone(),
            self.client_message_id.clone(),
            self.reaction_snapshot,
            self.reaction_authoritative_snapshot,
            self.reaction_versions.clone(),
        );
        if self.client_message_id.is_empty() {
            self.client_message_id = incoming.client_message_id.clone();
        }
        let accept_snapshot = incoming.reaction_authoritative_snapshot
            && (incoming.reaction_snapshot_fresh || !self.reaction_authoritative_snapshot)
            && self.reaction_snapshot_provenance.accepts_snapshot(
                &incoming.reaction_snapshot_provenance,
                &self.reaction_versions,
                incoming.reaction_snapshot_fresh,
            );

        if accept_snapshot {
            self.reaction_snapshot_provenance = incoming.reaction_snapshot_provenance.clone();
            self.reaction_summaries = incoming.reaction_summaries.clone();
            self.reaction_authoritative_snapshot = true;
            self.reaction_snapshot = true;
            for (user, version) in &mut self.reaction_versions {
                version.covered_events.append(&mut version.events);
                version.baseline = incoming
                    .reaction_summaries
                    .iter()
                    .filter_map(|s| Some((s.r_type?, *s.reactor_counts.get(user)?)))
                    .collect();
                version.ledger_initialized = true;
                version.snapshot_baseline = true;
            }
            for (user, incoming_version) in &incoming.reaction_versions {
                let covered = self
                    .reaction_versions
                    .get(user)
                    .map(|v| v.covered_events.clone())
                    .unwrap_or_default();
                let mut version = incoming_version.clone();
                version.covered_events.extend(covered);
                if incoming.reaction_snapshot_fresh {
                    version.covered_events.append(&mut version.events);
                    version.baseline = incoming
                        .reaction_summaries
                        .iter()
                        .filter_map(|s| Some((s.r_type?, *s.reactor_counts.get(user)?)))
                        .collect();
                    version.ledger_initialized = true;
                    version.snapshot_baseline = true;
                }
                self.reaction_versions.insert(user.clone(), version);
            }
        }
        if !self.reaction_authoritative_snapshot
            && !incoming.reaction_authoritative_snapshot
            && (incoming.reaction_snapshot || !incoming.reaction_summaries.is_empty())
            && self.reaction_versions.is_empty()
        {
            self.reaction_summaries = incoming.reaction_summaries.clone();
            self.reactions = crate::reactions::legacy_emojis(&self.reaction_summaries);
            self.reaction_snapshot = incoming.reaction_snapshot;
        }
        if !self.reaction_authoritative_snapshot
            && !incoming.reaction_authoritative_snapshot
            && !self.reaction_versions.is_empty()
            && (incoming.reaction_snapshot || !incoming.reaction_summaries.is_empty())
        {
            crate::reactions::merge_untracked_snapshot(
                &mut self.reaction_summaries,
                &incoming.reaction_summaries,
                &self.reaction_versions,
                incoming.reaction_snapshot && incoming.reaction_versions.is_empty(),
            );
        }
        for (user, version) in &incoming.reaction_versions {
            if accept_snapshot {
                continue;
            }
            if !self.reaction_versions.contains_key(user) {
                if self.reaction_authoritative_snapshot {
                    for event in &version.events {
                        if event.r_type >= -1 {
                            self.apply_reaction(event);
                        }
                    }
                    continue;
                }
                self.reaction_versions.insert(user.clone(), version.clone());
                if version.ledger_initialized {
                    version.rebuild(&mut self.reaction_summaries, user);
                }
            } else {
                for event in &version.events {
                    if event.r_type >= -1 {
                        self.apply_reaction(event);
                    }
                }
            }
        }
        if !self.reaction_summaries.is_empty()
            || incoming.reaction_snapshot
            || !incoming.reaction_versions.is_empty()
        {
            self.reactions = crate::reactions::legacy_emojis(&self.reaction_summaries);
        } else if self.reactions.is_empty()
            && self.reaction_versions.is_empty()
            && !self.reaction_snapshot
        {
            self.reactions = incoming.reactions.clone();
        }
        model_changed
            || previous_provenance != self.reaction_snapshot_provenance
            || before
                != (
                    self.reaction_summaries.clone(),
                    self.reactions.clone(),
                    self.client_message_id.clone(),
                    self.reaction_snapshot,
                    self.reaction_authoritative_snapshot,
                    self.reaction_versions.clone(),
                )
    }

    pub fn apply_reaction(&mut self, event: &crate::reactions::ReactionEvent) -> bool {
        if event.sender_id.is_empty() || event.r_type < -1 {
            return false;
        }
        if self.reaction_summaries.is_empty() && !self.reactions.is_empty() {
            self.reaction_summaries =
                crate::reactions::parse_reactions(&json!({"reactions":self.reactions}));
        }
        let changed = self
            .reaction_versions
            .entry(event.sender_id.clone())
            .or_default()
            .record(
                &mut self.reaction_summaries,
                event,
                self.reaction_authoritative_snapshot,
            );
        self.reactions = crate::reactions::legacy_emojis(&self.reaction_summaries);
        self.reaction_snapshot = true;
        changed
    }

    /// Repair cached media metadata without changing captions or signed URL bytes.
    pub fn hydrate_media_metadata(&mut self) -> bool {
        if self.text == "[Tin nhắn đã thu hồi]" {
            return false;
        }
        let before = (
            self.link_url.clone(),
            self.link_thumb.clone(),
            self.link_src.clone(),
        );
        let source_kind = self.history_source.as_ref().and_then(|source| {
            let kind = source
                .get("msgType")
                .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
            match kind {
                Some(2..=4) => Some("Hình ảnh Zalo"),
                Some(19) => Some("Video Zalo"),
                _ => None,
            }
        });
        if let Some(source) = &self.history_source {
            if let Some(attachment) = source
                .get("meta")
                .and_then(|m| m.get("attachsList"))
                .and_then(Value::as_array)
                .and_then(|a| a.first())
            {
                let (url, _, _, thumb, _) = extract_link_info(attachment);
                if self.link_url.is_none() {
                    self.link_url = url;
                }
                if self.link_thumb.is_none() {
                    self.link_thumb = thumb;
                }
            }
        }
        let label_kind = match self.text.trim() {
            "[Photo]" | "[Hình ảnh]" => Some("Hình ảnh Zalo"),
            "[Video]" => Some("Video Zalo"),
            _ => None,
        };
        if let Some(kind) = source_kind.or(label_kind) {
            self.link_src = Some(kind.into());
            if kind == "Hình ảnh Zalo" && self.link_thumb.is_none() {
                self.link_thumb = self.link_url.clone();
            }
        }
        before
            != (
                self.link_url.clone(),
                self.link_thumb.clone(),
                self.link_src.clone(),
            )
    }

    /// Fill independently missing card fields when an existing URL already exists.
    // The parser layer wires this helper in the dependent protocol change.
    #[allow(dead_code)]
    fn merge_media_metadata(&mut self, incoming: &Self) -> bool {
        if self.text == "[Tin nhắn đã thu hồi]" {
            return false;
        }
        let before = (
            self.link_url.clone(),
            self.link_title.clone(),
            self.link_desc.clone(),
            self.link_thumb.clone(),
            self.link_src.clone(),
        );
        if self.link_title.is_none() {
            self.link_title = incoming.link_title.clone();
        }
        if self.link_desc.is_none() {
            self.link_desc = incoming.link_desc.clone();
        }
        if self.link_thumb.is_none() {
            self.link_thumb = incoming.link_thumb.clone();
        }
        if self.link_src.is_none()
            || matches!(
                incoming.link_src.as_deref(),
                Some("Hình ảnh Zalo" | "Video Zalo")
            )
        {
            self.link_src = incoming.link_src.clone().or_else(|| self.link_src.clone());
        }
        self.hydrate_media_metadata();
        before
            != (
                self.link_url.clone(),
                self.link_title.clone(),
                self.link_desc.clone(),
                self.link_thumb.clone(),
                self.link_src.clone(),
            )
    }

    /// Compute the static PNG URL for a Zalo sticker (`compact-app-pc.b7bec208c4c8f73e09f8.js:7739583` & `18589292`).
    pub fn sticker_image_url(&self) -> Option<String> {
        let sid = self.sticker_id?;
        if sid == 0 {
            return None;
        }
        if self.sticker_cate_id == Some(0) && (1..=30).contains(&sid) {
            Some(format!(
                "https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png"
            ))
        } else {
            Some(format!(
                "https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={sid}&size=130"
            ))
        }
    }

    pub fn sticker_cache_key(&self) -> Option<String> {
        let sid = self.sticker_id?;
        if sid == 0 {
            return None;
        }
        let cid = self.sticker_cate_id.unwrap_or(0);
        Some(format!("stk_{cid}_{sid}"))
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Thread {
    #[serde(default)]
    pub presentation: SidebarPresentation,
    pub id: String,
    pub name: String,
    pub preview: String,
    pub time: String,
    pub avatar: Option<String>,
    pub is_group: bool,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub unread: crate::unread::UnreadState,
}

/// Durable sidebar metadata, including metadata-only startup snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SidebarPresentation {
    pub members: Vec<SidebarMember>,
    /// Server totalMember only; never inferred from message senders or avatar samples.
    pub member_count: Option<u64>,
    pub last_message_ts: i64,
    pub last_message_id: String,
    pub last_sender: String,
    pub last_sender_id: String,
    pub last_outgoing: bool,
    pub pinned: bool,
    pub muted: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SidebarMember {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarFontStyle {
    Regular,
    Semibold,
}

impl Thread {
    pub fn sidebar_font_style(&self) -> SidebarFontStyle {
        if self.unread.has_badge() {
            SidebarFontStyle::Semibold
        } else {
            SidebarFontStyle::Regular
        }
    }

    pub fn refresh_sidebar_presentation(&mut self) {
        if let Some(message) = self
            .messages
            .iter()
            .max_by_key(|m| crate::clock::normalize_ts_ms(m.ts))
        {
            let ts = crate::clock::normalize_ts_ms(message.ts);
            if ts >= crate::clock::normalize_ts_ms(self.presentation.last_message_ts) {
                let sender_id = sidebar_sender_id(message)
                    .or_else(|| {
                        (message.id == self.presentation.last_message_id
                            && ts
                                == crate::clock::normalize_ts_ms(self.presentation.last_message_ts))
                        .then(|| self.presentation.last_sender_id.clone())
                    })
                    .unwrap_or_default();
                let keep_resolved = message.id == self.presentation.last_message_id
                    && ts == crate::clock::normalize_ts_ms(self.presentation.last_message_ts)
                    && sender_id == self.presentation.last_sender_id
                    && !self.presentation.last_sender.is_empty()
                    && !self
                        .presentation
                        .last_sender
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                    && message.sender.bytes().all(|b| b.is_ascii_digit());
                self.presentation.last_message_ts = message.ts;
                self.presentation.last_message_id = message.id.clone();
                if !keep_resolved {
                    self.presentation.last_sender = message.sender.clone();
                }
                self.presentation.last_sender_id = sender_id;
                self.presentation.last_outgoing = message.is_outgoing;
            }
        }
    }

    pub fn sidebar_preview(&self) -> String {
        let latest = self
            .messages
            .iter()
            .max_by_key(|m| crate::clock::normalize_ts_ms(m.ts))
            .filter(|m| {
                crate::clock::normalize_ts_ms(m.ts)
                    > crate::clock::normalize_ts_ms(self.presentation.last_message_ts)
                    || self.presentation.last_sender.is_empty()
            });
        let (sender, outgoing) = latest
            .map(|m| (m.sender.as_str(), m.is_outgoing))
            .unwrap_or((
                &self.presentation.last_sender,
                self.presentation.last_outgoing,
            ));
        let sender = self
            .presentation
            .members
            .iter()
            .find(|member| member.id == sender && !member.name.trim().is_empty())
            .map(|member| member.name.as_str())
            .unwrap_or(sender);
        let text = self.preview.replace('\n', " ");
        if text.is_empty() {
            return text;
        }
        if outgoing {
            if text.starts_with("Bạn: ") {
                text
            } else {
                format!("Bạn: {text}")
            }
        } else if self.is_group
            && !sender.trim().is_empty()
            && !sender.trim().chars().all(|c| c.is_ascii_digit())
        {
            let prefix = format!("{}: ", sender.trim());
            if text.starts_with(&prefix) {
                text
            } else {
                format!("{prefix}{text}")
            }
        } else {
            text
        }
    }

    pub fn sidebar_activity_ts(&self) -> i64 {
        self.messages
            .iter()
            .map(|m| crate::clock::normalize_ts_ms(m.ts))
            .chain(std::iter::once(crate::clock::normalize_ts_ms(
                self.presentation.last_message_ts,
            )))
            .max()
            .unwrap_or(0)
    }

    pub fn sidebar_timestamp(&self, now_ms: i64) -> String {
        let ts = self.sidebar_activity_ts();
        if ts > 0 {
            crate::clock::sidebar_time_at(ts, now_ms)
        } else {
            self.time.clone()
        }
    }

    /// Parse only group roster metadata observed in the original getmg-v2 consumer.
    pub fn apply_group_sidebar_metadata(&mut self, info: &Value) {
        if let Some(count) = info
            .get("totalMember")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok()))
            .filter(|count| *count > 0)
        {
            self.presentation.member_count = Some(count);
        }
        let mut ids = Vec::new();
        if let Some(list) = info.get("memVerList").and_then(Value::as_array) {
            for item in list.iter().filter_map(Value::as_str) {
                let id = item.split('_').next().unwrap_or_default();
                if !id.is_empty() && !ids.iter().any(|s| s == id) {
                    ids.push(id.to_owned());
                }
            }
        } else if let Some(map) = info.get("memberVerMap").and_then(Value::as_object) {
            ids.extend(map.keys().cloned());
        }
        let mut leaders = Vec::new();
        if let Some(creator) = pick_str(info, &["creatorId"]) {
            leaders.push(creator);
        }
        if let Some(admins) = info.get("adminIds").and_then(Value::as_array) {
            leaders.extend(admins.iter().filter_map(Value::as_str).map(str::to_owned));
        }
        let mut ordered = Vec::new();
        for id in leaders.into_iter().chain(ids.iter().cloned()) {
            if ids.contains(&id) && !ordered.contains(&id) {
                ordered.push(id);
            }
        }
        ids = ordered;
        let top = info.get("topMember").and_then(Value::as_array);
        if let Some(top) = top {
            for profile in top {
                if let Some(id) = pick_str(profile, &["id", "userId", "uid"]) {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            }
        }
        if !ids.is_empty() {
            let previous = std::mem::take(&mut self.presentation.members);
            self.presentation.members =
                ids.into_iter()
                    .map(|id| {
                        let profile = top.and_then(|list| {
                            list.iter().find(|p| {
                                pick_str(p, &["id", "userId", "uid"]).as_ref() == Some(&id)
                            })
                        });
                        let mut member = previous.iter().find(|m| m.id == id).cloned().unwrap_or(
                            SidebarMember {
                                id,
                                ..Default::default()
                            },
                        );
                        if let Some(profile) = profile {
                            if let Some(name) =
                                pick_str(profile, &["displayName", "zaloName", "name"])
                            {
                                member.name = name;
                            }
                            if let Some(avatar) = pick_str(profile, &["avatar", "avt"]) {
                                member.avatar = Some(avatar);
                            }
                        }
                        member
                    })
                    .collect();
        }
    }
}

pub fn sidebar_sender_id(message: &ChatMessage) -> Option<String> {
    let raw = message.wire_ref.sender.as_ref().or_else(|| {
        message
            .history_source
            .as_ref()
            .and_then(|v| v.get("senderId"))
    })?;
    let id = match raw {
        Value::String(v) => v.clone(),
        Value::Number(v) if v.is_u64() => v.to_string(),
        _ => return None,
    };
    (!id.is_empty() && id != "0" && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// Return `true` if a thread name is empty or a client placeholder (`"Người dùng <id>"`,
/// `"Nhóm <id>"`, or a bare numeric ID). Real names such as `"Nhóm bạn thân"` are kept.
pub fn is_generic_thread_name(name: &str) -> bool {
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    let t = name.trim();
    // Only QTzalo's own placeholders ("Người dùng <id>", "Nhóm <id>") or a bare id: many real
    // group names start with "Nhóm " too, and must never be replaced by a placeholder.
    t.is_empty()
        || numeric(t)
        || t.strip_prefix("Người dùng ").is_some_and(|rest| numeric(rest.trim()))
        || t.strip_prefix("Nhóm ").is_some_and(|rest| numeric(rest.trim()))
}

/// Extract the stable avatar filename (e.g. `"00000000000000000000000000000001.jpg"`) from a Zalo CDN URL.
pub fn avatar_file_key(avatar: Option<&str>) -> Option<String> {
    let url = avatar?.trim();
    if url.is_empty() {
        return None;
    }
    let path = url.split('?').next()?;
    let fname = path.rsplit('/').next()?.trim();
    if fname.len() >= 12
        && (fname.ends_with(".jpg") || fname.ends_with(".png") || fname.ends_with(".webp"))
    {
        Some(fname.to_ascii_lowercase())
    } else {
        None
    }
}

/// Extract the `?time=...` expiry timestamp from a Zalo avatar URL so newer session avatars win.
pub fn avatar_url_time(avatar: Option<&str>) -> i64 {
    let Some(url) = avatar else {
        return 0;
    };
    let mut max_ts = 0i64;
    for seg in url.split(';') {
        let Some(query) = seg.split('?').nth(1) else {
            continue;
        };
        for pair in query.split('&') {
            if let Some(val) = pair.strip_prefix("time=") {
                let digits: String = val.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(ts) = digits.parse::<i64>() {
                    if ts > max_ts {
                        max_ts = ts;
                    }
                }
            }
        }
    }
    max_ts
}

/// Count how many real `http(s)://` image URLs (excluding `/default`) an avatar string contains.
pub fn avatar_http_url_count(avatar: Option<&str>) -> usize {
    let Some(raw) = avatar.map(str::trim).filter(|s| !s.is_empty()) else {
        return 0;
    };
    if let Some(rest) = raw.strip_prefix("multi:") {
        rest.split(';')
            .filter_map(|seg| seg.split_once('|').map(|(_, u)| u.trim()))
            .filter(|u| {
                (u.starts_with("http://") || u.starts_with("https://")) && !u.ends_with("/default")
            })
            .count()
    } else if (raw.starts_with("http://") || raw.starts_with("https://"))
        && !raw.ends_with("/default")
    {
        1
    } else {
        0
    }
}

/// Normalize an avatar string by extracting stable image filenames (`avatar_file_key`) so `s120`
/// vs `s160` CDN subdomains and rotating `?key=...&time=...` tokens compare equal.
fn normalize_avatar_signature(avatar: &str) -> String {
    let raw = avatar.trim();
    if let Some(rest) = raw.strip_prefix("multi:") {
        let mut out = String::from("multi:");
        for (idx, seg) in rest.split(';').enumerate() {
            if idx > 0 {
                out.push(';');
            }
            if let Some((name, url)) = seg.split_once('|') {
                let u_trim = url.trim();
                let file_key = avatar_file_key(Some(u_trim))
                    .unwrap_or_else(|| u_trim.split('?').next().unwrap_or("").to_string());
                out.push_str(name.trim());
                out.push('|');
                out.push_str(&file_key);
            } else {
                out.push_str(seg.trim());
            }
        }
        out
    } else {
        avatar_file_key(Some(raw))
            .unwrap_or_else(|| raw.split('?').next().unwrap_or(raw).to_string())
    }
}

/// Decide whether `incoming` should replace `current` avatar without downgrading a rich
/// `multi:` composite avatar (with member photo URLs) or flipping between `s120` and `s160` URLs.
pub fn should_replace_avatar(current: Option<&str>, incoming: Option<&str>) -> bool {
    let Some(inc) = incoming.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some(cur) = current.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let cur_urls = avatar_http_url_count(Some(cur));
    let inc_urls = avatar_http_url_count(Some(inc));
    if inc_urls < cur_urls {
        return false;
    }
    if inc_urls > cur_urls {
        return true;
    }
    if cur.starts_with("multi:") && inc.starts_with("multi:") && cur_urls > 0 {
        return false;
    }
    if normalize_avatar_signature(cur) == normalize_avatar_signature(inc) {
        return false;
    }
    avatar_url_time(Some(inc)) > avatar_url_time(Some(cur))
}

/// Return `true` if two `Thread` entries represent the same underlying contact or group,
/// even if Zalo issued a new session-scoped `userId` after a QR re-login.
///
/// Two threads with distinct non-empty IDs are NEVER treated as the same contact solely
/// because their display names match (#63 — multiple Zalo contacts/groups frequently share
/// the same display name). Across different IDs, only the singleton "Cloud của tôi" thread
/// or contacts sharing the same non-default CDN avatar hash (`avatar_file_key`) and name match.
pub fn threads_represent_same_contact(a: &Thread, b: &Thread) -> bool {
    if !a.id.is_empty() && a.id == b.id {
        return true;
    }
    if a.is_group != b.is_group {
        return false;
    }
    if !a.is_group && a.name.contains("Cloud của tôi") && b.name.contains("Cloud của tôi") {
        return true;
    }
    // Group ids are stable; different ids are different groups even with the same name or
    // the same (default) avatar file.
    if a.is_group && !a.id.is_empty() && !b.id.is_empty() {
        return false;
    }
    if let (Some(ka), Some(kb)) = (
        avatar_file_key(a.avatar.as_deref()),
        avatar_file_key(b.avatar.as_deref()),
    ) {
        if !ka.contains("default") && ka == kb {
            return is_generic_thread_name(&a.name)
                || is_generic_thread_name(&b.name)
                || a.name.trim() == b.name.trim();
        }
        return false;
    }
    false
}

fn wire_identity(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.trim().is_empty() && value != "0" => Some(value.clone()),
        Value::Number(value) => value.as_u64().filter(|id| *id > 0).map(|id| id.to_string()),
        _ => None,
    }
}

fn wire_sender(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) if !value.is_empty() => Some(value.clone()),
        Value::Number(value) => value.as_u64().map(|id| id.to_string()),
        _ => None,
    }
}

fn server_message_id(message: &ChatMessage) -> Option<String> {
    let wire_server = wire_identity(message.wire_ref.server_id.as_ref());
    let wire_client = wire_identity(message.wire_ref.client_id.as_ref());
    if message.id.starts_with("out_")
        || message.id.is_empty()
        || wire_client.as_deref() == Some(message.id.as_str())
    {
        wire_server
    } else {
        wire_identity(Some(&Value::String(message.id.clone())))
    }
}

fn client_message_id(message: &ChatMessage) -> Option<String> {
    let wire = wire_identity(message.wire_ref.client_id.as_ref());
    if message.client_message_id.trim().is_empty() || message.client_message_id == "0" {
        wire
    } else if wire
        .as_ref()
        .is_some_and(|id| id != &message.client_message_id)
    {
        None
    } else {
        Some(message.client_message_id.clone())
    }
}

#[derive(Default)]
struct MessageIdentityIndex {
    rows: std::collections::HashMap<(u8, String), std::collections::HashSet<usize>>,
}

impl MessageIdentityIndex {
    fn keys(message: &ChatMessage) -> Vec<(u8, String)> {
        let mut keys = Vec::with_capacity(3);
        if let Some(id) = server_message_id(message) {
            keys.push((0, id));
        } else if message.id.starts_with("out_") {
            keys.push((1, message.id.clone()));
        }
        if !message.id.starts_with("out_") {
            if let Some(id) = wire_identity(Some(&Value::String(message.id.clone()))) {
                // Old caches only retain this ID; it may have come from cliMsgId.
                keys.push((3, id));
            }
        }
        if let Some(id) = client_message_id(message) {
            keys.push((2, id));
        }
        keys
    }

    fn insert(&mut self, message: &ChatMessage, index: usize) {
        for key in Self::keys(message) {
            self.rows.entry(key).or_default().insert(index);
        }
    }

    fn remove(&mut self, message: &ChatMessage, index: usize) {
        for key in Self::keys(message) {
            if let Some(rows) = self.rows.get_mut(&key) {
                rows.remove(&index);
            }
        }
    }

    fn find(&self, messages: &[ChatMessage], incoming: &ChatMessage) -> Option<usize> {
        for key in Self::keys(incoming) {
            let Some(rows) = self.rows.get(&key).filter(|rows| !rows.is_empty()) else {
                continue;
            };
            // A colliding identity is not evidence for selecting any particular row.
            if rows.len() != 1 {
                return None;
            }
            let index = *rows.iter().next()?;
            let existing = &messages[index];
            if existing.is_outgoing != incoming.is_outgoing {
                return None;
            }
            if let (Some(a), Some(b)) = (
                wire_sender(existing.wire_ref.sender.as_ref()),
                wire_sender(incoming.wire_ref.sender.as_ref()),
            ) {
                if a != b {
                    return None;
                }
            }
            if key.0 != 2 {
                if key.0 == 3 {
                    if let (Some(a), Some(b)) = (
                        wire_identity(existing.wire_ref.server_id.as_ref()),
                        wire_identity(incoming.wire_ref.server_id.as_ref()),
                    ) {
                        if a != b {
                            return None;
                        }
                    }
                }
                return Some(index);
            }
            // Client-only packets can replay before a server ID is known. A client
            // reference never collapses two distinct pending or server messages.
            let existing_server = server_message_id(existing);
            let incoming_server = server_message_id(incoming);
            let existing_local = existing.id.starts_with("out_") && existing_server.is_none();
            let incoming_local = incoming.id.starts_with("out_") && incoming_server.is_none();
            let client_only_replay = existing_server.is_none()
                && incoming_server.is_none()
                && (existing.id.is_empty() || incoming.id.is_empty());
            return (client_only_replay
                || (existing.is_outgoing
                    && ((existing_local && incoming_server.is_some())
                        || (incoming_local && existing_server.is_some()))))
            .then_some(index);
        }
        None
    }
}

/// Merge a local send acknowledgement without replacing an existing server clock.
pub(crate) fn merge_sent_message(thread: &mut Thread, mut message: ChatMessage) {
    let mut identities = MessageIdentityIndex::default();
    for (row, existing) in thread.messages.iter().enumerate() {
        identities.insert(existing, row);
    }
    if identities
        .find(&thread.messages, &message)
        .is_some_and(|row| server_message_id(&thread.messages[row]).is_some())
    {
        // The acknowledgement receipt time is not the server's message timestamp.
        message.ts = 0;
        message.time.clear();
    }
    merge_thread_into(
        thread,
        Thread {
            messages: vec![message],
            ..Default::default()
        },
        false,
    );
}

/// Merge `incoming` thread into `target`. When `incoming_id_is_authoritative` is `true`,
/// `target.id` is migrated to `incoming.id` so actions (chat/VoIP call) always target the active session's `userId`.
pub fn merge_thread_into(
    target: &mut Thread,
    incoming: Thread,
    incoming_id_is_authoritative: bool,
) {
    if incoming_id_is_authoritative && !incoming.id.is_empty() && incoming.id != "0" {
        target.id = incoming.id;
    }
    if !incoming.name.is_empty()
        && (!is_generic_thread_name(&incoming.name) || is_generic_thread_name(&target.name))
    {
        target.name = incoming.name;
    }
    if should_replace_avatar(target.avatar.as_deref(), incoming.avatar.as_deref()) {
        target.avatar = incoming.avatar;
    }
    target.is_group = target.is_group || incoming.is_group;
    if let Some(count) = incoming.presentation.member_count {
        target.presentation.member_count = Some(count);
    }

    // Repeated text and timestamps are not identities. Keep ambiguous keys instead of
    // choosing the first candidate, especially when multiple pending sends collide.
    let mut identities = MessageIdentityIndex::default();
    for (index, message) in target.messages.iter().enumerate() {
        identities.insert(message, index);
    }

    for msg in incoming.messages {
        if msg.text == "sendBubbleMessage" {
            continue;
        }
        let found = identities.find(&target.messages, &msg);
        if let Some(idx) = found {
            let existing_msg = &mut target.messages[idx];
            identities.remove(existing_msg, idx);
            existing_msg.merge_reaction_metadata(&msg);
            if existing_msg.id.starts_with("out_")
                && !msg.id.is_empty()
                && !msg.id.starts_with("out_")
            {
                existing_msg.id = msg.id.clone();
            }
            if msg.ts > 0 && existing_msg.ts != msg.ts {
                existing_msg.ts = msg.ts;
                existing_msg.time = msg.time;
            }
            if msg.text.trim() == "[Tin nhắn đã thu hồi]" {
                existing_msg.text = "[Tin nhắn đã thu hồi]".to_string();
                existing_msg.sticker_id = None;
                existing_msg.link_url = None;
                existing_msg.link_thumb = None;
            }
            if existing_msg.sticker_id.is_none() && msg.sticker_id.is_some() {
                existing_msg.sticker_id = msg.sticker_id;
                existing_msg.sticker_cate_id = msg.sticker_cate_id;
                existing_msg.sticker_type = msg.sticker_type;
            }
            if existing_msg.reply_to_text.is_none() && msg.reply_to_text.is_some() {
                existing_msg.reply_to_id = msg.reply_to_id;
                existing_msg.reply_to_sender = msg.reply_to_sender;
                existing_msg.reply_to_text = msg.reply_to_text;
            }
            if existing_msg.link_url.is_none() && msg.link_url.is_some() {
                existing_msg.link_url = msg.link_url;
                existing_msg.link_title = msg.link_title;
                existing_msg.link_desc = msg.link_desc;
                existing_msg.link_thumb = msg.link_thumb;
                existing_msg.link_src = msg.link_src;
                if !msg.text.is_empty() {
                    existing_msg.text = msg.text;
                }
            }
            if existing_msg.reactions.is_empty() && !msg.reactions.is_empty() {
                existing_msg.reactions = msg.reactions;
            }
            existing_msg.is_pinned = existing_msg.is_pinned || msg.is_pinned;
            existing_msg.is_starred = existing_msg.is_starred || msg.is_starred;
            identities.insert(existing_msg, idx);
        } else {
            let idx = target.messages.len();
            identities.insert(&msg, idx);
            target.messages.push(msg);
        }
    }

    target.messages.retain(|m| m.text != "sendBubbleMessage");
    target.messages.sort_by_key(|m| m.ts);

    if let Some(newest) = target.messages.last() {
        if !newest.text.is_empty() {
            target.preview = newest.text.replace('\n', " ");
            target.time = newest.time.clone();
        }
    } else if !incoming.preview.is_empty() && target.preview.is_empty() {
        target.preview = incoming.preview;
        if !incoming.time.is_empty() {
            target.time = incoming.time;
        }
    } else if target.time.is_empty() && !incoming.time.is_empty() {
        target.time = incoming.time;
    }
}

/// Deduplicate `threads` in-place, merging entries that represent the same contact/group
/// (by `id`, avatar hash, or non-generic display name) and preserving the authoritative/newer `id`.
pub fn deduplicate_threads(
    threads: &mut Vec<Thread>,
    authoritative_ids: Option<&std::collections::HashSet<String>>,
) {
    let mut merged: Vec<Thread> = Vec::with_capacity(threads.len());
    for mut t in threads.drain(..) {
        if t.id.is_empty() || t.id == "0" {
            continue;
        }
        t.messages.retain(|m| m.text != "sendBubbleMessage");
        if let Some(newest) = t.messages.last() {
            if !newest.text.is_empty() && (t.preview.is_empty() || t.preview == "sendBubbleMessage")
            {
                t.preview = newest.text.replace('\n', " ");
                t.time = newest.time.clone();
            }
        }
        if let Some(existing) = merged.iter_mut().find(|e| {
            if e.id == t.id {
                return true;
            }
            if let Some(auth) = authoritative_ids {
                if auth.contains(&e.id) && auth.contains(&t.id) {
                    return false;
                }
            }
            threads_represent_same_contact(e, &t)
        }) {
            let incoming_auth = if let Some(auth) = authoritative_ids {
                auth.contains(&t.id) && !auth.contains(&existing.id)
            } else {
                // In cache order, later entries or entries with newer avatar URL timestamps come from newer sessions
                let t_time = avatar_url_time(t.avatar.as_deref());
                let e_time = avatar_url_time(existing.avatar.as_deref());
                if t_time != e_time {
                    t_time > e_time
                } else {
                    true
                }
            };
            merge_thread_into(existing, t, incoming_auth);
        } else {
            merged.push(t);
        }
    }
    let cloud_msg_ids: std::collections::HashSet<String> = merged
        .iter()
        .filter(|t| t.name.contains("Cloud của tôi"))
        .flat_map(|t| t.messages.iter())
        .filter_map(|m| {
            if !m.id.is_empty() && !m.id.starts_with("out_") && !m.id.starts_with("local_") {
                Some(m.id.clone())
            } else {
                None
            }
        })
        .collect();
    if !cloud_msg_ids.is_empty() {
        for t in merged.iter_mut() {
            if !t.name.contains("Cloud của tôi") {
                let before = t.messages.len();
                t.messages.retain(|m| !cloud_msg_ids.contains(&m.id));
                if t.messages.len() != before {
                    if let Some(newest) = t.messages.last() {
                        t.preview = newest.text.replace('\n', " ");
                        t.time = newest.time.clone();
                    } else {
                        t.preview.clear();
                        t.time.clear();
                    }
                }
            }
        }
    }
    *threads = merged;
}

fn active_uin_from_session() -> Option<String> {
    if let Ok(text) = std::fs::read_to_string("session.json") {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(uin) = val.pointer("/login/uin").and_then(|v| v.as_str()) {
                if !uin.is_empty() {
                    return Some(uin.to_string());
                }
            }
        }
    }
    None
}

/// Load locally cached conversations and message history scoped to active account.
pub fn load_threads_cache() -> Vec<Thread> {
    if let Some(uin) = active_uin_from_session() {
        let acc_path = format!("accounts/{uin}_threads.json");
        if let Ok(text) = std::fs::read_to_string(&acc_path) {
            if let Ok(mut threads) = serde_json::from_str::<Vec<Thread>>(&text) {
                deduplicate_threads(&mut threads, None);
                return threads;
            }
        }
    }
    if std::path::Path::new("session.json").exists() {
        if let Ok(text) = std::fs::read_to_string("threads_cache.json") {
            if let Ok(mut threads) = serde_json::from_str::<Vec<Thread>>(&text) {
                deduplicate_threads(&mut threads, None);
                return threads;
            }
        }
    }
    Vec::new()
}

fn cache_content_hash(text: &str) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// `true` when the serialized cache must be written: first write, changed content, or the file
/// is gone (deleted by the user or another build, so the remembered hash no longer describes it).
fn cache_write_needed(new_hash: u64, last_hash: Option<u64>, file_exists: bool) -> bool {
    !file_exists || last_hash != Some(new_hash)
}

pub fn load_threads_cache_for_account(uin: &str) -> Vec<Thread> {
    let Some(uin) = crate::sync_state::sanitize_uin(uin) else { return Vec::new(); };
    let path = format!("accounts/{uin}_threads.json");
    std::fs::read_to_string(path).ok()
        .and_then(|text| serde_json::from_str::<Vec<Thread>>(&text).ok())
        .map(|mut threads| { deduplicate_threads(&mut threads, None); threads })
        .unwrap_or_default()
}

/// Snapshot of the thread list for an instant first paint. Only the newest
/// `PRELOAD_MSGS` messages per thread are kept: full history lives in SQLite, and
/// rewriting every message as pretty JSON on each change cost more than the SQLite write.
fn serialize_threads_cache(threads: &[Thread]) -> serde_json::Result<String> {
    let keep = crate::store::MessageStore::PRELOAD_MSGS;
    let slim: Vec<Thread> = threads
        .iter()
        .map(|t| Thread {
            presentation: t.presentation.clone(),
            unread: t.unread.clone(),
            id: t.id.clone(),
            name: t.name.clone(),
            preview: t.preview.clone(),
            time: t.time.clone(),
            avatar: t.avatar.clone(),
            is_group: t.is_group,
            messages: t.messages[t.messages.len().saturating_sub(keep)..].to_vec(),
        })
        .collect();
    serde_json::to_string(&slim)
}

pub fn save_threads_cache_for_account(threads: &[Thread], uin: &str) {
    let Some(uin) = crate::sync_state::sanitize_uin(uin) else { return; };
    if let Ok(text) = serialize_threads_cache(threads) {
        let path = format!("accounts/{uin}_threads.json");
        let _ = crate::paths::write_private(std::path::Path::new(&path), text);
    }
}

pub fn save_threads_cache(threads: &[Thread]) {
    if let Ok(text) = serialize_threads_cache(threads) {
        let _ = crate::paths::write_private(std::path::Path::new("threads_cache.json"), &text);
        if let Some(uin) = active_uin_from_session().and_then(|value| crate::sync_state::sanitize_uin(&value).map(str::to_owned)) {
            let path = format!("accounts/{uin}_threads.json");
            let _ = crate::paths::write_private(std::path::Path::new(&path), text);
        }
    }
}

/// Hosts to try for `getfriends`, in order: `friend` first, `profile` as a fallback, no
/// empties, no duplicates.
fn friend_host_order<'a>(friend: &'a str, profile: &'a str) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::with_capacity(2);
    for h in [friend, profile] {
        if !h.is_empty() && !out.contains(&h) {
            out.push(h);
        }
    }
    out
}

/// `/api/social/friend/getfriends` — the friend list.
///
/// Tries `s.domains.friend` (the canonical `tt-friend-wpa.chat.zalo.me` host) first and
/// `s.domains.profile` only as a fallback. Every failing host is logged and the final error
/// carries context instead of being swallowed (upstream issue #22).
pub async fn get_friends(s: &Session) -> Result<Vec<Thread>> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[
        ("incInvalid", json!(1)),
        ("page", json!(1)),
        ("count", json!(20000)),
        ("avatar_size", json!(160)),
        ("actiontime", json!(0)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_friends::encoded_params", &e.to_string());
            eprintln!("! get_friends: no session key for encoded_params ({e}), falling back to plain_params");
            Session::plain_params(&body)
        }
    };
    let path_and_query = format!(
        "/api/social/friend/getfriends?{}{}",
        s.common_params(),
        if params.is_empty() {
            String::new()
        } else {
            format!("&{params}")
        }
    );
    let raw = s
        .get_endpoint("friend", &path_and_query)
        .await
        .map_err(|e| e.context("không tải được danh sách bạn bè"))?;
    let resp = s.maybe_decrypt(raw, s.session_key_bytes()).await;
    let data_root = resp.get("data").unwrap_or(&resp);
    let mut threads: Vec<Thread> = match data_root {
        Value::Array(items) => items.iter().filter_map(parse_thread).collect(),
        Value::Object(o) if o.contains_key("friends") => o["friends"]
            .as_array()
            .map(|a| a.iter().filter_map(parse_thread).collect())
            .unwrap_or_default(),
        Value::Object(o) if o.get("data").and_then(Value::as_array).is_some() => o["data"]
            .as_array()
            .map(|a| a.iter().filter_map(parse_thread).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    // If Cloud của tôi (send2me) is available, place it at the top
    if let Some(li) = &s.login {
        if !li.send2me_id.is_empty() && !threads.iter().any(|t| t.id == li.send2me_id) {
            threads.insert(
                0,
                Thread {
                    presentation: Default::default(),
                    unread: Default::default(),
                    id: li.send2me_id.clone(),
                    name: "Cloud của tôi (Lưu trữ)".into(),
                    preview: "".into(),
                    time: crate::ui::clock(),
                    avatar: None,
                    is_group: false,
                    messages: Vec::new(),
                },
            );
        }
    }

    // Hydrate with locally persisted history from SQLite scoped to this account
    let uin = s.login.as_ref().map(|l| l.uin.as_str()).unwrap_or_default();
    let db_path = crate::store::db_path_for_account(uin);
    crate::store::load_history_into_threads(
        &db_path,
        &mut threads,
        crate::store::MessageStore::PRELOAD_MSGS,
    );

    // Merge user groups (`getlg/v4` + `getmg-v2`) into threads list, upgrading any placeholder group names
    if let Ok(mut groups) = get_groups(s).await {
        for g in groups.drain(..) {
            if let Some(existing) = threads.iter_mut().find(|t| t.id == g.id) {
                merge_thread_into(existing, g, true);
            } else {
                threads.push(g);
            }
        }
    }

    resolve_thread_names(s, &mut threads).await;

    Ok(threads)
}

fn parse_thread(v: &Value) -> Option<Thread> {
    let id = pick_str(v, &["userId", "uid", "id", "friendId", "globalId"])?;
    let name = pick_str(v, &["displayName", "zaloName", "display_name", "name"])
        .or_else(|| {
            v.get("profile_displayName")
                .or_else(|| v.get("profile").and_then(|p| p.get("displayName")))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| id.clone());
    let avatar = pick_str(v, &["avatar", "avt", "profile_avatar"]);
    let is_group = is_group_flag(v);
    let time = v
        .get("lastActionTime")
        .and_then(|val| match val {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        })
        .map(crate::clock::message_time)
        .unwrap_or_default();

    Some(Thread {
        presentation: Default::default(),
        unread: Default::default(),
        id,
        name,
        preview: String::new(),
        time,
        avatar,
        is_group,
        messages: Vec::new(),
    })
}

fn pick_str(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => return Some(s.clone()),
            Some(Value::Number(n)) => return Some(n.to_string()),
            _ => {}
        }
    }
    None
}

/// Allocate a millisecond-shaped request ID once per send, including simultaneous sends.
pub(crate) fn next_client_message_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST_ID: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let previous = LAST_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            Some(now.max(last.saturating_add(1)))
        })
        .unwrap_or_else(|last| last);
    now.max(previous.saturating_add(1))
}

/// Convert inline Markdown (`**bold**`, `__underline__`, `~~strikethrough~~`, `*italic*`) and
/// line-prefix styles (`## `, `- `, `1. `) into Zalo PC's native `(clean_text, Option<textProperties>)`
/// format (`{"styles":[{"start":N,"len":M,"st":"b,i,u,s,f_18,lst_1,lst_2"}],"ver":0}` in `compact-app-pc.js`).
pub fn extract_zalo_rtf_text_properties(raw: &str) -> (String, Option<String>) {
    let mut out_text = String::with_capacity(raw.len());
    let mut styles: Vec<Value> = Vec::new();

    let lines: Vec<&str> = raw.split('\n').collect();
    for (line_idx, raw_line) in lines.iter().enumerate() {
        if line_idx > 0 {
            out_text.push('\n');
        }
        let mut line = *raw_line;
        let mut line_tags: Vec<&'static str> = Vec::new();
        if let Some(rest) = line.strip_prefix("### ") {
            line = rest;
            line_tags.push("b");
            line_tags.push("f_16");
        } else if let Some(rest) = line.strip_prefix("## ") {
            line = rest;
            line_tags.push("b");
            line_tags.push("f_18");
        } else if let Some(rest) = line.strip_prefix("# ") {
            line = rest;
            line_tags.push("b");
            line_tags.push("f_18");
        } else if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            line = rest;
            line_tags.push("lst_1");
        } else if let Some(rest) = line.strip_prefix("1. ") {
            line = rest;
            line_tags.push("lst_2");
        }

        let line_start_u16 = out_text.encode_utf16().count();
        let mut rest = line;
        while !rest.is_empty() {
            let mut matched_span = false;
            for (delim, st_tag) in [("`", ""), ("**", "b"), ("__", "u"), ("~~", "s"), ("*", "i")] {
                if rest.starts_with(delim) {
                    let dlen = delim.len();
                    let after = &rest[dlen..];
                    if let Some(end_pos) = after.find(delim) {
                        let inner = &after[..end_pos];
                        if !inner.is_empty() && !inner.contains('\n') {
                            let span_start = out_text.encode_utf16().count();
                            let span_len = inner.encode_utf16().count();
                            out_text.push_str(inner);
                            if !st_tag.is_empty() && span_len > 0 {
                                styles.push(json!({
                                    "start": span_start,
                                    "len": span_len,
                                    "st": st_tag,
                                }));
                            }
                            rest = &after[end_pos + dlen..];
                            matched_span = true;
                            break;
                        }
                    }
                }
            }
            if matched_span {
                continue;
            }
            let mut chars = rest.chars();
            if let Some(ch) = chars.next() {
                out_text.push(ch);
                rest = chars.as_str();
            } else {
                break;
            }
        }
        let line_len_u16 = out_text
            .encode_utf16()
            .count()
            .saturating_sub(line_start_u16);
        if !line_tags.is_empty() && line_len_u16 > 0 {
            styles.push(json!({
                "start": line_start_u16,
                "len": line_len_u16,
                "st": line_tags.join(","),
            }));
        }
    }

    if styles.is_empty() {
        (raw.to_string(), None)
    } else {
        let prop_str = json!({
            "styles": styles,
            "ver": 0,
        })
        .to_string();
        (out_text, Some(prop_str))
    }
}

/// Reconstruct Markdown markers from Zalo's incoming `rtf` object (`{"action":"rtf","title":...,"params":"{\"styles\":[...]}"}`)
/// so incoming rich-text messages sent from official Zalo PC / Mobile render with bold, underline, strikethrough, and lists.
fn format_incoming_zalo_rtf(title: &str, params_str: &str) -> String {
    let Ok(p_val) = serde_json::from_str::<Value>(params_str.trim()) else {
        return title.to_string();
    };
    let Some(styles) = p_val.get("styles").and_then(Value::as_array) else {
        return title.to_string();
    };
    if styles.is_empty() {
        return title.to_string();
    }
    let u16_units: Vec<u16> = title.encode_utf16().collect();
    let mut spans: Vec<(usize, usize, String)> = styles
        .iter()
        .filter_map(|s| {
            let start = s.get("start")?.as_u64()? as usize;
            let len = s.get("len")?.as_u64()? as usize;
            let st = s.get("st")?.as_str()?.to_string();
            if len > 0 && start + len <= u16_units.len() {
                Some((start, start + len, st))
            } else {
                None
            }
        })
        .collect();
    if spans.is_empty() {
        return title.to_string();
    }
    spans.sort_by_key(|x| x.0);
    let mut out = String::with_capacity(title.len() + spans.len() * 4);
    let mut cursor = 0usize;
    for (s, e, st) in spans {
        if s < cursor {
            continue;
        }
        if s > cursor {
            out.push_str(&String::from_utf16_lossy(&u16_units[cursor..s]));
        }
        let seg = String::from_utf16_lossy(&u16_units[s..e]);
        let tags: Vec<&str> = st.split(',').collect();
        let is_bold = tags.contains(&"b") || tags.iter().any(|t| t.starts_with("f_"));
        let is_italic = tags.contains(&"i");
        let is_under = tags.contains(&"u");
        let is_strike = tags.contains(&"s");
        let mut wrapped = seg;
        if is_italic && !is_bold {
            wrapped = format!("*{wrapped}*");
        }
        if is_bold {
            wrapped = format!("**{wrapped}**");
        }
        if is_under {
            wrapped = format!("__{wrapped}__");
        }
        if is_strike {
            wrapped = format!("~~{wrapped}~~");
        }
        out.push_str(&wrapped);
        cursor = e;
    }
    if cursor < u16_units.len() {
        out.push_str(&String::from_utf16_lossy(&u16_units[cursor..]));
    }
    out
}

/// `Some((code, message))` when a reply envelope reports a non-zero `error_code`.
///
/// [O] Zalo answers **HTTP 200** even when an action failed: the verdict only exists in
/// `error_code` (`re-notes/10` §3). A sender that never reads it reports a message the server
/// dropped, which is the worst failure mode for a chat client — so every sender below shares
/// this check. `None` also covers a reply without any `error_code` (nothing to reject).
fn reply_error(v: &Value) -> Option<(i64, String)> {
    let code = v.get("error_code").and_then(|c| c.as_i64())?;
    if code == 0 {
        return None;
    }
    let message = v
        .get("error_message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    Some((code, message))
}

/// The `Err` a failed action returns: Vietnamese for the user, the server code for the log.
fn failed(action: &str, (code, message): (i64, String)) -> anyhow::Error {
    if message.is_empty() {
        anyhow::anyhow!("{action} thất bại (mã {code})")
    } else {
        anyhow::anyhow!("{action} thất bại (mã {code}: {message})")
    }
}

/// Reject a **write** action whose reply carries a non-zero `error_code`.
///
/// One place for the rule every mutating call in this module follows: log the server code under
/// `scope`, then hand the caller a Vietnamese `Err` naming the action (`action`). A reply with no
/// `error_code` has no verdict, so it is not turned into a failure here.
fn ensure_action_ok(action: &str, scope: &str, reply: &Value) -> Result<()> {
    if let Some(err) = reply_error(reply) {
        crate::logger::log_error(scope, &format!("mã {}: {}", err.0, err.1));
        return Err(failed(action, err));
    }
    Ok(())
}

/// Send a 1-1 text message: `POST {domains.chat}/api/message/sms?{common}`
pub async fn send_message(s: &Session, to_id: &str, text: &str) -> Result<Value> {
    send_message_with_client_id(s, to_id, text, next_client_message_id()).await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn send_message_with_client_id(
    s: &Session,
    to_id: &str,
    text: &str,
    now: u64,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let (clean_text, text_props_opt) = extract_zalo_rtf_text_properties(text);
    let mut body = obj(&[
        ("toid", json!(to_id)),
        ("message", json!(clean_text)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
    ]);
    if let (Some(tp), Some(map)) = (text_props_opt, body.as_object_mut()) {
        map.insert("textProperties".into(), json!(tp));
    }
    let params = s.encoded_params(&body)?;

    let host = if s.domains.chat.contains("-msg-chat.")
        || s.domains.chat.contains("tt-chat2-wpa")
        || s.domains.chat.is_empty()
    {
        "https://tt-chat3-wpa.api.zaloapp.com"
    } else {
        &s.domains.chat
    };

    let url = format!("{host}/api/message/sms?{}", s.common_params());
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(crate::session::SessionError::new(code, msg).into());
        }
    }
    Ok(resp)
}

/// Send a group text message: `POST {domains.group}/api/group/sendmsg?{common}`
pub async fn send_group_message(s: &Session, grid: &str, text: &str) -> Result<Value> {
    send_group_message_with_client_id(s, grid, text, next_client_message_id()).await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn send_group_message_with_client_id(
    s: &Session,
    grid: &str,
    text: &str,
    now: u64,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean_grid = grid
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let (clean_text, text_props_opt) = extract_zalo_rtf_text_properties(text);
    let mut body = obj(&[
        ("grid", json!(clean_grid)),
        ("message", json!(clean_text)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
        ("visibility", json!(0)),
    ]);
    if let (Some(tp), Some(map)) = (text_props_opt, body.as_object_mut()) {
        map.insert("textProperties".into(), json!(tp));
    }
    let params = s.encoded_params(&body)?;
    let url = format!(
        "{}/api/group/sendmsg?{}",
        s.domains.group,
        s.common_params()
    );
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(crate::session::SessionError::new(code, msg).into());
        }
    }
    Ok(resp)
}

/// Format a byte count into a human-readable size string (e.g. `"1.4 MB"`, `"320 KB"`).
pub fn format_file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn md5_hex_bytes(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(bytes);
    let out = h.finalize();
    out.iter().fold(String::with_capacity(32), |mut acc, b| {
        use std::fmt::Write;
        let _ = write!(acc, "{b:02x}");
        acc
    })
}

/// How long `send_attachment` waits for the `file_done` control event of an async file upload.
const FILE_DONE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);
/// How long an unclaimed `file_done` event is kept for a waiter that registers late.
const FILE_DONE_EARLY_TTL: std::time::Duration = std::time::Duration::from_secs(120);

/// Async file uploads waiting for their `act_type: "file_done"` control event, keyed by `fileId`.
///
/// `asyncfile/upload` only returns a `fileId`; Zalo pushes the final `fileUrl` later over the
/// WebSocket (`cmd 601`) or `lp_control`, and `asyncfile/msg` is rejected without it.
#[derive(Default)]
struct FileDoneRegistry {
    waiters: std::collections::HashMap<String, tokio::sync::oneshot::Sender<String>>,
    /// Events that arrived before their waiter registered (`fileId` -> (`fileUrl`, received at)).
    early: std::collections::HashMap<String, (String, std::time::Instant)>,
}

static FILE_DONE_REGISTRY: std::sync::LazyLock<std::sync::Mutex<FileDoneRegistry>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(FileDoneRegistry::default()));

/// Set once any control-event consumer (WebSocket or `lp_control` poll) feeds `file_done` events,
/// so front-ends without one (TUI) skip the wait instead of stalling for `FILE_DONE_TIMEOUT`.
static FILE_DONE_LISTENER_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn file_id_key(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parse `act_type: "file_done"` items (`{fileId, data: {url}}`) into `(fileId, fileUrl)` pairs.
pub fn extract_file_done_controls(data_node: &Value) -> Vec<(String, String)> {
    let mut candidates: Vec<&Value> = Vec::new();
    if let Some(arr) = data_node.get("controls").and_then(Value::as_array) {
        candidates.extend(arr.iter());
    } else if let Some(arr) = data_node.as_array() {
        for item in arr {
            if let Some(sub) = item.get("controls").and_then(Value::as_array) {
                candidates.extend(sub.iter());
            } else {
                candidates.push(item);
            }
        }
    } else {
        candidates.push(data_node);
    }

    let mut out = Vec::new();
    for item in candidates {
        let c = item.get("content").unwrap_or(item);
        if c.get("act_type").and_then(Value::as_str) != Some("file_done") {
            continue;
        }
        let raw_data = c.get("data").unwrap_or(&Value::Null);
        let parsed_data: Value;
        let d = if let Some(s) = raw_data.as_str() {
            parsed_data = serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
            &parsed_data
        } else {
            raw_data
        };
        let Some(file_id) = c
            .get("fileId")
            .or_else(|| d.get("fileId"))
            .and_then(file_id_key)
        else {
            continue;
        };
        let Some(url) = pick_str(d, &["url", "fileUrl"]) else {
            continue;
        };
        out.push((file_id, url));
    }
    out
}

/// Resolve pending async file uploads from a decrypted control payload (WebSocket `cmd 601` or
/// `lp_control`). Unclaimed events are kept briefly for uploads whose waiter registers late.
pub fn dispatch_file_done_controls(data_node: &Value) {
    FILE_DONE_LISTENER_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
    let events = extract_file_done_controls(data_node);
    if events.is_empty() {
        return;
    }
    let Ok(mut reg) = FILE_DONE_REGISTRY.lock() else {
        crate::logger::log_error("chat::file_done", "registry mutex poisoned");
        return;
    };
    reg.early
        .retain(|_, (_, at)| at.elapsed() < FILE_DONE_EARLY_TTL);
    for (file_id, url) in events {
        match reg.waiters.remove(&file_id) {
            Some(tx) => {
                let _ = tx.send(url);
            }
            None => {
                reg.early.insert(file_id, (url, std::time::Instant::now()));
            }
        }
    }
}

fn register_file_done_waiter(file_id: &str) -> tokio::sync::oneshot::Receiver<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    match FILE_DONE_REGISTRY.lock() {
        Ok(mut reg) => match reg.early.remove(file_id) {
            Some((url, _)) => {
                let _ = tx.send(url);
            }
            None => {
                reg.waiters.insert(file_id.to_string(), tx);
            }
        },
        // Dropping `tx` makes the receiver resolve immediately with an error.
        Err(_) => crate::logger::log_error("chat::file_done", "registry mutex poisoned"),
    }
    rx
}

/// Wait up to `timeout` for the `file_done` event of `file_id`; `None` on timeout.
async fn wait_for_file_done(file_id: &str, timeout: std::time::Duration) -> Option<String> {
    let rx = register_file_done_waiter(file_id);
    let res = tokio::time::timeout(timeout, rx).await;
    if let Ok(mut reg) = FILE_DONE_REGISTRY.lock() {
        reg.waiters.remove(file_id);
    }
    res.ok().and_then(Result::ok)
}

/// Upload and send a photo, video, or general file attachment to a 1-1 or group conversation.
///
/// Implements Zalo PC 26.10.11's two-stage chunked upload (`photo_original/upload` -> `photo_original/send`
/// for images; `asyncfile/upload` -> `asyncfile/msg` for videos/files) with automatic fallback to
/// single-shot `message/photo` (`11152`) and `message/sharefile_full` (`11146`).
pub async fn send_attachment(
    s: &Session,
    to_id: &str,
    is_group: bool,
    file_path: &std::path::Path,
    caption: &str,
) -> Result<(Value, ChatMessage)> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let file_bytes = std::fs::read(file_path)?;
    if file_bytes.is_empty() {
        anyhow::bail!("Tập tin rỗng (0 byte)");
    }
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment.bin")
        .to_string();
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let is_image = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp"
    );
    let is_video = matches!(ext.as_str(), "mp4" | "mov" | "mkv" | "webm" | "avi");

    let file_host = if s.domains.file.is_empty() {
        "https://tt-files-wpa.zaloapp.com"
    } else {
        s.domains.file.trim_end_matches('/')
    };
    let clean_target = if is_group {
        to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G')
    } else {
        to_id
    };
    let client_id = next_client_message_id();
    let total_size = file_bytes.len() as u64;
    let chunk_size: usize = 1_048_576; // 1 MiB chunks
    let total_chunks = file_bytes.len().div_ceil(chunk_size).max(1);
    let target_type = if is_group { 11 } else { 2 };

    if is_image {
        let (width, height) = image::load_from_memory(&file_bytes)
            .map(|img| (img.width(), img.height()))
            .unwrap_or((800, 600));
        let mime = match ext.as_str() {
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        let upload_ep = if is_group {
            "/api/group/photo_original/upload"
        } else {
            "/api/message/photo_original/upload"
        };

        let mut last_up_dec = Value::Null;
        for (idx, chunk) in file_bytes.chunks(chunk_size).enumerate() {
            let chunk_id = idx + 1;
            let mut m = serde_json::Map::new();
            m.insert("totalChunk".into(), json!(total_chunks));
            m.insert("fileName".into(), json!(file_name));
            m.insert("clientId".into(), json!(client_id));
            m.insert("totalSize".into(), json!(total_size));
            m.insert("imei".into(), json!(s.imei));
            m.insert("isE2EE".into(), json!(0));
            m.insert("jxl".into(), json!(0));
            m.insert("chunkId".into(), json!(chunk_id));
            if is_group {
                m.insert("grid".into(), json!(clean_target));
            } else {
                m.insert("toid".into(), json!(clean_target));
            }
            let params = s.encoded_params(&Value::Object(m))?;
            let url = format!(
                "{file_host}{upload_ep}?{}&params={params}&type={target_type}",
                s.common_params()
            );
            let up_raw = s
                .post_multipart(&url, "chunkContent", &file_name, mime, chunk)
                .await?;
            last_up_dec = s.maybe_decrypt(up_raw, s.session_key_bytes()).await;
        }

        let data_node = last_up_dec.get("data").unwrap_or(&last_up_dec);
        let photo_id = pick_str(data_node, &["photoId", "clientFileId"]);
        let normal_url =
            pick_str(data_node, &["normalUrl", "hdUrl", "oriUrl", "url"]).unwrap_or_default();
        let hd_url = pick_str(data_node, &["hdUrl", "normalUrl", "oriUrl"])
            .unwrap_or_else(|| normal_url.clone());
        let thumb_url = pick_str(data_node, &["thumbUrl", "normalUrl", "hdUrl"])
            .unwrap_or_else(|| normal_url.clone());

        let send_resp = if let Some(pid) = photo_id.filter(|_| !normal_url.is_empty()) {
            let send_ep = if is_group {
                "/api/group/photo_original/send"
            } else {
                "/api/message/photo_original/send"
            };
            let pid_val = pid
                .parse::<u64>()
                .map(|n| json!(n))
                .unwrap_or_else(|_| json!(pid));
            let mut send_obj = serde_json::Map::new();
            send_obj.insert("photoId".into(), pid_val);
            send_obj.insert("clientId".into(), json!(client_id));
            send_obj.insert("desc".into(), json!(caption));
            send_obj.insert("width".into(), json!(width));
            send_obj.insert("height".into(), json!(height));
            send_obj.insert("rawUrl".into(), json!(normal_url));
            send_obj.insert("hdUrl".into(), json!(hd_url));
            send_obj.insert("thumbUrl".into(), json!(thumb_url));
            send_obj.insert("oriUrl".into(), json!(normal_url));
            send_obj.insert("normalUrl".into(), json!(normal_url));
            send_obj.insert("hdSize".into(), json!(total_size));
            send_obj.insert("zsource".into(), json!(-1));
            send_obj.insert("jcp".into(), json!("{\"convertible\":true}"));
            send_obj.insert("imei".into(), json!(s.imei));
            if is_group {
                send_obj.insert("grid".into(), json!(clean_target));
                send_obj.insert("visibility".into(), json!(0));
            } else {
                send_obj.insert("toid".into(), json!(clean_target));
            }
            let params = s.encoded_params(&Value::Object(send_obj))?;
            let url = format!("{file_host}{send_ep}?{}", s.common_params());
            let r = s.post_form(&url, format!("params={params}")).await?;
            s.maybe_decrypt(r, s.session_key_bytes()).await
        } else {
            // Fallback: single-shot `/api/message/photo` or `/api/group/sendpt`
            let single_ep = if is_group {
                "/api/group/sendpt"
            } else {
                "/api/message/photo"
            };
            let mut body_obj = serde_json::Map::new();
            body_obj.insert("clientId".into(), json!(client_id));
            body_obj.insert("desc".into(), json!(caption));
            body_obj.insert("width".into(), json!(width));
            body_obj.insert("height".into(), json!(height));
            body_obj.insert("imei".into(), json!(s.imei));
            if is_group {
                body_obj.insert("grid".into(), json!(clean_target));
                body_obj.insert("visibility".into(), json!(0));
            } else {
                body_obj.insert("toid".into(), json!(clean_target));
            }
            let params = s.encoded_params(&Value::Object(body_obj))?;
            let url = format!(
                "{file_host}{single_ep}?{}&params={params}&type={target_type}",
                s.common_params()
            );
            let r = s
                .post_multipart(&url, "fileContent", &file_name, mime, &file_bytes)
                .await?;
            s.maybe_decrypt(r, s.session_key_bytes()).await
        };

        if let Some(code) = send_resp.get("error_code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = send_resp
                    .get("error_message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(crate::session::SessionError::new(code, msg).into());
            }
        }

        let final_thumb = if !thumb_url.is_empty() {
            thumb_url
        } else {
            format!("local://{}", file_path.display())
        };
        let final_url = if !hd_url.is_empty() {
            hd_url
        } else {
            file_path.display().to_string()
        };
        // Pre-populate `stickers_cache/thumb_<md5>.png` with the local image bytes so the GUI
        // renders the sent photo immediately without an extra network round-trip.
        let cache_key = format!("thumb_{}", crate::crypto::md5_hex(&final_thumb));
        let _ = std::fs::write(
            crate::paths::cache_dir().join(format!("{cache_key}.png")),
            &file_bytes,
        );

        let msg_id = send_resp
            .get("data")
            .and_then(|d| pick_str(d, &["msgId"]))
            .unwrap_or_else(|| format!("out_{client_id}"));
        let now_str = crate::ui::clock();
        let display_text = if !caption.trim().is_empty() {
            caption.trim().to_string()
        } else {
            format!("[Hình ảnh] {file_name}")
        };
        let cm = ChatMessage {
            id: msg_id,
            client_message_id: client_id.to_string(),
            sender: "Tôi".into(),
            text: display_text.clone(),
            time: now_str,
            is_outgoing: true,
            ts: client_id as i64,
            link_url: Some(final_url),
            link_title: Some(format!(
                "{file_name} ({width}×{height} · {})",
                format_file_size(total_size)
            )),
            link_desc: if caption.trim().is_empty() {
                None
            } else {
                Some(caption.trim().to_string())
            },
            link_thumb: Some(final_thumb),
            link_src: Some("Hình ảnh Zalo".into()),
            ..Default::default()
        };
        return Ok((send_resp, cm));
    }

    // Video / Audio / Document / Archive upload (`asyncfile/upload` -> `asyncfile/msg`, fallback `sharefile_full`)
    let mime = match ext.as_str() {
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "txt" | "md" | "log" => "text/plain",
        _ => "application/octet-stream",
    };
    let checksum = md5_hex_bytes(&file_bytes);
    let upload_ep = if is_group {
        "/api/group/asyncfile/upload"
    } else {
        "/api/message/asyncfile/upload"
    };

    let mut last_up_dec = Value::Null;
    for (idx, chunk) in file_bytes.chunks(chunk_size).enumerate() {
        let chunk_id = idx + 1;
        let mut m = serde_json::Map::new();
        m.insert("totalChunk".into(), json!(total_chunks));
        m.insert("fileName".into(), json!(file_name));
        m.insert("clientId".into(), json!(client_id));
        m.insert("totalSize".into(), json!(total_size));
        m.insert("imei".into(), json!(s.imei));
        m.insert("isE2EE".into(), json!(0));
        m.insert("chunkId".into(), json!(chunk_id));
        if is_group {
            m.insert("grid".into(), json!(clean_target));
        } else {
            m.insert("toid".into(), json!(clean_target));
        }
        let params = s.encoded_params(&Value::Object(m))?;
        let url = format!(
            "{file_host}{upload_ep}?{}&params={params}&type={target_type}",
            s.common_params()
        );
        let up_raw = s
            .post_multipart(&url, "chunkContent", &file_name, mime, chunk)
            .await
            .map_err(|e| anyhow::anyhow!("tải lên phần {chunk_id}/{total_chunks} thất bại: {e}"))?;
        last_up_dec = s.maybe_decrypt(up_raw, s.session_key_bytes()).await;
        if let Some(code) = last_up_dec.get("error_code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = last_up_dec
                    .get("error_message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(crate::session::SessionError::new(code, msg).into());
            }
        }
    }

    let data_node = last_up_dec.get("data").unwrap_or(&last_up_dec);
    let file_id = pick_str(data_node, &["fileId", "clientFileId"]).filter(|id| id != "-1");
    let mut file_url = pick_str(data_node, &["fileUrl", "url", "normalUrl"]).unwrap_or_default();
    if file_url.is_empty() && FILE_DONE_LISTENER_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(fid) = file_id.as_deref() {
            match wait_for_file_done(fid, FILE_DONE_TIMEOUT).await {
                Some(url) => file_url = url,
                None => crate::logger::log_error(
                    "chat::send_attachment",
                    &format!("no file_done event for fileId {fid} within {FILE_DONE_TIMEOUT:?}"),
                ),
            }
        }
    }

    let mut send_resp = if let Some(fid) = file_id {
        let msg_ep = if is_group {
            "/api/group/asyncfile/msg"
        } else {
            "/api/message/asyncfile/msg"
        };
        let fid_val = fid
            .parse::<u64>()
            .map(|n| json!(n))
            .unwrap_or_else(|_| json!(fid));
        let mut msg_obj = serde_json::Map::new();
        msg_obj.insert("fileId".into(), fid_val);
        if !file_url.is_empty() {
            msg_obj.insert("fileUrl".into(), json!(file_url));
        }
        msg_obj.insert("checksum".into(), json!(checksum));
        msg_obj.insert("checksumSha".into(), json!(""));
        msg_obj.insert("extension".into(), json!(ext));
        msg_obj.insert("totalSize".into(), json!(total_size));
        msg_obj.insert("fileName".into(), json!(file_name));
        msg_obj.insert("clientId".into(), json!(client_id));
        msg_obj.insert("fType".into(), json!(if is_video { 2 } else { 1 }));
        msg_obj.insert("fileCount".into(), json!(0));
        msg_obj.insert("fdata".into(), json!("{}"));
        msg_obj.insert("imei".into(), json!(s.imei));
        if is_group {
            msg_obj.insert("grid".into(), json!(clean_target));
        } else {
            msg_obj.insert("toid".into(), json!(clean_target));
        }
        let params = s.encoded_params(&Value::Object(msg_obj))?;
        let url = format!("{file_host}{msg_ep}?{}", s.common_params());
        let r = s.post_form(&url, format!("params={params}")).await?;
        s.maybe_decrypt(r, s.session_key_bytes()).await
    } else {
        // No `fileId`: fall through to the single-shot `sharefile_full` path below. A made-up
        // `{"error_code": -1}` envelope used to stand in here, which made the fallback look
        // like a server verdict instead of a missing upload handle.
        crate::logger::log_error(
            "chat::send_attachment",
            "asyncfile/upload không trả về fileId; thử sharefile_full",
        );
        Value::Null
    };

    if send_resp
        .get("error_code")
        .and_then(Value::as_i64)
        .unwrap_or(-1)
        != 0
    {
        // Fallback to single-shot `/api/message/sharefile_full` or `/api/group/sharefile_full`
        let share_ep = if is_group {
            "/api/group/sharefile_full"
        } else {
            "/api/message/sharefile_full"
        };
        let body = if is_group {
            obj(&[
                ("grid", json!(clean_target)),
                ("clientId", json!(client_id)),
            ])
        } else {
            obj(&[
                ("toid", json!(clean_target)),
                ("clientId", json!(client_id)),
            ])
        };
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{file_host}{share_ep}?{}&params={params}&type={target_type}",
            s.common_params()
        );
        let r = s
            .post_multipart(&url, "fileContent", &file_name, mime, &file_bytes)
            .await?;
        send_resp = s.maybe_decrypt(r, s.session_key_bytes()).await;
    }

    match reply_error(&send_resp) {
        Some(err) => {
            crate::logger::log_error(
                "chat::send_attachment",
                &format!("tệp: mã {}: {}", err.0, err.1),
            );
            return Err(failed("Gửi tệp", err));
        }
        // Neither `asyncfile/msg` nor `sharefile_full` gave a verdict: the file is not on the
        // server, so do not hand the caller a message id we cannot point at. A reply that carries
        // a `data` object is accepted even without `error_code` — some servers answer with the
        // upload handle only.
        None if send_resp.get("error_code").is_none() && send_resp.get("data").is_none() => {
            crate::logger::log_error(
                "chat::send_attachment",
                "không có mã kết quả sau sharefile_full",
            );
            anyhow::bail!("Gửi tệp thất bại (máy chủ không trả về mã kết quả)");
        }
        None => {}
    }

    let final_url = if !file_url.is_empty() {
        file_url
    } else {
        send_resp
            .get("data")
            .and_then(|d| pick_str(d, &["fileUrl", "url", "href"]))
            .unwrap_or_default()
    };
    let msg_id = send_resp
        .get("data")
        .and_then(|d| pick_str(d, &["msgId"]))
        .unwrap_or_else(|| format!("out_{client_id}"));
    let kind_label = if is_video { "Video" } else { "Tập tin" };
    let display_text = if !caption.trim().is_empty() {
        caption.trim().to_string()
    } else {
        format!("[{kind_label}] {file_name}")
    };
    let cm = ChatMessage {
        id: msg_id,
        client_message_id: client_id.to_string(),
        sender: "Tôi".into(),
        text: display_text,
        time: crate::ui::clock(),
        is_outgoing: true,
        ts: client_id as i64,
        link_url: if final_url.is_empty() {
            None
        } else {
            Some(final_url)
        },
        link_title: Some(format!("{file_name} ({})", format_file_size(total_size))),
        link_desc: if caption.trim().is_empty() {
            None
        } else {
            Some(caption.trim().to_string())
        },
        link_src: Some(format!("{kind_label} Zalo")),
        ..Default::default()
    };
    Ok((send_resp, cm))
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct LinkMetadata {
    pub url: String,
    pub title: String,
    pub desc: String,
    pub thumb: String,
    pub src: String,
}

/// Extract the first `https://` or `http://` URL from a message text string.
pub fn extract_first_url(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let pos = trimmed
        .find("https://")
        .or_else(|| trimmed.find("http://"))?;
    let after = &trimmed[pos..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let url = after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';'));
    if url.len() >= 10 {
        Some(url.to_string())
    } else {
        None
    }
}

fn unescape_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

fn extract_meta_attr(tag: &str, attr_name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    for pat in [
        format!("{attr_name}=\""),
        format!("{attr_name}='"),
        format!("{attr_name} = \""),
    ] {
        if let Some(idx) = lower.find(&pat) {
            let quote_char = pat.chars().last().unwrap_or('"');
            let rest = &tag[idx + pat.len()..];
            if let Some(end) = rest.find(quote_char) {
                let val = unescape_html_entities(&rest[..end]);
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }
    None
}

/// Largest HTML prefix read when scraping OpenGraph tags.
const OG_MAX_BYTES: usize = 256 * 1024;
/// Redirect hops followed (each one re-validated) when scraping OpenGraph tags.
const OG_MAX_REDIRECTS: usize = 4;

/// `true` for globally routable addresses; `false` for loopback, private (RFC 1918), CGNAT,
/// link-local, unique-local (fc00::/7), multicast, documentation and unspecified ranges.
pub fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (o[1] & 0xC0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xFE) == 18))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg0 & 0xfe00) == 0xfc00
                || (seg0 & 0xffc0) == 0xfe80
                || seg0 == 0x2001 && v6.segments()[1] == 0x0db8)
        }
    }
}

/// Resolve an `https://` URL's host and return it with one address — only when *every*
/// resolved address is public. The caller pins that address so a second DNS answer cannot
/// redirect the request into the local network (DNS rebinding).
async fn resolve_public_https(url: &reqwest::Url) -> Option<(String, std::net::SocketAddr)> {
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default()?;
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        return is_public_ip(ip).then(|| (host.clone(), std::net::SocketAddr::new(ip, port)));
    }
    let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .ok()?
        .collect();
    if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
        return None;
    }
    Some((host, addrs[0]))
}

/// Scrape OpenGraph / Twitter Card / HTML `<meta>` tags directly from a webpage URL.
///
/// This request leaves from the user's own machine, so it is only used for links the user is
/// sending (see [`parse_link_metadata`]). Hardened: `https://` only, every hop (redirects are
/// followed manually) must resolve to public addresses only, the resolved address is pinned,
/// and at most [`OG_MAX_BYTES`] of the body are read.
pub async fn scrape_opengraph_metadata(url: &str) -> Option<LinkMetadata> {
    let mut current = reqwest::Url::parse(url.trim()).ok()?;
    let mut final_resp = None;
    for _ in 0..=OG_MAX_REDIRECTS {
        let (host, addr) = resolve_public_https(&current).await?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(3800))
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, addr)
            .build()
            .ok()?;
        let resp = client
            .get(current.clone())
            .header(
                "User-Agent",
                "Mozilla/5.0 (compatible; Discordbot/2.0; +https://discordapp.com)",
            )
            .header("Accept-Language", "vi-VN,vi;q=0.9,en-US;q=0.8")
            .send()
            .await
            .ok()?;
        if resp.status().is_redirection() {
            let loc = resp
                .headers()
                .get(reqwest::header::LOCATION)?
                .to_str()
                .ok()?
                .to_string();
            current = current.join(&loc).ok()?;
            continue;
        }
        final_resp = Some(resp);
        break;
    }
    let mut resp = final_resp?;
    if !resp.status().is_success() {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(16 * 1024);
    loop {
        match resp.chunk().await {
            Ok(Some(chunk)) => {
                let room = OG_MAX_BYTES - bytes.len();
                bytes.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if bytes.len() >= OG_MAX_BYTES {
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    let html = String::from_utf8_lossy(&bytes);

    let mut og_title = String::new();
    let mut og_desc = String::new();
    let mut og_thumb = String::new();
    let mut og_site = String::new();

    let mut cursor = html.as_ref();
    while let Some(start) = cursor.find('<') {
        let after = &cursor[start..];
        let Some(end) = after.find('>') else {
            break;
        };
        let tag = &after[..=end];
        cursor = &after[end + 1..];

        if tag.len() >= 5 && tag[1..5].eq_ignore_ascii_case("meta") {
            let key = extract_meta_attr(tag, "property")
                .or_else(|| extract_meta_attr(tag, "name"))
                .unwrap_or_default()
                .to_ascii_lowercase();
            let Some(content) = extract_meta_attr(tag, "content") else {
                continue;
            };
            match key.as_str() {
                "og:title" | "twitter:title" | "lark:url:video_title" if og_title.is_empty() => {
                    og_title = content;
                }
                "og:description" | "twitter:description" | "description" if og_desc.is_empty() => {
                    og_desc = content;
                }
                "og:image" | "twitter:image" | "lark:url:video_cover_image_url"
                    if og_thumb.is_empty() && content.starts_with("https://") =>
                {
                    og_thumb = content;
                }
                "og:site_name" if og_site.is_empty() => {
                    og_site = content;
                }
                _ => {}
            }
        } else if og_title.is_empty() && tag.len() >= 6 && tag[1..6].eq_ignore_ascii_case("title") {
            if let Some(close) = cursor.find("</") {
                let raw_t = unescape_html_entities(&cursor[..close]);
                if !raw_t.is_empty() {
                    og_title = raw_t;
                }
            }
        }
    }

    let domain = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("Liên kết")
        .to_string();
    Some(LinkMetadata {
        url: url.to_string(),
        title: og_title,
        desc: og_desc,
        thumb: og_thumb,
        src: if !og_site.is_empty() { og_site } else { domain },
    })
}

fn is_placeholder_zalo_thumb(thumb: &str) -> bool {
    thumb.is_empty()
        || thumb.contains("feed_thumb_link")
        || thumb.contains("im_parselink")
        || thumb.contains("aHR0cHM6Ly9yZXMtemFsby56YWRuLnZuL3VwbG9hZC9tZWRpYS8yMDE5LzEwLzE1L2ZlZWRfdGh1bWJfbGlua19fMV9fMTU3MTEzMzEyMjc3OF85Mzc4MC5wbmc=")
}

/// Parse rich link metadata (`title`, `desc`, `thumb`, `src`) via Zalo's `/api/message/parselink`
/// (`cmd 11285`) combined with direct OpenGraph HTML scraping fallback.
///
/// `allow_direct_fetch` controls the OpenGraph fallback that requests the page **from the
/// user's machine**. Pass `false` for links in received messages (and for any automatic
/// preview): only Zalo's server-side `parselink` is used, so a sender cannot learn the user's
/// IP address or probe the local network by sending a link. Pass `true` only for a link the
/// user is sending themselves.
pub async fn parse_link_metadata(
    s: &Session,
    raw_url: &str,
    allow_direct_fetch: bool,
) -> Result<LinkMetadata> {
    let url = raw_url.trim().to_string();
    let domain = url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("Liên kết")
        .to_string();
    let mut meta = LinkMetadata {
        url: url.clone(),
        title: String::new(),
        desc: String::new(),
        thumb: String::new(),
        src: domain.clone(),
    };

    if s.login.is_some() {
        let file_host = if s.domains.file.is_empty() {
            "https://tt-files-wpa.chat.zalo.me"
        } else {
            s.domains.file.trim_end_matches('/')
        };
        let body = obj(&[
            ("link", json!(url)),
            ("version", json!(1)),
            ("imei", json!(s.imei)),
        ]);
        if let Ok(params) = s.encoded_params(&body) {
            let parse_url = format!(
                "{file_host}/api/message/parselink?{}&params={params}",
                s.common_params()
            );
            if let Ok(resp) = s.get(&parse_url).await {
                let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
                let d = dec
                    .get("data")
                    .and_then(|x| x.get("data").or(Some(x)))
                    .unwrap_or(&dec);
                if let Some(t) = d.get("title").and_then(Value::as_str).map(str::trim) {
                    if !t.is_empty() && t != url {
                        meta.title = t.to_string();
                    }
                }
                if let Some(ds) = d.get("desc").and_then(Value::as_str).map(str::trim) {
                    if !ds.is_empty() && ds != url {
                        meta.desc = ds.to_string();
                    }
                }
                if let Some(th) = d.get("thumb").and_then(Value::as_str).map(str::trim) {
                    if !is_placeholder_zalo_thumb(th) {
                        meta.thumb = th.to_string();
                    }
                }
                if let Some(sr) = d.get("src").and_then(Value::as_str).map(str::trim) {
                    if !sr.is_empty() {
                        meta.src = sr.to_string();
                    }
                }
            }
        }
    }

    if allow_direct_fetch
        && (meta.title.is_empty() || meta.thumb.is_empty() || meta.desc.is_empty())
    {
        if let Some(og) = scrape_opengraph_metadata(&url).await {
            if meta.title.is_empty() && !og.title.is_empty() {
                meta.title = og.title;
            }
            if meta.desc.is_empty() && !og.desc.is_empty() {
                meta.desc = og.desc;
            }
            if meta.thumb.is_empty() && !og.thumb.is_empty() {
                meta.thumb = og.thumb;
            }
            if (meta.src.is_empty() || meta.src == domain) && !og.src.is_empty() {
                meta.src = og.src;
            }
        }
    }

    if meta.title.is_empty() {
        meta.title = domain;
    }
    Ok(meta)
}

/// Send a rich link message (`sendLinkMessage` @ `compact-app-pc.js:14319196`):
/// - 1-1 (`cmd 11286`): `POST {domains.chat}/api/message/link?{common}`
/// - Group (`cmd 11193`): `POST {domains.group}/api/group/sendlink?{common}`
pub async fn send_link_message(
    s: &Session,
    to_id: &str,
    text: &str,
    meta: &LinkMetadata,
    is_group: bool,
) -> Result<Value> {
    send_link_message_with_client_id(s, to_id, text, meta, is_group, next_client_message_id()).await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn send_link_message_with_client_id(
    s: &Session,
    to_id: &str,
    text: &str,
    meta: &LinkMetadata,
    is_group: bool,
    now: u64,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let msg_str = if text.trim().is_empty() {
        meta.url.as_str()
    } else {
        text.trim()
    };
    let title_str = if meta.title.trim().is_empty() {
        meta.url.as_str()
    } else {
        meta.title.trim()
    };
    let media_json = json!({
        "type": 0,
        "count": 0,
        "mediaTitle": "",
        "artist": "",
        "streamUrl": "",
        "stream_icon": ""
    })
    .to_string();

    let res: Result<Value> = async {
        if is_group {
            let clean_grid = to_id
                .trim_start_matches("group_")
                .trim_start_matches('g')
                .trim_start_matches('G');
            let body = obj(&[
                ("grid", json!(clean_grid)),
                ("msg", json!(msg_str)),
                ("href", json!(meta.url)),
                ("src", json!(meta.src)),
                ("title", json!(title_str)),
                ("desc", json!(meta.desc)),
                ("thumb", json!(meta.thumb)),
                ("type", json!(0)),
                ("media", json!(media_json)),
                ("clientId", json!(now)),
                ("mentionInfo", json!("[]")),
                ("language", json!("vi")),
                ("ttl", json!(0)),
                ("imei", json!(s.imei)),
            ]);
            let params = s.encoded_params(&body)?;
            let url = format!(
                "{}/api/group/sendlink?{}",
                s.domains.group,
                s.common_params()
            );
            let resp = s.post_form(&url, format!("params={params}")).await?;
            let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            // Carry the server's own reason (group variant): the user sees it, the wrapper logs it.
            if let Some(err) = reply_error(&dec) {
                return Err(failed("Gửi tin kèm liên kết", err));
            }
            Ok(dec)
        } else {
            let host = if s.domains.chat.contains("-msg-chat.")
                || s.domains.chat.contains("tt-chat2-wpa")
                || s.domains.chat.is_empty()
            {
                "https://tt-chat3-wpa.api.zaloapp.com"
            } else {
                &s.domains.chat
            };
            let body = obj(&[
                ("toId", json!(to_id)),
                ("msg", json!(msg_str)),
                ("href", json!(meta.url)),
                ("src", json!(meta.src)),
                ("title", json!(title_str)),
                ("desc", json!(meta.desc)),
                ("thumb", json!(meta.thumb)),
                ("type", json!(0)),
                ("media", json!(media_json)),
                ("clientId", json!(now)),
                ("mentionInfo", json!("")),
                ("language", json!("vi")),
                ("ttl", json!(0)),
                ("imei", json!(s.imei)),
            ]);
            let params = s.encoded_params(&body)?;
            let url = format!("{host}/api/message/link?{}", s.common_params());
            let resp = s.post_form(&url, format!("params={params}")).await?;
            let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            // Carry the server's own reason (1-1 variant): the user sees it, the wrapper logs it.
            if let Some(err) = reply_error(&dec) {
                return Err(failed("Gửi tin kèm liên kết", err));
            }
            Ok(dec)
        }
    }
    .await;

    match res {
        Ok(v) => Ok(v),
        Err(e) => {
            // Degrading the card to a plain-text message looked like success while the thing
            // the user asked to send (the link card) was never sent (`re-notes/10` §3).
            crate::logger::log_error("chat::send_link_message", &format!("{e:#}"));
            Err(e.context("Không gửi được tin kèm liên kết"))
        }
    }
}

/// Send a sticker (`sendSticker` in `compact-app-pc.b7bec208c4c8f73e09f8.js:14325410`):
/// - 1-1 (`cmd 11154`): `POST {domains.chat}/api/message/sticker?{common}`
/// - Group (`cmd 11165`): `POST {domains.group}/api/group/sticker?{common}`
pub async fn send_sticker(
    s: &Session,
    to_id: &str,
    sticker_id: u64,
    cate_id: u64,
    sticker_type: u32,
    is_group: bool,
) -> Result<Value> {
    send_sticker_with_client_id(
        s,
        to_id,
        sticker_id,
        cate_id,
        sticker_type,
        is_group,
        next_client_message_id(),
    )
    .await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn send_sticker_with_client_id(
    s: &Session,
    to_id: &str,
    sticker_id: u64,
    cate_id: u64,
    sticker_type: u32,
    is_group: bool,
    now: u64,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }

    let url = if is_group {
        let clean_grid = to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        let body = obj(&[
            ("grid", json!(clean_grid)),
            ("stickerId", json!(sticker_id)),
            ("cateId", json!(cate_id)),
            ("type", json!(sticker_type)),
            ("clientId", json!(now)),
            ("imei", json!(s.imei)),
            ("zsource", json!(101)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{}/api/group/sticker?{}",
            s.domains.group,
            s.common_params()
        );
        let form = format!("params={params}");
        let resp = s.post_form(&url, form).await?;
        let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
        if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
            if code != 0 {
                let msg = resp
                    .get("error_message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(crate::session::SessionError::new(code, msg).into());
            }
        }
        return Ok(resp);
    } else {
        let host = if s.domains.chat.contains("-msg-chat.")
            || s.domains.chat.contains("tt-chat2-wpa")
            || s.domains.chat.is_empty()
        {
            "https://tt-chat3-wpa.api.zaloapp.com"
        } else {
            &s.domains.chat
        };
        format!("{host}/api/message/sticker?{}", s.common_params())
    };

    let body = obj(&[
        ("toid", json!(to_id)),
        ("stickerId", json!(sticker_id)),
        ("cateId", json!(cate_id)),
        ("type", json!(sticker_type)),
        ("clientId", json!(now)),
        ("imei", json!(s.imei)),
        ("zsource", json!(101)),
    ]);
    let params = s.encoded_params(&body)?;
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = resp.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let msg = resp
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(crate::session::SessionError::new(code, msg).into());
        }
    }
    Ok(resp)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StickerPackInfo {
    pub id: u64,
    pub name: String,
    pub total_image: u32,
    pub icon_url: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StickerItem {
    pub sticker_id: u64,
    pub cate_id: u64,
    pub sticker_type: u32,
    pub label: String,
}

/// Fetch the complete Zalo Sticker Store catalog (`getStickerCatsV2` -> `getAllCate` in `compact-app-pc.js:14288962`).
///
/// Returns ~390 official Zalo sticker packs sorted by newest/featured first and caches them to
/// `stickers_cache/store_packs.json`.
pub async fn fetch_sticker_store_packs(s: &Session) -> Result<Vec<StickerPackInfo>> {
    let cache_dir_buf = crate::paths::cache_dir();
    let cache_dir = cache_dir_buf.as_path();
    let _ = std::fs::create_dir_all(cache_dir);
    let cache_path = cache_dir.join("store_packs.json");
    if let Ok(raw_cache) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<Vec<StickerPackInfo>>(&raw_cache) {
            if cached.len() >= 20 {
                return Ok(cached);
            }
        }
    }

    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let sticker_host = if s.domains.sticker.is_empty() {
        "https://tt-sticker-wpa.chat.zalo.me"
    } else {
        s.domains.sticker.trim_end_matches('/')
    };
    let key = s.session_key_bytes();
    if key.is_empty() {
        anyhow::bail!("thiếu session key để tải kho nhãn dán");
    }
    let enc_empty = crate::crypto::encode_aes(key, "")?;
    let mut encoded_param = String::with_capacity(enc_empty.len() + 16);
    for b in enc_empty.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded_param.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(encoded_param, "%{b:02X}");
            }
        }
    }

    let list_url = format!(
        "{sticker_host}/api/message/sticker/category/list/v2?{}&params={encoded_param}",
        s.common_params()
    );
    let resp = s.get(&list_url).await?;
    let dec = s.maybe_decrypt(resp, key).await;
    let data = dec.get("data").unwrap_or(&dec);

    let ex_ids: std::collections::HashSet<u64> = data
        .get("exCateIds")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_u64).collect())
        .unwrap_or_default();

    let data_url = data
        .get("dataUrl")
        .and_then(Value::as_str)
        .unwrap_or("https://zalo-api.zadn.vn/api/emoticon/pcweb/getAllCate")
        .replace("http://", "https://");

    let cdn_resp = s.http.get(&data_url).send().await?;
    let raw_bytes = cdn_resp.bytes().await?;
    let json_text = if raw_bytes.len() >= 2 && raw_bytes[0] == 0x1f && raw_bytes[1] == 0x8b {
        use std::io::Read;
        let mut gz = flate2::read::GzDecoder::new(&raw_bytes[..]);
        let mut out = String::new();
        gz.read_to_string(&mut out)?;
        out
    } else {
        String::from_utf8_lossy(&raw_bytes).into_owned()
    };

    let arr: Vec<Value> = serde_json::from_str(&json_text)?;
    let mut packs = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(id) = item.get("id").and_then(Value::as_u64) else {
            continue;
        };
        if id == 0 || ex_ids.contains(&id) {
            continue;
        }
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if name.is_empty() || name.to_lowercase().contains("test") {
            continue;
        }
        let total_image = item.get("totalImage").and_then(Value::as_u64).unwrap_or(16) as u32;
        let icon_url = item
            .get("iconUrl")
            .or_else(|| item.get("thumbImg"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace("http://", "https://");
        packs.push(StickerPackInfo {
            id,
            name,
            total_image,
            icon_url,
        });
    }

    // Sort newest packs first (`id` descending) so modern Zalo packs appear at the top of the store
    packs.sort_by(|a, b| b.id.cmp(&a.id));
    if let Ok(txt) = serde_json::to_string(&packs) {
        let _ = std::fs::write(&cache_path, txt);
    }
    Ok(packs)
}

/// Fetch all stickers belonging to a specific category (`getStickerCategoryDetail` — `cmd 11284`).
pub async fn fetch_sticker_pack_items(
    s: &Session,
    cate_id: u64,
    pack_name: &str,
) -> Result<Vec<StickerItem>> {
    if cate_id == 0 {
        return Ok((1..=30)
            .map(|id| StickerItem {
                sticker_id: id,
                cate_id: 0,
                sticker_type: 7,
                label: format!("Củ hành #{id}"),
            })
            .collect());
    }

    let cache_dir_buf = crate::paths::cache_dir();
    let cache_dir = cache_dir_buf.as_path();
    let _ = std::fs::create_dir_all(cache_dir);
    let cache_path = cache_dir.join(format!("pack_{cate_id}.json"));
    if let Ok(raw_cache) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<Vec<StickerItem>>(&raw_cache) {
            if !cached.is_empty() {
                return Ok(cached);
            }
        }
    }

    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let sticker_host = if s.domains.sticker.is_empty() {
        "https://tt-sticker-wpa.chat.zalo.me"
    } else {
        s.domains.sticker.trim_end_matches('/')
    };
    let params = s.encoded_params(&json!({ "cid": cate_id.to_string() }))?;
    let url = format!(
        "{sticker_host}/api/message/sticker/category/sticker_detail?{}&params={params}",
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    let arr = dec
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| dec.as_array())
        .cloned()
        .unwrap_or_default();

    let mut items = Vec::with_capacity(arr.len());
    for (idx, it) in arr.iter().enumerate() {
        let Some(sid) = it
            .get("id")
            .or_else(|| it.get("stickerId"))
            .and_then(Value::as_u64)
        else {
            continue;
        };
        let cid = it.get("cateId").and_then(Value::as_u64).unwrap_or(cate_id);
        let stype = it.get("type").and_then(Value::as_u64).unwrap_or(7) as u32;
        items.push(StickerItem {
            sticker_id: sid,
            cate_id: cid,
            sticker_type: stype,
            label: format!("{pack_name} #{}", idx + 1),
        });
    }

    if !items.is_empty() {
        if let Ok(txt) = serde_json::to_string(&items) {
            let _ = std::fs::write(&cache_path, txt);
        }
    }
    Ok(items)
}

/// Normalize a sticker keyword by converting non-breaking spaces (`\u{a0}`), collapsing
/// consecutive whitespace to a single ASCII space, and converting to lowercase.
pub fn normalize_sticker_keyword(s: &str) -> String {
    s.replace('\u{a0}', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Strip Vietnamese diacritics to lowercase ASCII (`simpleStripVietnamese` in `compact-app-pc.js:12955017`).
pub fn strip_vn_for_sticker_kw(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for raw in s.chars() {
        for c in raw.to_lowercase() {
            let mapped = match c {
                'à' | 'á' | 'ạ' | 'ả' | 'ã' | 'â' | 'ầ' | 'ấ' | 'ậ' | 'ẩ' | 'ẫ' | 'ă' | 'ằ'
                | 'ắ' | 'ặ' | 'ẳ' | 'ẵ' => 'a',
                'è' | 'é' | 'ẹ' | 'ẻ' | 'ẽ' | 'ê' | 'ề' | 'ế' | 'ệ' | 'ể' | 'ễ' => {
                    'e'
                }
                'ì' | 'í' | 'ị' | 'ỉ' | 'ĩ' => 'i',
                'ò' | 'ó' | 'ọ' | 'ỏ' | 'õ' | 'ô' | 'ồ' | 'ố' | 'ộ' | 'ổ' | 'ỗ' | 'ơ' | 'ờ'
                | 'ớ' | 'ợ' | 'ở' | 'ỡ' => 'o',
                'ù' | 'ú' | 'ụ' | 'ủ' | 'ũ' | 'ư' | 'ừ' | 'ứ' | 'ự' | 'ử' | 'ữ' => {
                    'u'
                }
                'ỳ' | 'ý' | 'ỵ' | 'ỷ' | 'ỹ' => 'y',
                'đ' => 'd',
                '\u{a0}' => ' ',
                other => other,
            };
            out.push(mapped);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Map common unaccented Vietnamese keywords, English terms, or chat slang to the accented
/// Vietnamese form indexed by Zalo's `/api/message/sticker/suggest/stickers` (`cmd 11741`) and
/// `/api/message/sticker/search` (`cmd 11747`) endpoints.
pub fn canonical_accented_vn_for_sticker(stripped: &str) -> Option<&'static str> {
    match stripped {
        "nho" | "nho em" | "nho anh" | "nho qua" | "miss" | "miss u" | "miss you" => Some("nhớ"),
        "khoc" | "huhu" | "huhuhu" | "hic" | "cry" => Some("khóc"),
        "buon" | "buon qua" | "sad" | "tui than" => Some("buồn"),
        "vui" | "vui ve" | "vui qua" | "happy" => Some("vui"),
        "cuoi" | "mac cuoi" | "buon cuoi" => Some("cười"),
        "yeu" | "iu" | "yeu em" | "yeu anh" | "iu em" | "iu anh" | "love" | "love u"
        | "love you" | "tha tim" | "tim" => Some("yêu"),
        "thuong" | "thuong em" | "thuong anh" | "thuong qua" => Some("thương"),
        "gian" | "gian doi" | "doi hon" | "tuc" | "buc" | "buc minh" | "cay" | "angry" => {
            Some("giận")
        }
        "met" | "met qua" | "met moi" | "duoi" | "duoi qua" | "tired" => Some("mệt"),
        "doi" | "doi bung" | "doi qua" | "hungry" => Some("đói"),
        "an" | "an com" | "di an" => Some("ăn cơm"),
        "ca phe" | "cf" | "cafe" | "coffee" => Some("cà phê"),
        "tra sua" | "trasua" => Some("trà sữa"),
        "nhau" | "di nhau" | "bia" | "dzoo" | "dzo" => Some("nhậu"),
        "ngu" | "di ngu" | "buon ngu" | "ngu ngon" | "ngu ngon nhe" | "ngu ngon nha"
        | "good night" | "goodnight" | "gn" | "g9" => Some("ngủ"),
        "chao" | "xin chao" | "chao ban" | "chao anh" | "chao chi" | "chao em" | "he lo"
        | "helo" => Some("xin chào"),
        "cam on" | "cam on ban" | "cam on anh" | "cam on chi" | "cam on em" | "cam on nhieu"
        | "thank" | "thanks" | "thank you" | "tks" | "tkss" | "thank kiu" => Some("cảm ơn"),
        "xin loi" | "xin loi nha" | "xin loi nhe" | "sorry" | "sr" | "so ry" => Some("xin lỗi"),
        "tam biet" | "tam biet nhe" | "bye" | "byebye" | "good bye" | "goodbye" | "bibi" | "pp" => {
            Some("tạm biệt")
        }
        "chuc mung" | "happy birthday" | "hpbd" | "sinh nhat" | "cmsn" => Some("chúc mừng"),
        "tet" | "chuc mung nam moi" | "nam moi" => Some("tết"),
        "co len" | "co gang" | "fighting" => Some("cố lên"),
        "tuyet" | "tuyet voi" | "xuat sac" | "dinh" | "dinh qua" | "chuan" | "duoc" => {
            Some("tuyệt vời")
        }
        "ngai" | "ngai qua" | "ngai nhi" | "xau ho" => Some("ngại"),
        "so" | "so qua" | "scared" => Some("sợ"),
        "lanh" | "lanh qua" | "lanh khong" | "ret" | "ret qua" => Some("lạnh"),
        "nong" | "nong qua" | "nong buc" => Some("nóng"),
        "tien" | "luong" => Some("tiền"),
        "het tien" | "ngheo" | "ngheo qua" | "vi rong" => Some("hết tiền"),
        "quy" | "lay" | "xin" | "van xin" => Some("lạy"),
        "ban" | "ban qua" | "busy" => Some("bận"),
        "chan" | "chan qua" | "chan doi" | "boring" => Some("chán"),
        "cho" | "cho chut" | "cho xiu" | "doi chut" | "doi xiu" | "wait" => Some("chờ chút"),
        "di choi" | "choi" | "dao pho" => Some("đi chơi"),
        "di lam" | "lam viec" | "deadline" | "code" => Some("làm"),
        "hong" | "hong chuyen" | "ngac nhien" => Some("ngạc nhiên"),
        _ => None,
    }
}

/// Build the deduplicated list of query variants (accented + unaccented + canonical synonym)
/// to maximize recall across both Zalo Sticker Suggest (`cmd 11741`) and Search (`cmd 11747`).
pub fn canonical_vn_sticker_queries(keyword: &str) -> Vec<String> {
    let norm = normalize_sticker_keyword(keyword);
    if norm.is_empty() {
        return Vec::new();
    }
    let stripped = strip_vn_for_sticker_kw(&norm);
    let mut queries = vec![norm.clone()];
    if !stripped.is_empty() && stripped != norm && !queries.contains(&stripped) {
        queries.push(stripped.clone());
    }
    if let Some(accented) = canonical_accented_vn_for_sticker(&stripped) {
        let acc_s = accented.to_string();
        if !queries.contains(&acc_s) {
            queries.push(acc_s.clone());
        }
        let acc_stripped = strip_vn_for_sticker_kw(&acc_s);
        if !acc_stripped.is_empty() && !queries.contains(&acc_stripped) {
            queries.push(acc_stripped);
        }
    }
    queries.truncate(4);
    queries
}

/// Fetch official Zalo contextual sticker trigger keywords (`getStickerListKeyWords` — `cmd 11740`).
/// Includes both normalized accented/raw forms and ASCII-folded forms so typing accented or
/// unaccented Vietnamese triggers suggestions reliably.
pub async fn fetch_sticker_suggest_keywords(s: &Session) -> Result<Vec<String>> {
    let cache_dir_buf = crate::paths::cache_dir();
    let cache_dir = cache_dir_buf.as_path();
    let _ = std::fs::create_dir_all(cache_dir);
    let cache_path = cache_dir.join("suggest_keywords_v2.json");
    if let Ok(raw_cache) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<Vec<String>>(&raw_cache) {
            if cached.len() >= 100 {
                return Ok(cached);
            }
        }
    }
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let sticker_host = if s.domains.sticker.is_empty() {
        "https://tt-sticker-wpa.chat.zalo.me"
    } else {
        s.domains.sticker.trim_end_matches('/')
    };
    let params = s.encoded_params(&json!({ "imei": s.imei }))?;
    let url = format!(
        "{sticker_host}/api/message/sticker/suggest/keywords?{}&params={params}",
        s.common_params()
    );
    let resp = match s.get(&url).await {
        Ok(r) => r,
        Err(_) => {
            let fallback_url = format!(
                "https://tt-sticker-wpa.zaloapp.com/api/message/sticker/suggest/keywords?{}&params={params}",
                s.common_params()
            );
            s.get(&fallback_url).await?
        }
    };
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    let mut seen = std::collections::HashSet::new();
    let mut kws: Vec<String> = Vec::new();
    if let Some(arr) = dec
        .get("data")
        .or(Some(&dec))
        .and_then(|d| d.get("keywords"))
        .and_then(Value::as_array)
    {
        for raw_v in arr {
            let Some(raw_str) = raw_v.as_str() else {
                continue;
            };
            let norm = normalize_sticker_keyword(raw_str);
            if norm.is_empty() || norm.starts_with("zkwd_") {
                continue;
            }
            if seen.insert(norm.clone()) {
                kws.push(norm.clone());
            }
            let stripped = strip_vn_for_sticker_kw(&norm);
            if !stripped.is_empty() && seen.insert(stripped.clone()) {
                kws.push(stripped);
            }
        }
    }
    if !kws.is_empty() {
        if let Ok(txt) = serde_json::to_string(&kws) {
            let _ = std::fs::write(&cache_path, txt);
        }
    }
    Ok(kws)
}

async fn query_sticker_endpoints_once(
    s: &Session,
    sticker_host: &str,
    queries: &[String],
) -> Vec<(u64, u64, u32, u8)> {
    let mut futs = Vec::with_capacity(queries.len() * 2);
    for (q_idx, q) in queries.iter().enumerate() {
        // 1) Contextual suggest endpoint (`getStickerSuggestion` — `cmd 11741`)
        let sugg_body = obj(&[
            ("keyword", json!(q)),
            ("gif", json!(1)),
            ("guggy", json!(0)),
            ("imei", json!(s.imei)),
        ]);
        if let Ok(params) = s.encoded_params(&sugg_body) {
            let url = format!(
                "{sticker_host}/api/message/sticker/suggest/stickers?{}&params={params}",
                s.common_params()
            );
            let is_primary = q_idx == 0;
            futs.push(async move {
                let Ok(resp) = s.get(&url).await else {
                    return Vec::new();
                };
                let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
                let arr = dec
                    .get("data")
                    .or(Some(&dec))
                    .and_then(|d| d.get("sugg_sticker"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut items = Vec::with_capacity(arr.len());
                for it in arr {
                    let Some(sid) = it
                        .get("sticker_id")
                        .or_else(|| it.get("id"))
                        .and_then(Value::as_u64)
                    else {
                        continue;
                    };
                    let cid = it
                        .get("cate_id")
                        .or_else(|| it.get("cateId"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let stype = it.get("type").and_then(Value::as_u64).unwrap_or(7) as u32;
                    items.push((cid, sid, stype, if is_primary { 3u8 } else { 2u8 }));
                }
                items
            });
        }
    }

    let mut search_futs = Vec::with_capacity(queries.len());
    for (q_idx, q) in queries.iter().enumerate() {
        // 2) Full sticker tag search endpoint (`searchSticker` — `cmd 11747`)
        let search_body = obj(&[
            ("keyword", json!(q)),
            ("limit", json!(30)),
            ("srcType", json!(0)),
            ("imei", json!(s.imei)),
        ]);
        if let Ok(params) = s.encoded_params(&search_body) {
            let url = format!(
                "{sticker_host}/api/message/sticker/search?{}&params={params}",
                s.common_params()
            );
            let is_primary = q_idx == 0;
            search_futs.push(async move {
                let Ok(resp) = s.get(&url).await else {
                    return Vec::new();
                };
                let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
                let arr = dec
                    .get("data")
                    .and_then(Value::as_array)
                    .or_else(|| dec.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut items = Vec::with_capacity(arr.len());
                for it in arr {
                    let Some(sid) = it
                        .get("sticker_id")
                        .or_else(|| it.get("id"))
                        .and_then(Value::as_u64)
                    else {
                        continue;
                    };
                    let cid = it
                        .get("cate_id")
                        .or_else(|| it.get("cateId"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let stype = it.get("type").and_then(Value::as_u64).unwrap_or(7) as u32;
                    items.push((cid, sid, stype, if is_primary { 3u8 } else { 2u8 }));
                }
                items
            });
        }
    }

    let (sugg_batches, search_batches) = tokio::join!(
        futures_util::future::join_all(futs),
        futures_util::future::join_all(search_futs)
    );

    // Merge and boost stickers that matched across multiple endpoints/queries
    let mut score_map: std::collections::HashMap<u64, (u64, u32, u8)> =
        std::collections::HashMap::new();
    for batch in sugg_batches.into_iter().chain(search_batches.into_iter()) {
        for (cid, sid, stype, weight) in batch {
            score_map
                .entry(sid)
                .and_modify(|e| {
                    e.2 = e.2.saturating_add(weight);
                    if e.0 == 0 && cid > 0 {
                        e.0 = cid;
                    }
                })
                .or_insert((cid, stype, weight));
        }
    }

    score_map
        .into_iter()
        .map(|(sid, (cid, stype, score))| (cid, sid, stype, score))
        .collect()
}

/// Search Zalo stickers by Vietnamese, English, or emoji keyword by querying both
/// `/api/message/sticker/suggest/stickers` (`cmd 11741`) and `/api/message/sticker/search` (`cmd 11747`)
/// concurrently across accented and unaccented Vietnamese variants.
pub async fn search_stickers_by_keyword(s: &Session, keyword: &str) -> Result<Vec<StickerItem>> {
    let kw = normalize_sticker_keyword(keyword);
    if kw.is_empty() {
        return Ok(Vec::new());
    }
    let cache_dir_buf = crate::paths::cache_dir();
    let cache_dir = cache_dir_buf.as_path();
    let _ = std::fs::create_dir_all(cache_dir);
    let cache_path = cache_dir.join(format!("suggest_v2_{}.json", crate::crypto::md5_hex(&kw)));
    if let Ok(raw_cache) = std::fs::read_to_string(&cache_path) {
        if let Ok(cached) = serde_json::from_str::<Vec<StickerItem>>(&raw_cache) {
            if !cached.is_empty() {
                return Ok(cached);
            }
        }
    }
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let sticker_host = if s.domains.sticker.is_empty() {
        "https://tt-sticker-wpa.chat.zalo.me"
    } else {
        s.domains.sticker.trim_end_matches('/')
    };

    let queries = canonical_vn_sticker_queries(&kw);
    let mut raw_items = query_sticker_endpoints_once(s, sticker_host, &queries).await;

    // If a multi-word sentence/phrase (e.g. "anh ơi đi chơi", "hôm nay mệt quá") had no direct match,
    // automatically fallback to its trailing 2-word phrase (`tail2`) or trailing word (`last_word`).
    if raw_items.is_empty() {
        let words: Vec<&str> = kw.split_whitespace().collect();
        if words.len() >= 2 && words.len() <= 5 {
            let mut fallback_phrases = Vec::new();
            if words.len() >= 3 {
                fallback_phrases.push(format!(
                    "{} {}",
                    words[words.len() - 2],
                    words[words.len() - 1]
                ));
            }
            let last_w = words[words.len() - 1];
            if last_w.chars().count() >= 2 {
                fallback_phrases.push(last_w.to_string());
            }
            for fb in fallback_phrases {
                let fb_queries = canonical_vn_sticker_queries(&fb);
                raw_items = query_sticker_endpoints_once(s, sticker_host, &fb_queries).await;
                if !raw_items.is_empty() {
                    break;
                }
            }
        }
    }

    // Prioritize modern illustrated Zalo character packs (`cate_id` descending, matching Zalo PC's
    // `pointCate` ranking) and exclude legacy round-emoji/photo-meme packs when character packs exist.
    const LEGACY_NON_CHAR_PACKS: &[u64] = &[
        0, 50, 10788, 11190, 11984, 11998, 12002, 12003, 12024, 12658,
    ];
    let has_modern_char = raw_items
        .iter()
        .any(|(cid, _, _, _)| *cid >= 10000 && !LEGACY_NON_CHAR_PACKS.contains(cid));
    if has_modern_char {
        raw_items.retain(|(cid, _, _, _)| *cid >= 10000 && !LEGACY_NON_CHAR_PACKS.contains(cid));
    }
    // Sort by relevance score first (matched in both search + suggest), then newest pack (`cate_id` desc)
    raw_items.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| b.0.cmp(&a.0)));

    // Keep at most 2 stickers from the same pack in the first pass so suggestions show diverse characters,
    // then fill up to 24 items if needed for Sticker Picker search grids.
    let mut pack_counts: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    let mut out = Vec::with_capacity(24);
    let mut deferred = Vec::new();
    for &(cid, sid, stype, _) in &raw_items {
        let cnt = pack_counts.entry(cid).or_insert(0);
        if *cnt >= 2 {
            deferred.push((cid, sid, stype));
            continue;
        }
        *cnt += 1;
        let idx = out.len() + 1;
        out.push(StickerItem {
            sticker_id: sid,
            cate_id: cid,
            sticker_type: stype,
            label: format!("{kw} #{idx}"),
        });
        if out.len() >= 24 {
            break;
        }
    }
    for (cid, sid, stype) in deferred {
        if out.len() >= 24 {
            break;
        }
        let idx = out.len() + 1;
        out.push(StickerItem {
            sticker_id: sid,
            cate_id: cid,
            sticker_type: stype,
            label: format!("{kw} #{idx}"),
        });
    }

    if !out.is_empty() {
        if let Ok(txt) = serde_json::to_string(&out) {
            let _ = std::fs::write(&cache_path, txt);
        }
    }
    Ok(out)
}

/// `(qmsgId, qmsgCliId, qmsgTs)` of the message a quote points at.
///
/// [O] Both ids are the quoted message's **server-side** identity (`re-notes/10` §3). Older
/// builds invented them when parsing failed (`now - 1000`, and the timestamp reused as
/// `qmsgCliId`), which produced a quote the server accepted but which pointed at nothing. A row
/// without a server id (`out_` / `local_` / empty) is a message that has not been sent yet and
/// must be refused, not guessed.
///
/// `Err` message is user-facing (Vietnamese): the GUI shows it as the send failure.
fn quote_target_ids(quoted: &ChatMessage) -> Result<(u64, u64, u64)> {
    if quoted.id.is_empty()
        || quoted.id.starts_with("out_")
        || quoted.id.starts_with("local_")
    {
        anyhow::bail!("Không trích dẫn được tin này (tin chưa có mã từ máy chủ)");
    }
    let qmsg_id = quoted
        .id
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("Không trích dẫn được tin này (mã tin không hợp lệ)"))?;
    // `qmsgCliId` is the client id of the quoted message; the send time is the fallback Zalo
    // itself uses when the row predates `cliMsgId` capture. Never fabricated.
    let qmsg_cli_id = quoted
        .cli_msg_id
        .as_deref()
        .and_then(|c| c.parse::<u64>().ok())
        .or_else(|| (quoted.ts > 0).then_some(quoted.ts as u64))
        .ok_or_else(|| {
            anyhow::anyhow!("Không trích dẫn được tin này (thiếu thời điểm gửi của tin gốc)")
        })?;
    let qmsg_ts = quoted.ts.max(0) as u64;
    Ok((qmsg_id, qmsg_cli_id, qmsg_ts))
}

/// Send a quote/reply message (`sendQuoteMessage` / `sendGroupQuoteMessage` in `compact-app-pc.b7bec208c4c8f73e09f8.js:14317494`):
/// - 1-1 (`cmd 11771`): `POST {domains.chat}/api/message/quote?{common}`
/// - Group (`cmd 11197`): `POST {domains.group}/api/group/quote?{common}`
/// Refused (instead of degraded to plain text) when the quoted message has no server id.
pub async fn send_quote_message(
    s: &Session,
    to_id: &str,
    text: &str,
    quoted: &ChatMessage,
    is_group: bool,
) -> Result<Value> {
    send_quote_message_with_client_id(s, to_id, text, quoted, is_group, next_client_message_id())
        .await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn send_quote_message_with_client_id(
    s: &Session,
    to_id: &str,
    text: &str,
    quoted: &ChatMessage,
    is_group: bool,
    now: u64,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }

    let my_uid = s
        .login
        .as_ref()
        .map(|l| l.uin.clone())
        .unwrap_or_else(|| "0".into());

    let qmsg_owner = if quoted.is_outgoing {
        my_uid.as_str()
    } else if !quoted.sender.is_empty() && quoted.sender.chars().all(|c| c.is_ascii_digit()) {
        quoted.sender.as_str()
    } else if !is_group {
        to_id
    } else {
        "0"
    };

    let (qmsg_id_num, qmsg_cli_id, qmsg_ts) = quote_target_ids(quoted)?;

    let quote_res: Result<Value> = async {
        if is_group {
            let clean_grid = to_id
                .trim_start_matches("group_")
                .trim_start_matches('g')
                .trim_start_matches('G');
            let (clean_text, text_props_opt) = extract_zalo_rtf_text_properties(text);
            let mut body = obj(&[
                ("grid", json!(clean_grid)),
                ("message", json!(clean_text)),
                ("clientId", json!(now)),
                ("language", json!("vi")),
                ("qmsgOwner", json!(qmsg_owner)),
                ("qmsgId", json!(qmsg_id_num)),
                ("qmsgCliId", json!(qmsg_cli_id)),
                ("qmsgType", json!(1)),
                ("qmsg", json!(quoted.text)),
                ("qmsgTs", json!(qmsg_ts)),
                ("visibility", json!(0)),
                ("ttl", json!(0)),
                ("imei", json!(s.imei)),
                ("zsource", json!(101)),
            ]);
            if let (Some(tp), Some(map)) = (text_props_opt, body.as_object_mut()) {
                map.insert("textProperties".into(), json!(tp));
            }
            let params = s.encoded_params(&body)?;
            let url = format!("{}/api/group/quote?{}", s.domains.group, s.common_params());
            let resp = s.post_form(&url, format!("params={params}")).await?;
            let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            // Keep the server's reason (group variant); the wrapper logs it and adds context.
            if let Some(err) = reply_error(&resp) {
                return Err(failed("Gửi tin trích dẫn", err));
            }
            Ok(resp)
        } else {
            let host = if s.domains.chat.contains("-msg-chat.")
                || s.domains.chat.contains("tt-chat2-wpa")
                || s.domains.chat.is_empty()
            {
                "https://tt-chat3-wpa.api.zaloapp.com"
            } else {
                &s.domains.chat
            };
            let (clean_text, text_props_opt) = extract_zalo_rtf_text_properties(text);
            let mut body = obj(&[
                ("toid", json!(to_id)),
                ("message", json!(clean_text)),
                ("clientId", json!(now)),
                ("qmsgOwner", json!(qmsg_owner)),
                ("qmsgId", json!(qmsg_id_num)),
                ("qmsgCliId", json!(qmsg_cli_id)),
                ("qmsgType", json!(1)),
                ("qmsgTs", json!(qmsg_ts)),
                ("qmsg", json!(quoted.text)),
                ("imei", json!(s.imei)),
                ("ttl", json!(0)),
                ("zsource", json!(101)),
            ]);
            if let (Some(tp), Some(map)) = (text_props_opt, body.as_object_mut()) {
                map.insert("textProperties".into(), json!(tp));
            }
            let params = s.encoded_params(&body)?;
            let url = format!("{host}/api/message/quote?{}", s.common_params());
            let resp = s.post_form(&url, format!("params={params}")).await?;
            let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            // Keep the server's reason (1-1 variant); the wrapper logs it and adds context.
            if let Some(err) = reply_error(&resp) {
                return Err(failed("Gửi tin trích dẫn", err));
            }
            Ok(resp)
        }
    }
    .await;

    match quote_res {
        Ok(r) => Ok(r),
        Err(e) => {
            // Same rule as `send_link_message`: sending the text instead and reporting Ok hid a
            // quote that never existed on the server (`re-notes/10` §3).
            crate::logger::log_error("chat::send_quote_message", &format!("{e:#}"));
            Err(e.context("Không gửi được tin trích dẫn"))
        }
    }
}

/// Forward a message or sticker to another conversation (`forwardMultiMessage` @ `compact-app-pc.js:14323900`).
pub async fn forward_message(
    s: &Session,
    target_id: &str,
    target_is_group: bool,
    msg: &ChatMessage,
) -> Result<Value> {
    forward_message_with_client_id(s, target_id, target_is_group, msg, next_client_message_id())
        .await
}

/// Preserve the request client ID for reconciliation with a later server echo.
pub(crate) async fn forward_message_with_client_id(
    s: &Session,
    target_id: &str,
    target_is_group: bool,
    msg: &ChatMessage,
    now: u64,
) -> Result<Value> {
    if let Some(sid) = msg.sticker_id {
        if sid > 0 {
            let cid = msg.sticker_cate_id.unwrap_or(0);
            let stype = msg.sticker_type.unwrap_or(7);
            return send_sticker_with_client_id(
                s,
                target_id,
                sid,
                cid,
                stype,
                target_is_group,
                now,
            )
            .await;
        }
    }
    if target_is_group {
        send_group_message_with_client_id(s, target_id, &msg.text, now).await
    } else {
        send_message_with_client_id(s, target_id, &msg.text, now).await
    }
}

/// Convert Zalo reaction icon code to visual emoji.
pub fn icon_to_emoji(icon: &str) -> &'static str {
    match icon {
        "/-strong" => "👍",
        "/-heart" => "❤️",
        ":>" => "😆",
        ":o" => "😮",
        ":-((" | ":-(( " => "😢",
        ":-h" => "😠",
        _ => "❤️",
    }
}

/// Map a visual reaction emoji or Zalo reaction code to `(rIcon, rType)` (`compact-app-pc.js:10848900`).
pub fn reaction_code_and_type(emoji_or_code: &str) -> (&'static str, i32) {
    match emoji_or_code {
        "👍" | "/-strong" => ("/-strong", 3),
        "❤️" | "❤" | "/-heart" => ("/-heart", 5),
        "😆" | ":>" => (":>", 0),
        "😮" | ":o" => (":o", 32),
        "😢" | ":-(( " | ":-((" => (":-((", 2),
        "😠" | ":-h" => (":-h", 20),
        _ => ("/-heart", 5),
    }
}

/// Send a message reaction (`sendReactionMessage` in `compact-app-pc.b7bec208c4c8f73e09f8.js:14317097`, cmd `12410` / `12411`).
pub async fn send_reaction(
    s: &Session,
    to_id: &str,
    is_group: bool,
    msg: &ChatMessage,
    reaction: &str,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let (r_icon, r_type) = reaction_code_and_type(reaction);
    let is_local_id = msg.id.is_empty() || msg.id.starts_with("out_") || msg.id.starts_with("local_");
    let g_msg_id = if is_local_id { 0 } else { msg.id.parse::<u64>().unwrap_or(0) };
    let c_msg_id = msg.client_message_id.parse::<u64>().ok().filter(|id| *id > 0)
        .or_else(|| (msg.ts > 0).then_some(msg.ts as u64))
        .unwrap_or(g_msg_id);
    let msg_type = if msg.sticker_id.is_some() { 5 } else if is_zalo_photo_cdn_url(msg.link_url.as_deref().unwrap_or("")) || msg.link_src.as_deref() == Some("Hình ảnh Zalo") { 32 } else { 1 };
    let inner_msg = json!({
        "rMsg": [{ "gMsgID": g_msg_id, "cMsgID": c_msg_id, "msgType": msg_type }],
        "rIcon": r_icon,
        "rType": r_type,
        "source": 6
    }).to_string();

    let react_list = json!([{
        "message": inner_msg,
        "clientId": now
    }]);

    let (url, body) = if is_group {
        let clean_grid = to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        (
            format!(
                "https://reaction.api.zaloapp.com/api/group/reaction?{}",
                s.common_params()
            ),
            obj(&[
                ("react_list", react_list),
                ("grid", json!(clean_grid)),
                ("imei", json!(s.imei)),
            ]),
        )
    } else {
        (
            format!(
                "https://reaction.api.zaloapp.com/api/message/reaction?{}",
                s.common_params()
            ),
            obj(&[("react_list", react_list), ("toid", json!(to_id))]),
        )
    };

    let params = s.encoded_params(&body)?;
    let resp = s.post_form(&url, format!("params={params}")).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    // The reaction endpoint answers HTTP 200 with `error_code != 0` on refusal; without this
    // check a failed tap looked delivered (the GUI had already added it optimistically).
    ensure_action_ok("Thả cảm xúc", "chat::send_reaction", &resp)?;
    Ok(resp)
}

/// Server-side id of a message that may be referenced by a write request (delete / recall).
///
/// A row with no server id yet (`out_*`, `local_*`, empty) exists only on this client: sending
/// that id makes the server answer a generic error while the UI has already removed the bubble,
/// i.e. a no-op that looks successful. `action` is the leading Vietnamese phrase so the caller's
/// message stays specific ("Không xoá được tin này …" / "Không thu hồi được tin này …").
fn server_message_id<'a>(msg: &'a ChatMessage, action: &str) -> Result<&'a str> {
    let id = msg.id.trim();
    if id.is_empty() || id.starts_with("out_") || id.starts_with("local_") {
        anyhow::bail!("{action} (tin chưa có mã từ máy chủ)");
    }
    Ok(id)
}

/// Delete a message for me (`deleteOneOneMessageV2` / `deleteGroupMessageV2` @ `compact-app-pc.js:14373319`, cmd `11779` / `11834`).
pub async fn delete_message_for_me(
    s: &Session,
    to_id: &str,
    is_group: bool,
    msg: &ChatMessage,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // Refuse before any request: a client-only id cannot be deleted on the server.
    let msg_id = server_message_id(msg, "Không xoá được tin này")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let my_uid = s
        .login
        .as_ref()
        .map(|l| l.uin.clone())
        .unwrap_or_else(|| "0".into());
    let owner_id = if msg.is_outgoing {
        my_uid.as_str()
    } else if !is_group {
        to_id
    } else {
        &msg.sender
    };
    let cli_id = if msg.ts > 0 {
        msg.ts.to_string()
    } else {
        now.to_string()
    };

    let msgs_arr = json!([{
        "cliMsgId": cli_id,
        "globalMsgId": msg_id,
        "ownerId": owner_id,
        "destId": to_id
    }]);

    let reply = if is_group {
        let clean_grid = to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        let body = obj(&[
            ("grid", json!(clean_grid)),
            ("cliMsgId", json!(now)),
            ("msgs", msgs_arr),
            ("onlyMe", json!(1)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{}/api/group/deletemsg?{}",
            s.domains.group,
            s.common_params()
        );
        let resp = s.post_form(&url, format!("params={params}")).await?;
        s.maybe_decrypt(resp, s.session_key_bytes()).await
    } else {
        let host = if s.domains.chat.contains("-msg-chat.")
            || s.domains.chat.contains("tt-chat2-wpa")
            || s.domains.chat.is_empty()
        {
            "https://tt-chat3-wpa.api.zaloapp.com"
        } else {
            &s.domains.chat
        };
        let body = obj(&[
            ("toid", json!(to_id)),
            ("cliMsgId", json!(now)),
            ("msgs", msgs_arr),
            ("onlyMe", json!(1)),
            ("imei", json!(s.imei)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!("{host}/api/message/delete?{}", s.common_params());
        let resp = s.post_form(&url, format!("params={params}")).await?;
        s.maybe_decrypt(resp, s.session_key_bytes()).await
    };
    // "Xoá chỉ ở phía tôi" reported success as soon as the HTTP call left the client, so a
    // refused delete kept the bubble on the server while the UI had already removed it.
    ensure_action_ok("Xoá tin nhắn", "chat::delete_message_for_me", &reply)?;
    Ok(reply)
}

/// Recall / undo an outgoing message for everyone (`undoMessage` @ `compact-app-pc.js:14376599`, cmd `11281` / `11192`).
pub async fn recall_message(
    s: &Session,
    to_id: &str,
    is_group: bool,
    msg: &ChatMessage,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // Refuse before any request: a client-only id cannot be recalled on the server.
    let msg_id = server_message_id(msg, "Không thu hồi được tin này")?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let cli_id = if msg.ts > 0 {
        msg.ts.to_string()
    } else {
        now.to_string()
    };

    let reply = if is_group {
        let clean_grid = to_id
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        let body = obj(&[
            ("msgId", json!(msg_id)),
            ("grid", json!(clean_grid)),
            ("clientId", json!(now)),
            ("visibility", json!(0)),
            ("cliMsgIdUndo", json!(cli_id)),
            ("imei", json!(s.imei)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{}/api/group/undomsg?{}",
            s.domains.group,
            s.common_params()
        );
        let resp = s.post_form(&url, format!("params={params}")).await?;
        s.maybe_decrypt(resp, s.session_key_bytes()).await
    } else {
        let host = if s.domains.chat.contains("-msg-chat.")
            || s.domains.chat.contains("tt-chat2-wpa")
            || s.domains.chat.is_empty()
        {
            "https://tt-chat3-wpa.api.zaloapp.com"
        } else {
            &s.domains.chat
        };
        let body = obj(&[
            ("msgId", json!(msg_id)),
            ("toid", json!(to_id)),
            ("clientId", json!(now)),
            ("cliMsgIdUndo", json!(cli_id)),
        ]);
        let params = s.encoded_params(&body)?;
        let url = format!("{host}/api/message/undo?{}", s.common_params());
        let resp = s.post_form(&url, format!("params={params}")).await?;
        s.maybe_decrypt(resp, s.session_key_bytes()).await
    };
    // A refused recall (too late, not the sender, no permission) used to look like a recall:
    // the bubble stayed everywhere but the local UI had already greyed it out.
    ensure_action_ok("Thu hồi tin nhắn", "chat::recall_message", &reply)?;
    Ok(reply)
}

/// `GET /api/preloadconvers/get-last-msgs` (cmd 10053).
///
/// [O] request: GET `{convers_api}/api/preloadconvers/get-last-msgs?{common}&params={encodeAES(json)}`
/// where payload is `{ threadIdLocalMsgId: map_object, imei }`.
pub async fn get_last_msgs(s: &Session, thread_ids: &[String]) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let mut map = serde_json::Map::new();
    // [O] `getLastMessagesForPreview(e)` in `compact-app-pc.js:14385933` passes `{}` on preload
    // to fetch all recent conversation previews without overflowing URL length.
    if thread_ids.len() <= 25 {
        for id in thread_ids {
            let clean = id.trim_start_matches("group_").trim_start_matches('g');
            let key = if id.starts_with("group_") || id.starts_with('g') {
                format!("{clean}_1")
            } else {
                format!("{clean}_0")
            };
            map.insert(key, json!("1"));
        }
    }
    let map_str = serde_json::to_string(&map).unwrap_or_else(|_| "{}".into());
    let body = obj(&[
        ("threadIdLocalMsgId", json!(map_str)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_last_msgs::encoded_params", &e.to_string());
            eprintln!("! get_last_msgs: no session key for encoded_params ({e}), falling back to plain_params");
            Session::plain_params(&body)
        }
    };

    let url = format!(
        "{}/api/preloadconvers/get-last-msgs?{}&{params}",
        s.domains.convers_api,
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if crate::trace_shape::enabled() {
        crate::trace_shape::line(
            "glm",
            format!("get-last-msgs threads={} <- {}", map.len(), crate::trace_shape::shape(&resp)),
        );
    }
    Ok(resp)
}

/// `GET /api/pinconvers/list` (cmd 12120) — pinned threads.
pub async fn get_pinned(s: &Session) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let url = format!(
        "{}/api/pinconvers/list?{}",
        s.domains.conversation,
        s.common_params()
    );
    s.get(&url).await
}

/// `GET /api/group/history` (cmd 11240) — group message history.
pub async fn get_group_history(s: &Session, group_id: &str, count: u32) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let grid = group_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let body = obj(&[
        ("grid", json!(grid)),
        ("count", json!(count)),
        ("imei", json!(s.imei)),
    ]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error("chat::get_group_history::encoded_params", &e.to_string());
            Session::plain_params(&body)
        }
    };
    let url = format!(
        "{}/api/group/history?{}&{params}",
        s.domains.group,
        s.common_params()
    );
    match s.get(&url).await {
        Ok(resp) => Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await),
        Err(e) => {
            // Keep the original cause: the raw probe below asks the same question with a
            // different body, so a silent fallback hides which request actually failed.
            crate::logger::log_error("chat::get_group_history", &format!("{e:#}"));
            get_group_history_raw(s, grid, count).await
        }
    }
}

/// Byte-exact replica of the shipped bundle's `getHistoryMessage(e, t)`.
///
/// [O] `compact-app-pc:14364564`:
/// `n = {grid: e, count: t}`
/// `getGroupDomain() + "/api/group/history?" + _getCommonParams() + "&params=" + encodeURIComponent(encodeAES(JSON.stringify(n)))`
/// Signing tag: **11240**.
///
/// Note: the bundle sends **only** `grid` and `count` — no `imei`, no `signkey` on this
/// path. Kept as a separate raw probe so the guessed `get_group_history` above can be
/// compared against it without breaking existing callers.
pub async fn get_group_history_raw(s: &Session, grid: &str, count: u32) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // [O] getGroupIdFromConversationId: strip the group-id prefix if present.
    let grid = grid
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let body = obj(&[("grid", json!(grid)), ("count", json!(count))]);
    let params = match s.encoded_params(&body) {
        Ok(p) => format!("params={p}"),
        Err(e) => {
            crate::logger::log_error(
                "chat::get_group_history_raw::encoded_params",
                &e.to_string(),
            );
            Session::plain_params(&body)
        }
    };
    let url = format!(
        "{}/api/group/history?{}&{params}",
        s.domains.group,
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    Ok(resp)
}

/// Fetch group metadata (`name`, `avt`/`fullAvt`, `totalMember`, `removedsGroup`) from
/// `POST /api/group/getmg-v2` by sending `gridVerMap: {"<grid>": 0}` so the server always
/// returns `gridInfoMap` instead of putting groups into `unchangedsGroup`.
pub async fn fetch_group_profiles(
    s: &Session,
    group_ids: &[String],
) -> Result<(
    serde_json::Map<String, Value>,
    std::collections::HashSet<String>,
)> {
    if s.login.is_none() || group_ids.is_empty() {
        return Ok((serde_json::Map::new(), std::collections::HashSet::new()));
    }
    let mut gv_zero = serde_json::Map::new();
    for gid in group_ids {
        let clean = gid
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G')
            .trim();
        if !clean.is_empty() {
            gv_zero.insert(clean.to_string(), json!(0));
        }
    }
    let gv_str = serde_json::to_string(&gv_zero)?;
    let body = obj(&[("gridVerMap", json!(gv_str)), ("imei", json!(s.imei))]);
    let params = s.encoded_params(&body)?;
    let mg_url = format!(
        "{}/api/group/getmg-v2?{}",
        s.domains.group,
        s.common_params()
    );
    let mg_resp = s.post_form(&mg_url, format!("params={params}")).await?;
    let mut mg_dec = s.maybe_decrypt(mg_resp, s.session_key_bytes()).await;
    if let Some(d_str) = mg_dec.get("data").and_then(Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<Value>(d_str.trim()) {
            mg_dec["data"] = parsed;
        }
    }
    let data_node = mg_dec
        .pointer("/data/data")
        .or_else(|| mg_dec.get("data"))
        .unwrap_or(&mg_dec);
    let info_map = data_node
        .get("gridInfoMap")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let removed: std::collections::HashSet<String> = data_node
        .get("removedsGroup")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    Ok((info_map, removed))
}

/// Extract up to 4 sorted member UIDs (`creatorId` first, then `adminIds`, then `memVerList`)
/// and total member count from a `getmg-v2` group info object.
fn extract_group_member_ids(info: &Value) -> (Vec<String>, usize) {
    let mut raw_ids: Vec<String> = Vec::new();
    if let Some(arr) = info.get("memVerList").and_then(Value::as_array) {
        for item in arr {
            if let Some(s) = item.as_str() {
                let uid = s.split('_').next().unwrap_or("").trim();
                if !uid.is_empty() && !raw_ids.iter().any(|x| x == uid) {
                    raw_ids.push(uid.to_string());
                }
            }
        }
    }
    let mut sorted: Vec<String> = Vec::new();
    if let Some(creator) = pick_str(info, &["creatorId"]) {
        if raw_ids.iter().any(|x| x == &creator) {
            sorted.push(creator);
        }
    }
    if let Some(admins) = info.get("adminIds").and_then(Value::as_array) {
        for a in admins {
            let aid = a
                .as_str()
                .map(str::to_string)
                .or_else(|| a.as_u64().map(|n| n.to_string()))
                .unwrap_or_default();
            if !aid.is_empty()
                && raw_ids.iter().any(|x| x == &aid)
                && !sorted.iter().any(|x| x == &aid)
            {
                sorted.push(aid);
            }
        }
    }
    for uid in &raw_ids {
        if !sorted.iter().any(|x| x == uid) {
            sorted.push(uid.clone());
        }
    }
    let total = info
        .get("totalMember")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or_else(|| sorted.len())
        .max(sorted.len());
    sorted.truncate(4);
    (sorted, total)
}

/// Format a `multi:<total>;<name1>|<url1>;...` composite group avatar string.
fn format_multi_group_avatar(
    group_name: &str,
    mem_ids: &[String],
    total_members: usize,
    profiles: &serde_json::Map<String, Value>,
) -> Option<String> {
    let fallback_names: Vec<&str> = group_name
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let mut slots: Vec<String> = Vec::new();
    for (idx, uid) in mem_ids.iter().take(4).enumerate() {
        let mut m_name = fallback_names
            .get(idx)
            .copied()
            .unwrap_or("Thành viên")
            .to_string();
        let mut m_ava = String::new();
        if let Some(p) = profiles.get(uid) {
            if let Some(n) = pick_str(p, &["zaloName", "displayName", "name", "display_name"]) {
                if !n.is_empty() {
                    m_name = n;
                }
            }
            if let Some(a) = pick_str(p, &["avatar", "avt", "profile_avatar"]) {
                if !a.is_empty() && !a.ends_with("/default") {
                    m_ava = a;
                }
            }
        }
        let clean_name = m_name.replace(';', " ").replace('|', " ");
        let clean_ava = m_ava.replace(';', "").replace('|', "");
        slots.push(format!("{clean_name}|{clean_ava}"));
    }
    if slots.is_empty() {
        return None;
    }
    // If the group name is a comma-separated list of member names (e.g. "Đỗ Thành, Đtc, Hoang"),
    // order the slots to match `fallback_names` so API-resolved and name-derived slots never swap positions.
    if fallback_names.len() >= 2 {
        slots.sort_by_key(|slot| {
            let s_name = slot.split('|').next().unwrap_or("").trim();
            let s_fold = strip_vn_for_sticker_kw(s_name);
            fallback_names
                .iter()
                .position(|cand| {
                    let c_fold = strip_vn_for_sticker_kw(cand);
                    !c_fold.is_empty()
                        && (c_fold == s_fold
                            || s_fold.contains(&c_fold)
                            || c_fold.contains(&s_fold))
                })
                .unwrap_or(99)
        });
    }
    let tot = total_members.max(slots.len());
    Some(format!("multi:{tot};{}", slots.join(";")))
}

/// Groups asked about per `getmg-v2` request.
const GROUP_INFO_CHUNK: usize = 50;

/// `gridVerMap` for `getmg-v2`: every group at version `0`. The server only returns
/// `gridInfoMap` entries for groups whose version differs from the one sent; sending the
/// versions `getlg/v4` just reported made it answer `unchangedsGroup` for all of them, so every
/// group stayed "Nhóm <id>" (real account, 02/11). QTzalo caches no group info, so it always
/// asks from version 0.
fn group_info_request_map(ids: &[String]) -> String {
    let map: serde_json::Map<String, Value> = ids.iter().map(|id| (id.clone(), json!(0))).collect();
    Value::Object(map).to_string()
}

/// `GET /api/group/getlg/v4` (cmd 12027) — list of all groups user belongs to.
pub async fn get_groups(s: &Session) -> Result<Vec<Thread>> {
    if s.login.is_none() {
        return Ok(Vec::new());
    }
    let url = format!(
        "{}/api/group/getlg/v4?{}",
        s.domains.group,
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let mut resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(d_str) = resp.get("data").and_then(Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<Value>(d_str.trim()) {
            resp["data"] = parsed;
        }
    }
    let data_node = resp
        .pointer("/data/data")
        .or_else(|| resp.get("data"))
        .unwrap_or(&resp);

    let mut groups = Vec::new();
    if let Some(map) = data_node.get("gridVerMap").and_then(Value::as_object) {
        for (grid, _ver) in map {
            groups.push(Thread {
                presentation: Default::default(),
                unread: Default::default(),
                id: grid.clone(),
                name: format!("Nhóm {}", grid),
                preview: String::new(),
                time: crate::ui::clock(),
                avatar: None,
                is_group: true,
                messages: Vec::new(),
            });
        }
        if !groups.is_empty() {
            let gids: Vec<String> = groups.iter().map(|g| g.id.clone()).collect();
            let mut combined_info = serde_json::Map::new();
            let mut combined_removed = std::collections::HashSet::new();
            for chunk in gids.chunks(GROUP_INFO_CHUNK) {
                if let Ok((info, removed)) = fetch_group_profiles(s, chunk).await {
                    combined_info.extend(info);
                    combined_removed.extend(removed);
                }
            }
            {
                let info_map = combined_info;
                let rem_ids = combined_removed;
                let mut need_mem_uids: Vec<String> = Vec::new();
                for g in &mut groups {
                    if let Some(info) = info_map.get(&g.id) {
                        g.apply_group_sidebar_metadata(info);
                        if let Some(name) = pick_str(info, &["name", "groupName", "displayName"]) {
                            g.name = name;
                        }
                        if let Some(avt) = pick_str(info, &["fullAvt", "avt", "avatar"])
                            .filter(|u| !u.is_empty() && !u.ends_with("/default"))
                        {
                            g.avatar = Some(avt);
                        } else {
                            let (mids, _) = extract_group_member_ids(info);
                            for uid in mids {
                                if !need_mem_uids.iter().any(|x| x == &uid) {
                                    need_mem_uids.push(uid);
                                }
                            }
                        }
                        if g.preview.is_empty() {
                            if let Some(n_mem) = info.get("totalMember").and_then(Value::as_u64) {
                                if n_mem > 0 {
                                    g.preview = format!("Nhóm · {n_mem} thành viên");
                                }
                            }
                        }
                    }
                }
                if !need_mem_uids.is_empty() {
                    let mut all_profiles = serde_json::Map::new();
                    for chunk in need_mem_uids.chunks(50).take(3) {
                        if let Ok(mp) = get_mini_profiles(s, chunk).await {
                            for (k, v) in mp {
                                all_profiles.insert(k, v);
                            }
                        }
                    }
                    for g in &mut groups {
                        if g.avatar.is_none() {
                            if let Some(info) = info_map.get(&g.id) {
                                let (mids, tot) = extract_group_member_ids(info);
                                g.avatar =
                                    format_multi_group_avatar(&g.name, &mids, tot, &all_profiles);
                            }
                        }
                    }
                }
                if !rem_ids.is_empty() {
                    groups.retain(|g| !rem_ids.contains(&g.id));
                }
            }
            crate::trace_shape::line(
                "grp",
                format!(
                    "get_groups: {} groups, {} without a name",
                    groups.len(),
                    groups.iter().filter(|g| is_generic_thread_name(&g.name)).count()
                ),
            );
        }
    }
    Ok(groups)
}

/// `POST /api/social/group/members` (cmd 12030) — fetch member display names and avatars for user IDs.
///
/// Zalo's `/api/social/friend/getminiprofiles` returns `error_code = 114` for raw UID lists,
/// whereas `/api/social/group/members` with `friend_pversion_map: ["<uid>_0", ...]` returns
/// `data.profiles.<uid>.{displayName, zaloName, avatar}` with `error_code = 0`.
pub async fn get_mini_profiles(
    s: &Session,
    user_ids: &[String],
) -> Result<serde_json::Map<String, Value>> {
    if s.login.is_none() || user_ids.is_empty() {
        return Ok(serde_json::Map::new());
    }
    let versioned_ids: Vec<String> = user_ids
        .iter()
        .map(|uid| {
            let clean = uid.trim();
            if clean.contains('_') {
                clean.to_string()
            } else {
                format!("{clean}_0")
            }
        })
        .collect();
    let body = obj(&[("friend_pversion_map", json!(versioned_ids))]);
    let params = s.encoded_params(&body)?;
    let url = format!(
        "{}/api/social/group/members?{}",
        s.domains.profile,
        s.common_params()
    );
    let form = format!("params={params}");
    let resp = s.post_form(&url, form).await?;
    let mut resp = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(d_str) = resp.get("data").and_then(Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<Value>(d_str.trim()) {
            resp["data"] = parsed;
        }
    }
    if let Some(profiles) = resp
        .pointer("/data/profiles")
        .or_else(|| resp.pointer("/data/data/profiles"))
        .or_else(|| resp.get("data"))
        .and_then(Value::as_object)
    {
        return Ok(profiles.clone());
    }
    Ok(serde_json::Map::new())
}

/// Automatically resolve real display names and avatars for 1-1 and group threads that only have placeholder names.
pub async fn resolve_thread_names(s: &Session, threads: &mut [Thread]) {
    let unknown_ids: Vec<String> = threads
        .iter()
        .filter(|t| {
            !t.is_group
                && !t.name.contains("Cloud của tôi")
                && (t.name.starts_with("Người dùng ")
                    || t.name.is_empty()
                    || (t.avatar.as_deref().unwrap_or("").is_empty()
                        && (!t.messages.is_empty() || !t.preview.is_empty())))
        })
        .map(|t| t.id.clone())
        .collect();

    if !unknown_ids.is_empty() {
        for chunk in unknown_ids.chunks(50).take(2) {
            if let Ok(profiles) = get_mini_profiles(s, chunk).await {
                for t in threads.iter_mut() {
                    if let Some(p) = profiles.get(&t.id) {
                        if t.name.starts_with("Người dùng ") || t.name.is_empty() {
                            if let Some(name) =
                                pick_str(p, &["zaloName", "displayName", "name", "display_name"])
                            {
                                if !name.is_empty() {
                                    t.name = name;
                                }
                            }
                        }
                        if let Some(ava) = pick_str(p, &["avatar", "avt", "profile_avatar"]) {
                            if !ava.is_empty() {
                                t.avatar = Some(ava);
                            }
                        }
                    }
                }
            }
        }
    }

    let unknown_group_ids: Vec<String> = threads
        .iter()
        .filter(|t| {
            t.is_group
                && (is_generic_thread_name(&t.name)
                    || (t.avatar.as_deref().unwrap_or("").is_empty() && t.preview.is_empty()))
        })
        .map(|t| t.id.clone())
        .collect();

    if !unknown_group_ids.is_empty() {
        for chunk in unknown_group_ids.chunks(50).take(2) {
            if let Ok((info_map, _)) = fetch_group_profiles(s, chunk).await {
                let mut need_mem_uids: Vec<String> = Vec::new();
                for t in threads.iter_mut() {
                    let clean_id =
                        t.id.trim_start_matches("group_")
                            .trim_start_matches('g')
                            .trim_start_matches('G');
                    if let Some(info) = info_map.get(&t.id).or_else(|| info_map.get(clean_id)) {
                        t.apply_group_sidebar_metadata(info);
                        if let Some(name) = pick_str(info, &["name", "groupName", "displayName"]) {
                            if !name.is_empty() {
                                t.name = name;
                            }
                        }
                        if let Some(avt) = pick_str(info, &["fullAvt", "avt", "avatar"])
                            .filter(|u| !u.is_empty() && !u.ends_with("/default"))
                        {
                            t.avatar = Some(avt);
                        } else {
                            let (mids, _) = extract_group_member_ids(info);
                            for uid in mids {
                                if !need_mem_uids.iter().any(|x| x == &uid) {
                                    need_mem_uids.push(uid);
                                }
                            }
                        }
                        if t.preview.is_empty() {
                            if let Some(n_mem) = info.get("totalMember").and_then(Value::as_u64) {
                                if n_mem > 0 {
                                    t.preview = format!("Nhóm · {n_mem} thành viên");
                                }
                            }
                        }
                    }
                }
                if !need_mem_uids.is_empty() {
                    let mut all_profiles = serde_json::Map::new();
                    for mchunk in need_mem_uids.chunks(50).take(2) {
                        if let Ok(mp) = get_mini_profiles(s, mchunk).await {
                            for (k, v) in mp {
                                all_profiles.insert(k, v);
                            }
                        }
                    }
                    for t in threads.iter_mut() {
                        if !t.is_group {
                            continue;
                        }
                        let clean_id =
                            t.id.trim_start_matches("group_")
                                .trim_start_matches('g')
                                .trim_start_matches('G');
                        if let Some(info) = info_map.get(&t.id).or_else(|| info_map.get(clean_id)) {
                            let cur = t.avatar.as_deref().unwrap_or("");
                            if cur.is_empty() || cur.starts_with("multi:") {
                                let (mids, tot) = extract_group_member_ids(info);
                                if let Some(multi_s) =
                                    format_multi_group_avatar(&t.name, &mids, tot, &all_profiles)
                                {
                                    if should_replace_avatar(t.avatar.as_deref(), Some(&multi_s)) {
                                        t.avatar = Some(multi_s);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Offline / local contact fallback for composite group avatars: match comma-separated member names
    // against known 1-1 contact threads so group avatars display real member photos even without network.
    let known_contacts: Vec<(String, String)> = threads
        .iter()
        .filter(|t| {
            !t.is_group
                && !t.name.contains("Cloud của tôi")
                && !t.avatar.as_deref().unwrap_or("").is_empty()
                && !t.avatar.as_deref().unwrap_or("").ends_with("/default")
        })
        .map(|t| (t.name.clone(), t.avatar.clone().unwrap_or_default()))
        .collect();

    for t in threads.iter_mut() {
        if !t.is_group {
            continue;
        }
        let cur_ava = t.avatar.as_deref().unwrap_or("");
        if avatar_http_url_count(Some(cur_ava)) > 0 {
            continue;
        }
        let parts: Vec<&str> = t
            .name
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() >= 2 {
            let mut slots: Vec<String> = Vec::new();
            for p in parts.iter().take(4) {
                let p_fold = strip_vn_for_sticker_kw(p);
                let matched_url = known_contacts
                    .iter()
                    .find(|(cname, _)| {
                        let c_fold = strip_vn_for_sticker_kw(cname);
                        !p_fold.is_empty() && c_fold == p_fold
                    })
                    .map(|(_, url)| url.as_str())
                    .unwrap_or("");
                slots.push(format!(
                    "{}|{}",
                    p.replace(';', " ").replace('|', " "),
                    matched_url
                ));
            }
            let new_multi = format!("multi:{};{}", parts.len(), slots.join(";"));
            if should_replace_avatar(t.avatar.as_deref(), Some(&new_multi)) {
                t.avatar = Some(new_multi);
            }
        }
    }
}

/// Per-queue cursor for `getChatFromPolling` (`MultiQueueId` in module `1iR4`, `sync-v2-sub-worker.js:1620000`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct QueueCursor {
    #[serde(rename = "lastId")]
    pub last_id: String,
    pub ids: Vec<String>,
}

/// State machine for Zalo's native HTTP long-polling message receiver (`getChatFromPolling`, cmd `99100`).
///
/// [O] Observed in `sync-v2-sub-worker.b7bec208c4c8f73e09f8.js` (offsets `1620000`, `3498103`, `14557981`):
/// - `R = new y({ type: "MSG", qosId: 91021, queueIds: [1, 2, 3], ... })`
/// - `getLastActionIds()` builds:
///   `"{lastActionId},{lastActionIdOther}," + encodeURIComponent(btoa(JSON.stringify(queues)) + ",1")`
/// - `getChatFromPolling(params)`:
///   `GET {zpw_chts_new[0]}/?zpw_ver=694&zpw_type=30&params={params}&ts={unix_sec}`
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct PollState {
    #[serde(skip)]
    pub owner_uin: String,
    pub last_action_id: String,
    pub last_action_id_other: String,
    pub queues: std::collections::BTreeMap<String, QueueCursor>,
}

/// Long-poll MSG queue ids. In dev mode `QTZALO_LP_QUEUES=1,2,3,6` tries extra queues for the
/// sync investigation — e.g. `"6"`, which `queue_actionid_map` pairs with socket queue `511_0`
/// (group messages while offline). Not enabled by default: see `update_from_response`.
fn lp_msg_queue_ids() -> Vec<String> {
    let default = || vec!["1".to_string(), "2".to_string(), "3".to_string()];
    if !crate::logger::is_dev_mode() {
        return default();
    }
    let Ok(raw) = std::env::var("QTZALO_LP_QUEUES") else {
        return default();
    };
    let ids: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 3 && s.bytes().all(|b| b.is_ascii_digit()))
        .map(String::from)
        .collect();
    if ids.is_empty() {
        default()
    } else {
        ids
    }
}

impl Default for PollState {
    fn default() -> Self {
        let mut queues = std::collections::BTreeMap::new();
        for qid in lp_msg_queue_ids() {
            queues.insert(
                qid,
                QueueCursor {
                    last_id: "1".into(),
                    ids: Vec::new(),
                },
            );
        }
        Self {
            owner_uin: String::new(),
            last_action_id: "1".into(),
            last_action_id_other: "1".into(),
            queues,
        }
    }
}

/// The queue action ids a long-poll echoes back: at most the 10 newest ids `<= last_id`,
/// ascending. [O] `QueueControl.getLastActionIdsArray` (`sync-v2-sub-worker.js:1606950`): sort
/// descending, take at most 10 <= lastId, then reverse. Ids come as strings or numbers.
pub(crate) fn newest_action_ids(ids_arr: &[Value], last_id: &str) -> Vec<String> {
    let max_lid = last_id.parse::<u128>().unwrap_or(u128::MAX);
    let mut ids: Vec<String> = ids_arr
        .iter()
        .filter_map(|v| match v {
            Value::String(s) if !s.is_empty() => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| s.parse::<u128>().map(|n| n <= max_lid).unwrap_or(true))
        .collect();
    ids.sort_by_key(|s| std::cmp::Reverse(s.parse::<u128>().unwrap_or(0)));
    ids.dedup();
    ids.truncate(10);
    ids.reverse();
    ids
}

fn max_action_id(a: &str, b: &str) -> String {
    let na = a.parse::<u128>().unwrap_or(0);
    let nb = b.parse::<u128>().unwrap_or(0);
    if nb >= na && nb > 0 {
        b.to_string()
    } else if na > 0 {
        a.to_string()
    } else {
        "1".into()
    }
}

impl PollState {
    pub fn encode_params(&self) -> String {
        use base64::Engine;
        let q_json = serde_json::to_string(&self.queues).unwrap_or_else(|_| "{}".into());
        let b64 = base64::engine::general_purpose::STANDARD.encode(q_json.as_bytes());
        let tail = crate::session::urlencode(&format!("{b64},1"));
        format!(
            "{},{},{}",
            self.last_action_id, self.last_action_id_other, tail
        )
    }

    pub fn update_from_response(&mut self, resp: &Value) {
        let mut data_node = resp.get("data").unwrap_or(resp);
        if let Some(inner) = data_node.get("data").filter(|v| v.is_object()) {
            data_node = inner;
        }
        if let Some(reset_id) = pick_str(data_node, &["resetLastActionId"]) {
            if reset_id != "0" && reset_id.parse::<u128>().unwrap_or(0) > 0 {
                self.last_action_id = reset_id.clone();
                self.last_action_id_other = "1".into();
                for q in self.queues.values_mut() {
                    q.last_id = reset_id.clone();
                    q.ids.clear();
                }
                return;
            }
        }
        if let Some(lai) = pick_str(data_node, &["lastActionId"]) {
            if lai != "0" {
                self.last_action_id = max_action_id(&self.last_action_id, &lai);
            }
        }
        if let Some(laio) = pick_str(data_node, &["lastActionIdOther"]) {
            if laio != "0" {
                self.last_action_id_other = max_action_id(&self.last_action_id_other, &laio);
            }
        }
        if let Some(qs) = data_node.get("queueStatus").and_then(Value::as_object) {
            for (qid, qval) in qs {
                // Message queues the server reports for this session but we do not track yet
                // (`queue_actionid_map`: "6" = 511_0, "15" = 510_1, "16" = 511_1) are tracked
                // from the start of what the server kept. Without this the long-poll repeats
                // the same `more = 1` page forever (real account: group queue "16", 02/11).
                // Unknown queues ("7", "8") stay untracked.
                if matches!(qid.as_str(), "6" | "15" | "16") && !self.queues.contains_key(qid) {
                    self.queues.insert(
                        qid.clone(),
                        QueueCursor {
                            last_id: "1".into(),
                            ids: Vec::new(),
                        },
                    );
                    continue;
                }
                let Some(entry) = self.queues.get_mut(qid) else {
                    continue;
                };
                if let Some(lid) = pick_str(qval, &["lastId"]) {
                    if lid != "0" {
                        entry.last_id = max_action_id(&entry.last_id, &lid);
                    }
                }
                if let Some(ids_arr) = qval.get("ids").and_then(Value::as_array) {
                    let new_ids = newest_action_ids(ids_arr, &entry.last_id);
                    if !new_ids.is_empty() {
                        entry.ids = new_ids;
                    }
                }
            }
        }
    }
}

/// Fetch cloud message history (`getCM` in `sync-v2-sub-worker.b7bec208c4c8f73e09f8.js:14595433`, cmd `12506` / `12507`).
///
/// [O] Observed in module `1EWQ` (`sync-v2-sub-worker.js:1545933`):
/// Cloud message history (`/api/cm/getrecentv2` and `/api/cm/getoldv2` on `group_cloud_message`)
/// is enabled for Group conversations (`g<groupId>`) and My Cloud (`sendToMeId`).
pub async fn get_cloud_messages(
    s: &Session,
    conv_id: &str,
    global_msg_id: &str,
    is_old: bool,
    count: u32,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean_id = conv_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G');
    let is_send2me = s
        .login
        .as_ref()
        .map(|l| !l.send2me_id.is_empty() && l.send2me_id == clean_id)
        .unwrap_or(false);
    let mut map = serde_json::Map::new();
    map.insert("groupId".into(), json!(clean_id));
    map.insert(
        "globalMsgId".into(),
        json!(if global_msg_id.is_empty() || global_msg_id == "0" {
            "9223372036854775808"
        } else {
            global_msg_id
        }),
    );
    map.insert("count".into(), json!(count));
    map.insert("msgIds".into(), json!([]));
    map.insert("imei".into(), json!(s.imei));
    if is_send2me {
        map.insert("isOA".into(), json!(1));
    }
    map.insert("src".into(), json!(if is_old { 3 } else { 1 }));

    let body = Value::Object(map);
    let params = s.encoded_params(&body)?;
    let ep = if is_old {
        "/api/cm/getoldv2"
    } else {
        "/api/cm/getrecentv2"
    };
    let host = if s.domains.cloud_msg.is_empty() {
        "https://tt-group-cm.api.zaloapp.com"
    } else {
        &s.domains.cloud_msg
    };
    let url = format!(
        "{}{ep}?{}&params={params}&nretry=0",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let mut dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(d_str) = dec.get("data").and_then(Value::as_str) {
        let trimmed = d_str.trim();
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            if let Ok(parsed_data) = serde_json::from_str::<Value>(trimmed) {
                dec["data"] = parsed_data;
            }
        }
    }
    if crate::trace_shape::enabled() {
        let kind = if is_send2me { "send2me" } else { "group" };
        crate::trace_shape::line(
            "cm",
            format!("{ep} {kind} count={count} <- {}", crate::trace_shape::shape(&dec)),
        );
    }
    Ok(dec)
}

/// Field [`get_chat_from_polling`] sets when the 12 s client window expired with **no** answer.
///
/// [O] The server ends its own long-poll with an empty body on keepalive timeout; a *client*
/// timeout is a different thing — the request may still be in flight, no page was delivered
/// and the cursor did not move. Treating it as `{"error_code":0,"data":{}}` made a slow network
/// indistinguishable from "no new messages" (`re-notes/10` §7).
const LP_TIMEOUT_KEY: &str = "__qtzalo_lp_timeout";

/// `true` when a long-poll reply is the client-side timeout marker, not a server page.
pub fn is_poll_timeout(lp_resp: &Value) -> bool {
    lp_resp
        .get(LP_TIMEOUT_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The value [`get_chat_from_polling`] returns when its window expired without an answer.
///
/// It keeps `error_code: 0` so existing callers do not treat it as an error, but the extra
/// field makes it distinguishable — see [`is_poll_timeout`].
fn poll_timeout_marker() -> Value {
    let mut v = json!({ "error_code": 0, "data": {} });
    v[LP_TIMEOUT_KEY] = json!(true);
    v
}

/// Call Zalo's native HTTP long-polling message endpoint (`getChatFromPolling`, cmd `99100`).
///
/// Uses a 12-second timeout matching `lp_chat` long-polling windows (`sync-v2-sub-worker.js:3498030`).
pub async fn get_chat_from_polling(s: &Session, state: &mut PollState) -> Result<Value> {
    let uin = s.login.as_ref().map(|login| login.uin.as_str()).unwrap_or_default();
    if state.owner_uin != uin {
        *state = crate::sync_state::SyncState::new(uin).poll;
    }

    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let host = if s.domains.lp_chat.is_empty() {
        "https://p4-msg-chat.zaloapp.com"
    } else {
        &s.domains.lp_chat
    };
    let ts = crate::login::unix_now();
    let url = format!(
        "{}/?{}&params={}&ts={ts}",
        host.trim_end_matches('/'),
        s.common_params(),
        state.encode_params()
    );
    if crate::trace_shape::enabled() {
        let queues: Vec<String> = state
            .queues
            .iter()
            .map(|(q, c)| format!("{q}:{}/{}", c.last_id, c.ids.len()))
            .collect();
        crate::trace_shape::line(
            "lp",
            format!(
                "-> poll lastActionId={} other={} queues(lastId/ids)=[{}]",
                state.last_action_id,
                state.last_action_id_other,
                queues.join(" ")
            ),
        );
    }
    let fut = s.get(&url);
    match tokio::time::timeout(std::time::Duration::from_millis(12000), fut).await {
        Ok(Ok(resp)) => {
            let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
            if crate::trace_shape::enabled() {
                crate::trace_shape::line("lp", format!("<- {}", crate::trace_shape::shape(&dec)));
            }
            if let Some(login) = s.login.as_ref() {
                crate::sync_state::record_gaps_from(&dec, &login.uin);
            }
            if let Some(code) = dec.get("error_code").and_then(Value::as_i64) {
                if code != 0 {
                    let msg = dec
                        .get("error_message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    return Err(crate::session::SessionError::new(code, msg).into());
                }
            }
            state.update_from_response(&dec);
            Ok(dec)
        }
        Ok(Err(e)) => {
            crate::trace_shape::line("lp", format!("<- error {e:#}"));
            Err(e)
        }
        Err(_) => {
            // The window expired and the cursor did not move: hand the caller a distinguishable
            // value instead of an empty page.
            crate::trace_shape::line("lp", "<- timeout 12s (cửa sổ kết thúc, con trỏ không đổi)");
            Ok(poll_timeout_marker())
        }
    }
}

/// Safety cap on long-poll pages in one catch-up (each page is one `getChatFromPolling`).
pub const LP_CATCHUP_MAX_BATCHES: usize = 200;

/// `true` when a long-poll reply says there is more to fetch right away (`more == 1` or a
/// cursor reset to follow).
pub fn lp_reply_wants_more(lp_resp: &Value) -> bool {
    let data_node = lp_resp.get("data").unwrap_or(lp_resp);
    let more = data_node.get("more").and_then(Value::as_i64).unwrap_or(0);
    let has_reset = pick_str(data_node, &["resetLastActionId"])
        .map(|r| r != "0" && !r.is_empty())
        .unwrap_or(false);
    more != 0 || has_reset
}

/// Follow a long-poll reply with another request only when the server says there is more
/// *and* the cursor moved. A reply can say `more = 1` for a queue the client does not track
/// (seen on the real account: group queue `16` repeated the same 100 messages 1800 times in
/// 3 minutes), and asking again with an unchanged cursor would only return the same page.
pub fn lp_should_continue(before: &PollState, after: &PollState, lp_resp: &Value) -> bool {
    if !lp_reply_wants_more(lp_resp) {
        return false;
    }
    if before == after {
        crate::trace_shape::line("lp", "more=1 but the cursor did not move; stop");
        return false;
    }
    true
}

/// Number of chat messages (`msgs` + `groupMsgs`) in a long-poll reply.
pub fn lp_reply_message_count(lp_resp: &Value) -> usize {
    let data_node = lp_resp.get("data").unwrap_or(lp_resp);
    ["msgs", "groupMsgs"]
        .iter()
        .filter_map(|k| data_node.get(*k).and_then(Value::as_array))
        .map(Vec::len)
        .sum()
}

/// `(thread_id, is_group, last_msg_id)` for each conversation in a `get-last-msgs` reply
/// (`clearUnreads`, type 0). On a device that just logged in this is the only list of recent
/// conversations the server gives (previews are filtered), and `lastMsgId` orders them in time.
pub fn conversation_last_msg_ids(resp: &Value) -> Vec<(String, bool, String)> {
    let parsed;
    let mut data_node = resp.get("data").unwrap_or(resp);
    if let Some(s) = data_node.as_str() {
        parsed = serde_json::from_str::<Value>(s.trim()).unwrap_or(Value::Null);
        data_node = &parsed;
    }
    let Some(arr) = data_node.get("clearUnreads").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<(String, bool, String)> = Vec::new();
    for cu in arr {
        if cu.get("type").and_then(Value::as_i64).unwrap_or(0) != 0 {
            continue;
        }
        let (Some(tid), Some(last)) = (pick_str(cu, &["idTo"]), pick_str(cu, &["lastMsgId"])) else {
            continue;
        };
        if tid.is_empty() || tid == "0" || last.parse::<u64>().map(|n| n == 0).unwrap_or(true) {
            continue;
        }
        let is_group = is_group_flag(cu);
        match out.iter_mut().find(|(t, _, _)| *t == tid) {
            Some(e) if e.2.parse::<u64>().unwrap_or(0) < last.parse::<u64>().unwrap_or(0) => e.2 = last,
            Some(_) => {}
            None => out.push((tid, is_group, last)),
        }
    }
    out
}

/// Pages of older cloud history fetched per conversation to close a gap (A4).
pub const CLOUD_GAP_MAX_PAGES: usize = 10;

/// A cloud history page leaves a hole when its oldest message is newer than the newest one
/// stored locally (more arrived while QTzalo was closed than one page holds).
pub fn needs_gap_fill(local_newest_ts: i64, page_oldest_ts: i64) -> bool {
    local_newest_ts > 0 && page_oldest_ts > local_newest_ts
}

/// `(oldest ts, minMsgId, hasMore)` of a `getrecentv2` / `getoldv2` reply.
pub fn cloud_page_info(resp: &Value) -> (i64, Option<String>, bool) {
    let d = resp.get("data").unwrap_or(resp);
    let oldest = d
        .get("groupMsgs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|m| pick_str(m, &["ts"]).and_then(|t| t.parse::<i64>().ok()))
                .min()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let min_id = pick_str(d, &["minMsgId"]).filter(|s| s.parse::<u64>().map(|n| n > 0).unwrap_or(false));
    let has_more = d.get("hasMore").and_then(Value::as_i64).unwrap_or(0) != 0;
    (oldest, min_id, has_more)
}

/// Merge the newest cloud page of `conv_id` into `t`, then follow `getoldv2` until it meets
/// the messages already stored (or [`CLOUD_GAP_MAX_PAGES`]).
async fn merge_cloud_with_gap_fill(s: &Session, t: &mut Thread, conv_id: &str, first_page: &Value) {
    let local_newest = t.messages.iter().map(|m| m.ts).max().unwrap_or(0);
    merge_group_history(t, first_page);
    let (mut oldest, mut min_id, mut has_more) = cloud_page_info(first_page);
    let mut pages = 0;
    while has_more && needs_gap_fill(local_newest, oldest) && pages < CLOUD_GAP_MAX_PAGES {
        let Some(cursor) = min_id.take() else {
            break;
        };
        let Ok(page) = get_cloud_messages(s, conv_id, &cursor, true, 50).await else {
            break;
        };
        merge_group_history(t, &page);
        (oldest, min_id, has_more) = cloud_page_info(&page);
        pages += 1;
    }
    if pages > 0 {
        crate::trace_shape::line("cm", format!("gap fill: {pages} older page(s)"));
    }
}

#[cfg(test)]
mod catchup_tests {
    use super::*;

    #[test]
    fn gap_fill_decision_and_page_info() {
        assert!(needs_gap_fill(1_000, 5_000));
        assert!(!needs_gap_fill(5_000, 1_000), "page reaches back past what we have");
        assert!(!needs_gap_fill(0, 5_000), "nothing stored yet: nothing to join");
        let page = json!({ "data": { "hasMore": 1, "minMsgId": "8440000000001",
            "groupMsgs": [ { "ts": "1793549000000" }, { "ts": "1793548000000" } ] } });
        assert_eq!(cloud_page_info(&page), (1_793_548_000_000, Some("8440000000001".into()), true));
        let empty = json!({ "data": { "hasMore": 0, "groupMsgs": [], "minMsgId": "0" } });
        assert_eq!(cloud_page_info(&empty), (0, None, false));
    }

    fn group(id: &str, name: &str, avatar: Option<&str>) -> Thread {
        Thread {
            id: id.into(),
            name: name.into(),
            preview: String::new(),
            time: String::new(),
            avatar: avatar.map(Into::into),
            is_group: true,
            messages: Vec::new(),
        }
    }

    #[test]
    fn real_names_starting_with_nhom_are_not_placeholders() {
        assert!(is_generic_thread_name("Nhóm 7012345678901234567"));
        assert!(is_generic_thread_name("Người dùng 123"));
        assert!(is_generic_thread_name("123456"));
        assert!(is_generic_thread_name("  "));
        assert!(!is_generic_thread_name("Nhóm Thiết kế"));
        assert!(!is_generic_thread_name("Nhóm 12A1"));

        // A placeholder from an incoming group message never replaces the real name.
        let mut t = group("7012", "Nhóm Thiết kế", None);
        merge_thread_into(&mut t, group("7012", "Nhóm 7012", None), true);
        assert_eq!(t.name, "Nhóm Thiết kế");
        // …but the real name replaces a placeholder.
        let mut p = group("7012", "Nhóm 7012", None);
        merge_thread_into(&mut p, group("7012", "Nhóm Thiết kế", None), true);
        assert_eq!(p.name, "Nhóm Thiết kế");
    }

    #[test]
    fn different_groups_are_never_the_same_contact() {
        let avt = Some("https://s120-ava-talk.zadn.vn/a/b/default_group.jpg");
        let a = group("111", "Lớp 12A1", avt);
        let b = group("222", "Lớp 12A1", avt);
        assert!(!threads_represent_same_contact(&a, &b));
        assert!(threads_represent_same_contact(&a, &group("111", "Khác", None)));
    }

    #[test]
    fn group_info_request_asks_from_version_zero() {
        let ids = vec!["111".to_string(), "222".to_string()];
        let v: Value = serde_json::from_str(&group_info_request_map(&ids)).unwrap();
        assert_eq!(v, json!({ "111": 0, "222": 0 }));
        assert_eq!(group_info_request_map(&[]), "{}");
    }

    #[test]
    fn lp_tracks_reported_message_queues_and_stops_when_stuck() {
        let mut st = PollState::default();
        let reply = json!({ "data": { "more": 1, "lastActionId": "100", "queueStatus": {
            "1": { "lastId": "100", "ids": [] },
            "16": { "lastId": "8442899323544", "ids": ["8442899323544"] },
            "7": { "lastId": "100", "evict": 1 }
        } } });
        st.update_from_response(&reply);
        assert_eq!(st.queues["16"].last_id, "1", "new queue starts from what the server kept");
        assert!(!st.queues.contains_key("7"));

        // Same reply again: queue 16 now advances, so the catch-up may continue.
        let before = st.clone();
        st.update_from_response(&reply);
        assert_eq!(st.queues["16"].last_id, "8442899323544");
        assert!(lp_should_continue(&before, &st, &reply));

        // Nothing moved although the server says "more": stop instead of spinning.
        let before = st.clone();
        st.update_from_response(&reply);
        assert!(!lp_should_continue(&before, &st, &reply));
    }

    #[test]
    fn lp_reply_more_and_count() {
        let done = json!({ "data": { "more": 0, "msgs": [{}, {}], "groupMsgs": [{}], "resetLastActionId": "0" } });
        assert!(!lp_reply_wants_more(&done));
        assert_eq!(lp_reply_message_count(&done), 3);
        assert!(lp_reply_wants_more(&json!({ "data": { "more": 1, "msgs": [] } })));
        assert!(lp_reply_wants_more(&json!({ "data": { "more": 0, "resetLastActionId": "77" } })));
        assert_eq!(lp_reply_message_count(&json!({ "error_code": 0, "data": {} })), 0);
    }

    #[test]
    fn last_msg_ids_from_clear_unreads() {
        let resp = json!({ "data": { "msgs": [], "clearUnreads": [
            { "idTo": "111", "isGroup": 0, "lastMsgId": "8440000000001", "type": 0 },
            { "idTo": "222", "isGroup": 1, "lastMsgId": "8430000000000", "type": 0 },
            { "idTo": "111", "isGroup": 0, "lastMsgId": "8440000000009", "type": 0 },
            { "idTo": "333", "isGroup": 0, "lastMsgId": "0", "type": 0 },
            { "idTo": "444", "isGroup": 0, "lastMsgId": "8450000000000", "type": 1 }
        ] } });
        assert_eq!(
            conversation_last_msg_ids(&resp),
            vec![
                ("111".to_string(), false, "8440000000009".to_string()),
                ("222".to_string(), true, "8430000000000".to_string()),
            ]
        );
        let as_string = json!({ "data": resp["data"].to_string() });
        assert_eq!(conversation_last_msg_ids(&as_string).len(), 2);
    }
}

/// Groups refreshed through `/api/cm/getrecentv2` per catch-up round.
///
/// [O] Each group costs one request, so refreshing every group is an N+1 pattern: on the real
/// account a refresh with many groups fired one `getrecentv2` per group, every round
/// (`re-notes/09` §7). Four is enough to keep the conversations a user is looking at fresh;
/// the rest keep their stored history and come up in the next round.
pub const CLOUD_SYNC_GROUPS_PER_ROUND: usize = 4;

/// Poll both `getChatFromPolling` (`lp_chat`) and `get-last-msgs` (`convers_api`),
/// merge all incoming/outgoing/group messages into `threads`, resolve unknown names,
/// and persist to `zalo_history.db` and `threads_cache.json`.
///
/// The long-poll is followed until the server says it is done (`more == 0`, no reset), up to
/// [`LP_CATCHUP_MAX_BATCHES`] pages. Returns how many messages the long-poll delivered.
pub async fn poll_new_messages(
    s: &Session,
    state: &mut PollState,
    threads: &mut Vec<Thread>,
) -> Result<usize> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    // 1. Poll live/offline message queues via `getChatFromPolling` until caught up.
    let mut fetched = 0usize;
    for batch in 1..=LP_CATCHUP_MAX_BATCHES {
        let before = state.clone();
        let lp_resp = match get_chat_from_polling(s, state).await {
            Ok(r) => r,
            Err(e) => {
                // A transport or server error is not "no new messages": end the round and keep
                // the cursor, so the next poll asks for the same range again.
                crate::logger::log_error("chat::poll_new_messages", &format!("{e:#}"));
                break;
            }
        };
        if is_poll_timeout(&lp_resp) {
            // The window ended without a page. Stop instead of pretending the server answered
            // "nothing new"; the cursor did not move, so the next round resumes here.
            crate::trace_shape::line("lp", "hết cửa sổ long-poll; giữ con trỏ, dừng đợt");
            break;
        }
        merge_previews(threads, &lp_resp);
        fetched += lp_reply_message_count(&lp_resp);
        if !lp_should_continue(&before, state, &lp_resp) {
            break;
        }
        if batch == LP_CATCHUP_MAX_BATCHES {
            crate::trace_shape::line("lp", format!("stopped at the {batch}-batch cap"));
        }
    }
    // 2. Poll conversation previews via `get-last-msgs` (both empty map and populated known thread IDs)
    match get_last_msgs(s, &[]).await {
        Ok(preview_resp) => {
            merge_previews(threads, &preview_resp);
            let activity = conversation_last_msg_ids(&preview_resp);
            if let Some(uin) = s.login.as_ref().and_then(|login| crate::sync_state::sanitize_uin(&login.uin)) {
                if !activity.is_empty() {
                    if let Ok(mut store) = crate::store::MessageStore::open(crate::store::db_path_for_account(uin)) {
                        let _ = store.upsert_thread_activity(&activity);
                    }
                }
            }
        },
        Err(e) if crate::session::SessionStatus::from_error(&e) == crate::session::SessionStatus::Expired => {
            return Err(e);
        }
        Err(_) => {}
    }
    let known_ids: Vec<String> = threads
        .iter()
        .filter(|t| !t.id.is_empty() && t.preview.is_empty() && t.messages.is_empty())
        .map(|t| {
            if t.is_group && !t.id.starts_with('g') && !t.id.starts_with("group_") {
                format!("g{}", t.id)
            } else {
                t.id.clone()
            }
        })
        .collect();
    for chunk in known_ids.chunks(25).take(2) {
        if let Ok(preview_resp) = get_last_msgs(s, chunk).await {
            merge_previews(threads, &preview_resp);
        }
    }
    // 2b. Sync Cloud của tôi (`send2me_id`) and group conversations via `/api/cm/getrecentv2`
    let send2me_id = s
        .login
        .as_ref()
        .map(|l| l.send2me_id.clone())
        .unwrap_or_default();
    if !send2me_id.is_empty() {
        if let Ok(cm_resp) = get_cloud_messages(s, &send2me_id, "0", false, 50).await {
            if let Some(t) = threads.iter_mut().find(|t| t.id == send2me_id) {
                merge_cloud_with_gap_fill(s, t, &send2me_id, &cm_resp).await;
            } else {
                merge_previews(threads, &cm_resp);
            }
        }
    }
    // Newest local activity first, then capped (see [`CLOUD_SYNC_GROUPS_PER_ROUND`]).
    let mut group_ids: Vec<(i64, String)> = threads
        .iter()
        .filter(|t| t.is_group && !t.id.is_empty() && t.messages.is_empty() && !t.preview.is_empty())
        .map(|t| t.id.clone())
        .collect();
    for gid in group_ids.into_iter().take(5) {
        if let Ok(cm_resp) = get_cloud_messages(s, &gid, "0", false, 50).await {
            if let Some(t) = threads.iter_mut().find(|t| t.id == gid) {
                merge_cloud_with_gap_fill(s, t, &gid, &cm_resp).await;
            }
        }
    }
    // 3. Resolve display names for any newly discovered threads
    resolve_thread_names(s, threads).await;
    threads.retain(|t| !t.messages.is_empty() || !t.name.starts_with("Người dùng "));
    deduplicate_threads(threads, None);
    if let Some(uin) = s.login.as_ref().and_then(|login| crate::sync_state::sanitize_uin(&login.uin)) {
        if let Ok(mut store) = crate::store::MessageStore::open(crate::store::db_path_for_account(uin)) {
            let _ = crate::store::persist_threads(&mut store, threads);
        }
        save_threads_cache_for_account(threads, uin);
    }
    Ok(fetched)
}

fn extract_content_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed == "sendBubbleMessage" {
                return None;
            }
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    if let Some(inner) = extract_content_text(&parsed) {
                        if !inner.is_empty() {
                            return Some(inner);
                        }
                    }
                    // It is a JSON object with no text/caption fields (like sticker {catId, id} or photo {href}).
                    // Return None so parse_chat_message can assign a clean descriptive label like [Nhãn dán] or [Hình ảnh]
                    // instead of displaying the raw JSON string in chat.
                    return None;
                }
            }
            Some(s.clone())
        }
        Value::Object(o) => {
            let href = o
                .get("href")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|h| {
                    !h.is_empty() && (h.starts_with("http://") || h.starts_with("https://"))
                });
            if o.get("action").and_then(Value::as_str) == Some("rtf") {
                if let Some(title) = o.get("title").and_then(Value::as_str) {
                    if let Some(params_s) = o.get("params").and_then(Value::as_str) {
                        return Some(format_incoming_zalo_rtf(title, params_s));
                    }
                    return Some(title.to_string());
                }
            }
            for k in ["description", "caption", "text", "msg", "title"] {
                if let Some(Value::String(s)) = o.get(k) {
                    let t = s.trim();
                    if !t.is_empty() && t != "sendBubbleMessage" {
                        return Some(s.clone());
                    }
                }
            }
            if let Some(Value::String(s)) = o.get("fileName").or_else(|| o.get("name")) {
                if !s.trim().is_empty() {
                    return Some(format!("[Tập tin] {s}"));
                }
            }
            if let Some(link) = href {
                return Some(link.to_string());
            }
            None
        }
        _ => None,
    }
}

fn normalize_msg_type(it: &Value) -> String {
    let raw = it
        .get("msgType")
        .or_else(|| it.get("messageType"))
        .or_else(|| it.get("type"));
    match raw {
        Some(Value::String(s)) => s.to_ascii_lowercase(),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(1) => "chat.text".into(),
            Some(2) => "chat.photo".into(),
            Some(3) => "chat.voice".into(),
            Some(4) => "chat.video".into(),
            Some(5) => "chat.sticker".into(),
            Some(19) => "chat.file".into(),
            Some(24) => "chat.location".into(),
            Some(31) => "chat.undo".into(),
            _ => n.to_string(),
        },
        _ => String::new(),
    }
}

fn extract_sticker_info(v: &Value) -> Option<(u64, u64, u32)> {
    match v {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    return extract_sticker_info(&parsed);
                }
            }
            None
        }
        Value::Object(o) => {
            let parse_u64 = |val: &Value| -> Option<u64> {
                match val {
                    Value::Number(n) => n.as_u64(),
                    Value::String(s) => s.parse::<u64>().ok(),
                    _ => None,
                }
            };
            let cat_id = o
                .get("catId")
                .or_else(|| o.get("cateId"))
                .and_then(parse_u64);
            let stk_id = o
                .get("stickerId")
                .or_else(|| o.get("id"))
                .and_then(parse_u64);
            let stk_type = o
                .get("type")
                .and_then(parse_u64)
                .map(|t| t as u32)
                .unwrap_or(7);
            if let Some(sid) = stk_id {
                if cat_id.is_some()
                    || o.contains_key("stickerId")
                    || o.contains_key("catId")
                    || o.contains_key("cateId")
                {
                    return Some((sid, cat_id.unwrap_or(0), stk_type));
                }
            }
            None
        }
        _ => None,
    }
}

/// Check whether `url` points to a Zalo photo CDN asset (`photo-stal`, `photo-zmp`, `zpcloud.zdn.vn`, etc., #72).
pub fn is_zalo_photo_cdn_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    if (!u.starts_with("http://") && !u.starts_with("https://")) || u.contains("/renewmedia") {
        return false;
    }
    let path_part = u.split('?').next().unwrap_or(&u);
    if [
        ".doc", ".docx", ".pdf", ".xls", ".xlsx", ".ppt", ".pptx", ".zip", ".rar", ".7z", ".txt",
        ".csv", ".mp4", ".mov", ".mp3", ".m4a", ".exe", ".dmg", ".apk",
    ]
    .iter()
    .any(|ext| path_part.ends_with(ext))
    {
        return false;
    }
    u.contains("photo-stal")
        || u.contains("photo-zmp")
        || u.contains("photo-aka")
        || (u.contains("zdn.vn")
            && (path_part.contains("/jpg/")
                || path_part.contains("/png/")
                || path_part.contains("/webp/")
                || path_part.contains("/gif/")
                || path_part.ends_with(".jpg")
                || path_part.ends_with(".jpeg")
                || path_part.ends_with(".png")
                || path_part.ends_with(".webp")
                || path_part.ends_with(".gif")))
}

/// Sanitize and resolve the original sender filename for a Zalo file attachment (#74),
/// ensuring `/renewMedia` URLs never overwrite the sender's real filename.
pub fn resolve_attachment_file_name(title: Option<&str>, text: &str, url: &str) -> String {
    let is_usable_name = |s: &str| -> bool {
        let t = s.trim();
        !t.is_empty()
            && !t.starts_with("http://")
            && !t.starts_with("https://")
            && !t.eq_ignore_ascii_case("renewmedia")
            && !t.starts_with("renewMedia.")
            && !t.starts_with("renewmedia.")
            && !matches!(
                t,
                "[Tập tin]" | "[Chia sẻ]" | "[Hình ảnh]" | "[Video]" | "[Tin nhắn]"
            )
    };

    fn strip_prefix_tag(s: &str) -> &str {
        let t = s.trim();
        t.strip_prefix("[Tập tin]")
            .or_else(|| t.strip_prefix("[Chia sẻ]"))
            .map(str::trim)
            .unwrap_or(t)
    }

    let raw_candidate = title
        .map(strip_prefix_tag)
        .filter(|s| is_usable_name(s))
        .or_else(|| {
            let cleaned = strip_prefix_tag(text);
            is_usable_name(cleaned).then_some(cleaned)
        })
        .unwrap_or_else(|| {
            let path_seg = url
                .split('?')
                .next()
                .unwrap_or(url)
                .rsplit('/')
                .next()
                .unwrap_or("tap_tin_zalo");
            if path_seg.is_empty() || path_seg.eq_ignore_ascii_case("renewmedia") {
                "tap_tin_zalo"
            } else {
                path_seg
            }
        });

    let sanitized: String = raw_candidate
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.');
    if trimmed.is_empty() {
        "tap_tin_zalo".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Extract rich link/media card metadata (`link_url`, `link_title`, `link_desc`, `link_thumb`, `link_src`)
/// from a Zalo `chat.recommended` (`recommened.link`) or rich-object `content`.
fn extract_link_info(
    v: &Value,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    match v {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.starts_with('{') && trimmed.ends_with('}') {
                if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                    return extract_link_info(&parsed);
                }
            }
            (None, None, None, None, None)
        }
        Value::Object(o) => {
            let href = o
                .get("href")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|h| {
                    !h.is_empty() && (h.starts_with("http://") || h.starts_with("https://"))
                })
                .map(str::to_string);
            let Some(url) = href else {
                return (None, None, None, None, None);
            };
            let thumb = o
                .get("thumb")
                .or_else(|| o.get("hd"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| {
                    !t.is_empty() && (t.starts_with("http://") || t.starts_with("https://"))
                })
                .map(str::to_string);
            let mut title = o
                .get("fileName")
                .or_else(|| o.get("name"))
                .or_else(|| o.get("title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
            let desc = o
                .get("description")
                .or_else(|| o.get("caption"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string);
            let mut src = None;
            let mut file_ext: Option<String> = o
                .get("fileExt")
                .and_then(Value::as_str)
                .map(|s| s.trim().trim_start_matches('.').to_string())
                .filter(|s| !s.is_empty());
            let mut has_file_params =
                o.get("fileName").is_some() || url.to_ascii_lowercase().contains("/renewmedia");

            if let Some(params_str) = o.get("params").and_then(Value::as_str) {
                if let Ok(p_json) = serde_json::from_str::<Value>(params_str.trim()) {
                    if title.is_none() {
                        title = p_json
                            .get("fileName")
                            .or_else(|| p_json.get("mediaTitle"))
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string);
                    }
                    if file_ext.is_none() {
                        file_ext = p_json
                            .get("fileExt")
                            .and_then(Value::as_str)
                            .map(|s| s.trim().trim_start_matches('.').to_string())
                            .filter(|s| !s.is_empty());
                    }
                    if p_json.get("fileExt").is_some()
                        || p_json.get("fileSize").is_some()
                        || p_json.get("checksum").is_some()
                    {
                        has_file_params = true;
                    }
                    src = p_json
                        .get("src")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                }
            }
            if let (Some(ref mut t), Some(ref ext)) = (&mut title, &file_ext) {
                let suffix = format!(".{ext}");
                if !t
                    .to_ascii_lowercase()
                    .ends_with(&suffix.to_ascii_lowercase())
                    && !t.starts_with("http://")
                    && !t.starts_with("https://")
                {
                    t.push_str(&suffix);
                }
            }
            if src.is_none() && has_file_params {
                src = Some("Tập tin Zalo".to_string());
            }
            if src.is_none() && is_zalo_photo_cdn_url(&url) {
                src = Some("Hình ảnh Zalo".to_string());
            }
            if src.is_none() {
                src = url
                    .split("://")
                    .nth(1)
                    .and_then(|rest| rest.split('/').next())
                    .filter(|h| !h.is_empty())
                    .map(str::to_string);
            }
            (Some(url), title, desc, thumb, src)
        }
        _ => (None, None, None, None, None),
    }
}

/// Kind of attachment a chat message carries (photo / video / file / voice note).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
    File,
    Voice,
}

/// Media kind for a normalised `msgType` (see [`normalize_msg_type`]). Queue messages use
/// `chat.photo`, `chat.video.msg`, `chat.voice`, `share.file`; history uses `chat.video` /
/// `chat.file` or the numeric codes. `chat.recommended` (shared links) stays a link card.
fn media_kind_for_type(msg_type: &str) -> Option<MediaKind> {
    match msg_type {
        "chat.photo" | "photo" | "2" => Some(MediaKind::Photo),
        "chat.video" | "video" | "4" => Some(MediaKind::Video),
        t if t.starts_with("chat.video.") => Some(MediaKind::Video),
        "chat.voice" | "voice" | "3" => Some(MediaKind::Voice),
        "share.file" | "chat.file" | "file" | "19" => Some(MediaKind::File),
        _ => None,
    }
}

/// Fields of a media bubble derived from a rich `content` object (`ChatMessage::text` and the
/// `link_*` card fields). `text` is always a label (`[Hình ảnh]`, `[Video]`, …), never a URL.
#[derive(Debug, Clone, Default, PartialEq)]
struct MediaFields {
    text: String,
    link_url: Option<String>,
    link_title: Option<String>,
    link_desc: Option<String>,
    link_thumb: Option<String>,
    link_src: Option<String>,
}

/// `content` as a JSON object, whether it arrived as an object or as a JSON string.
fn rich_content_object(v: &Value) -> Option<serde_json::Map<String, Value>> {
    match v {
        Value::Object(o) => Some(o.clone()),
        Value::String(s) => {
            let t = s.trim();
            if t.starts_with('{') && t.ends_with('}') {
                match serde_json::from_str::<Value>(t) {
                    Ok(Value::Object(o)) => Some(o),
                    _ => None,
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// `content.params`: a JSON string in real messages, sometimes already an object.
fn content_params(o: &serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    match o.get("params") {
        Some(Value::Object(p)) => p.clone(),
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s.trim()) {
            Ok(Value::Object(p)) => p,
            _ => serde_json::Map::new(),
        },
        _ => serde_json::Map::new(),
    }
}

/// A trimmed `http(s)://` URL without whitespace, or `None`.
fn web_url(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    let ok = (s.starts_with("https://") || s.starts_with("http://"))
        && s.len() > "https://".len()
        && !s.chars().any(char::is_whitespace);
    ok.then(|| s.to_string())
}

/// A caption / name typed by a person: non-empty, not a URL, not a JSON blob.
fn human_text(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    let lower = s.to_ascii_lowercase();
    let reject = s.is_empty()
        || s == "sendBubbleMessage"
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("www.")
        || (s.starts_with('{') && s.ends_with('}'));
    (!reject).then(|| s.to_string())
}

/// A non-negative integer from a JSON number or numeric string (`"12345"`).
fn json_u64(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse::<u64>().ok(),
        _ => None,
    }
}

/// Build the bubble fields for a media message. `content` may be missing or not an object
/// (then only the label is set).
fn media_fields(kind: MediaKind, content: Option<&Value>) -> MediaFields {
    let label = match kind {
        MediaKind::Photo => "[Hình ảnh]",
        MediaKind::Video => "[Video]",
        MediaKind::File => "[Tập tin]",
        MediaKind::Voice => "[Tin nhắn thoại]",
    };
    let mut f = MediaFields {
        text: label.to_string(),
        ..Default::default()
    };
    let Some(o) = content.and_then(rich_content_object) else {
        return f;
    };
    let params = content_params(&o);
    let href = web_url(o.get("href"));
    let thumb = web_url(o.get("thumb"));
    // Zalo durations are milliseconds.
    let duration = json_u64(params.get("duration"))
        .filter(|ms| *ms > 0)
        .map(|ms| crate::video::format_clock(ms as f64 / 1000.0));
    let size = json_u64(params.get("fileSize").or_else(|| params.get("size")))
        .filter(|b| *b > 0)
        .map(crate::download::format_bytes);
    match kind {
        MediaKind::Photo => {
            let caption = human_text(o.get("description")).or_else(|| human_text(o.get("title")));
            let hd = web_url(params.get("hd")).filter(|u| u.starts_with("https://"));
            if let Some(c) = &caption {
                f.text = format!("{label} {c}");
            }
            f.link_url = hd.or_else(|| href.clone()).or_else(|| thumb.clone());
            f.link_thumb = thumb.or(href);
            f.link_desc = caption;
            f.link_src = Some("Hình ảnh Zalo".into());
        }
        MediaKind::Video => {
            f.link_title = human_text(o.get("title"));
            f.link_url = href;
            f.link_thumb = thumb;
            f.link_desc = match (duration, size) {
                (Some(d), Some(s)) => Some(format!("{d} · {s}")),
                (d, s) => d.or(s),
            };
            f.link_src = Some("Video Zalo".into());
        }
        MediaKind::File => {
            let name = human_text(o.get("title"))
                .or_else(|| human_text(params.get("fileName")))
                .or_else(|| human_text(o.get("fileName")))
                .or_else(|| human_text(o.get("name")));
            if let Some(n) = &name {
                f.text = format!("{label} {n}");
            }
            f.link_title = name;
            f.link_url = href;
            f.link_desc = size;
            f.link_src = Some("Tập tin Zalo".into());
        }
        MediaKind::Voice => {
            f.link_title = Some("Tin nhắn thoại".into());
            f.link_url = href;
            f.link_desc = duration;
        }
    }
    f
}

/// Media kind of a bare Zalo CDN URL, for rows stored before media messages were recognised
/// (their `text` is the URL itself). Conservative on purpose, by CDN family seen on the real
/// account (02/11):
/// * `*.zdn.vn` / `*.zadn.vn` (`photo-stal-N`, `fN-zpg-r`, `fN-zpc`, `voice-aac-dl`): a
///   photo / video extension, or a host label naming the media (`photo`, `video`, `voice`);
/// * `*.dlmd.me` (`video-stal-N`, no file extension): a host label naming `video`;
/// * `*.dlfl.vn` (`file-stal-N`, `otN`, `fgN`): Zalo's file CDN, always a file.
///
/// Everything else stays an ordinary link.
pub fn media_kind_from_legacy_url(url: &str) -> Option<MediaKind> {
    let u = url.trim();
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))?;
    if rest.chars().any(char::is_whitespace) {
        return None;
    }
    let auth_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..auth_end];
    if authority.contains('@') {
        return None;
    }
    let host = authority
        .split(':')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let on = |b: &str| host == b || host.ends_with(&format!(".{b}"));
    if on("dlfl.vn") {
        return Some(MediaKind::File);
    }
    if on("dlmd.me") {
        let labels = host.strip_suffix("dlmd.me").unwrap_or("").trim_end_matches('.');
        return labels.split('.').any(|l| l.contains("video")).then_some(MediaKind::Video);
    }
    let base = ["zdn.vn", "zadn.vn"].into_iter().find(|b| on(b))?;
    let path = rest[auth_end..]
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext = path
        .rsplit('/')
        .next()
        .and_then(|seg| seg.rsplit_once('.'))
        .map(|(_, e)| e)
        .unwrap_or("");
    match ext {
        // Only formats the image decoder can show (png / jpeg).
        "jpg" | "jpeg" | "png" => return Some(MediaKind::Photo),
        "mp4" | "mov" | "m4v" | "3gp" | "webm" | "mkv" => return Some(MediaKind::Video),
        _ => {}
    }
    let labels = host.strip_suffix(base).unwrap_or("").trim_end_matches('.');
    for label in labels.split('.') {
        if label.contains("photo") {
            return Some(MediaKind::Photo);
        }
        if label.contains("video") {
            return Some(MediaKind::Video);
        }
        if label.contains("voice") {
            return Some(MediaKind::Voice);
        }
    }
    None
}

/// Media kind of a stored row that predates media parsing: its `link_url` is a Zalo media URL
/// ([`media_kind_from_legacy_url`]) and `text` is that URL — or, for files, the file name
/// (older builds took the attachment title as text). Rows that already carry a media marker,
/// a label (`[…]`) or a sticker are not legacy rows.
pub fn legacy_media_kind(msg: &ChatMessage) -> Option<MediaKind> {
    if msg.sticker_id.is_some() {
        return None;
    }
    if matches!(
        msg.link_src.as_deref(),
        Some("Hình ảnh Zalo" | "Video Zalo" | "Tập tin Zalo")
    ) {
        return None;
    }
    let url = msg.link_url.as_deref()?.trim();
    let text = msg.text.trim();
    if url.is_empty() || text.starts_with('[') {
        return None;
    }
    let kind = media_kind_from_legacy_url(url)?;
    (text == url || kind == MediaKind::File).then_some(kind)
}

/// Bubble text for a message QTzalo cannot read: Zalo's end-to-end (Signal) encrypted payloads.
///
/// [I] Not yet observed on a real account (61/61 groups reported `e2eeStatus = 0` on 03/11), so
/// this is a display-safety placeholder, not a feature: the message is shown as unreadable here
/// and the user is pointed at the phone, which holds the keys.
pub const E2EE_PLACEHOLDER_TEXT: &str = "[Tin nhắn mã hoá đầu cuối — xem trên điện thoại]";

/// Payload keys that can carry the end-to-end-encryption flag of one message.
const E2EE_FLAG_KEYS: &[&str] = &["e2eeStatus", "e2ee"];

/// `true` only when a message payload states **explicitly** that its content is E2EE.
///
/// [I] Accepted evidence, nothing else: the boolean `true`, a non-zero number, or a numeric string
/// other than `"0"`/`""` under [`E2EE_FLAG_KEYS`]. Deliberately *not* inferred from payload
/// length, alphabet (base64-looking text), `msgType` or unknown fields: mislabelling an ordinary
/// message as encrypted replaces readable text with a "xem trên điện thoại" bubble, which is worse
/// than showing one ciphertext bubble by mistake. A `false`/`0`/`""` value, a missing field and any
/// unparsable string all mean "not encrypted" as far as this client can tell.
pub fn e2ee_placeholder_needed(payload: &Value) -> bool {
    E2EE_FLAG_KEYS.iter().any(|key| match payload.get(*key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(false),
        Some(Value::String(s)) => {
            let t = s.trim();
            !t.is_empty() && t != "0" && t.parse::<f64>().map(|f| f != 0.0).unwrap_or(false)
        }
        _ => false,
    })
}

fn parse_chat_message(it: &Value) -> Option<ChatMessage> {
    let msg_type = normalize_msg_type(it);
    let is_e2ee = e2ee_placeholder_needed(it);

    let raw_content_val = it
        .get("content")
        .or_else(|| {
            it.get("message")
                .and_then(|m| m.get("content"))
                .or_else(|| it.get("message"))
        })
        .or_else(|| it.get("msg"));

    let mut sticker_meta = raw_content_val.and_then(extract_sticker_info);
    if sticker_meta.is_none()
        && (msg_type == "chat.sticker" || msg_type == "sticker" || msg_type == "5")
    {
        sticker_meta = extract_sticker_info(it);
    }
    let (mut link_url, mut link_title, mut link_desc, mut link_thumb, mut link_src) = raw_content_val
        .map(extract_link_info)
        .unwrap_or((None, None, None, None, None));

    let mut content = raw_content_val
        .and_then(extract_content_text)
        .unwrap_or_default();

    let is_photo_msg = matches!(msg_type.as_str(), "chat.photo" | "photo" | "2")
        || link_src.as_deref() == Some("Hình ảnh Zalo")
        || link_url.as_deref().is_some_and(is_zalo_photo_cdn_url)
        || link_thumb.as_deref().is_some_and(is_zalo_photo_cdn_url);
    let is_file_msg = matches!(
        msg_type.as_str(),
        "chat.file" | "file" | "19" | "share.file"
    ) || link_src.as_deref() == Some("Tập tin Zalo")
        || link_url
            .as_deref()
            .is_some_and(|u| u.to_ascii_lowercase().contains("/renewmedia"));

    if is_photo_msg && !is_file_msg && sticker_meta.is_none() {
        link_src = Some("Hình ảnh Zalo".to_string());
        if link_thumb.is_none() {
            link_thumb = link_url.clone();
        }
        let trimmed_c = content.trim();
        let is_raw_url_content =
            trimmed_c.starts_with("http://") || trimmed_c.starts_with("https://");
        if trimmed_c.is_empty() || is_raw_url_content {
            if let Some(cap) = link_desc.as_deref().map(str::trim).filter(|d| {
                !d.is_empty() && !d.starts_with("http://") && !d.starts_with("https://")
            }) {
                content = format!("[Hình ảnh] {cap}");
            } else {
                content = "[Hình ảnh]".to_string();
            }
        }
    } else if is_file_msg && sticker_meta.is_none() {
        link_src = Some("Tập tin Zalo".to_string());
        if link_url.is_some() || link_title.is_some() {
            let resolved_name = resolve_attachment_file_name(
                link_title.as_deref(),
                &content,
                link_url.as_deref().unwrap_or_default(),
            );
            if link_title.is_none()
                || link_title
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case("renewmedia"))
            {
                link_title = Some(resolved_name.clone());
            }
            let final_title = link_title.as_deref().unwrap_or(&resolved_name);
            let trimmed_c = content.trim();
            if !trimmed_c.starts_with("[Tập tin]") {
                content = format!("[Tập tin] {final_title}");
            }
        }
    }
    let is_undo_type = matches!(msg_type.as_str(), "chat.undo" | "undo" | "31");
    // An E2EE payload names no readable content, so it is never an undo of another message.
    let is_undo_type = !is_e2ee && matches!(msg_type.as_str(), "chat.undo" | "undo" | "31");
    if is_undo_type {
        content = "[Tin nhắn đã thu hồi]".to_string();
        sticker_meta = None;
    } else if sticker_meta.is_some() && (content.is_empty() || content.starts_with('{')) {
        content = "[Nhãn dán]".to_string();
    } else if let Some(kind) = media_kind_for_type(&msg_type).filter(|_| sticker_meta.is_none()) {
        // Photos / videos / files / voice notes: a label plus card fields, never the raw CDN
        // URL as text (photo `title` / `description` are usually empty, so the generic path
        // above would have returned `href`).
        let f = media_fields(kind, raw_content_val);
        content = f.text;
        link_url = f.link_url;
        link_title = f.link_title;
        link_desc = f.link_desc;
        link_thumb = f.link_thumb;
        link_src = f.link_src;
    }
    if sticker_meta.is_none() {
        let parse_ami = |s: &str| -> Option<(u64, u64, u32)> {
            let pos = s.find("ami_fat_cat_")?;
            let num_str = s.get(pos + 12..pos + 14)?;
            let idx = num_str.parse::<u64>().ok()?;
            if (1..=15).contains(&idx) {
                Some((999000 + idx, 99901, 7))
            } else {
                None
            }
        };
        if let Some(m) = parse_ami(&content)
            .or_else(|| link_desc.as_deref().and_then(parse_ami))
            .or_else(|| link_title.as_deref().and_then(parse_ami))
            .or_else(|| link_url.as_deref().and_then(parse_ami))
        {
            sticker_meta = Some(m);
            content = "[Nhãn dán]".to_string();
        }
    }

    if content.is_empty() {
        content = match msg_type.as_str() {
            "chat.sticker" | "sticker" | "5" => "[Nhãn dán]".to_string(),
            "chat.photo" | "photo" | "2" => "[Hình ảnh]".to_string(),
            "chat.voice" | "voice" | "3" => "[Tin nhắn thoại]".to_string(),
            "chat.video" | "video" | "4" => "[Video]".to_string(),
            "chat.file" | "file" | "19" => "[Tập tin]".to_string(),
            "chat.location" | "location" | "24" => "[Vị trí]".to_string(),
            "chat.contact" | "contact" => "[Danh thiếp]".to_string(),
            "chat.undo" | "undo" | "31" => "[Tin nhắn đã thu hồi]".to_string(),
            "chat.recommended" => "[Chia sẻ]".to_string(),
            _ => {
                let obj = it.get("content").or_else(|| it.get("message"));
                if let Some(o) = obj {
                    if o.get("catId").is_some()
                        || o.get("stickerId").is_some()
                        || o.get("id").is_some()
                    {
                        "[Nhãn dán]".to_string()
                    } else if o.get("href").is_some() || o.get("thumb").is_some() {
                        "[Hình ảnh]".to_string()
                    } else if o.is_object() {
                        "[Tin nhắn]".to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
        };
    }

    let undo_target_id = if is_undo_type {
        raw_content_val.and_then(|rv| match rv {
            Value::Object(_) => pick_str(rv, &["globalMsgId", "msgId", "cliMsgId"]),
            Value::String(s) if s.trim().starts_with('{') => {
                serde_json::from_str::<Value>(s.trim())
                    .ok()
                    .and_then(|parsed| pick_str(&parsed, &["globalMsgId", "msgId", "cliMsgId"]))
            }
            _ => None,
        })
    } else {
        None
    };

    // Preserve the persisted canonical ID contract until a schema-aware migration
    // can replace legacy cliMsgId keys without duplicating cached messages.
    let msg_id = undo_target_id
        .or_else(|| {
            ["msgId", "cliMsgId", "globalMsgId", "lastMsgId", "id"]
                .iter()
                .find_map(|key| wire_identity(it.get(*key)))
        })
        .unwrap_or_default();

    if content.is_empty() && msg_id.is_empty() {
        return None;
    }

    let uid_from = it
        .get("uidFrom")
        .or_else(|| it.get("fromUid"))
        .or_else(|| it.get("fromId"))
        .or_else(|| it.get("senderId"))
        .or_else(|| it.get("sender"))
        .and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .unwrap_or_default();

    let display_sender = it
        .get("dName")
        .or_else(|| it.get("displayName"))
        .or_else(|| it.get("fromName"))
        .or_else(|| it.get("senderName"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uid_from.clone());

    let is_outgoing = uid_from == "0"
        || uid_from.is_empty()
        || it
            .get("isOutgoing")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || it
            .get("is_outgoing")
            .and_then(Value::as_i64)
            .map(|n| n == 1)
            .unwrap_or(false);

    let ts = it
        .get("ts")
        .or_else(|| it.get("cliMsgId"))
        .or_else(|| it.get("sendTime"))
        .or_else(|| it.get("serverTime"))
        .or_else(|| it.get("time"))
        .and_then(|v| match v {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.parse::<i64>().ok(),
            _ => None,
        })
        .unwrap_or(0);

    let time = if ts > 0 {
        crate::clock::message_time(ts)
    } else {
        crate::ui::clock()
    };

    let (reply_to_id, reply_to_sender, reply_to_text) =
        if let Some(q) = it.get("quote").filter(|v| v.is_object()) {
            let q_id = pick_str(q, &["cliMsgId", "globalMsgId", "msgId"]);
            let q_sender = pick_str(q, &["fromD", "dName", "ownerId", "fromUid", "uidFrom"]);
            let q_text = pick_str(q, &["msg", "message", "content"]).or_else(|| {
                q.get("msg")
                    .or_else(|| q.get("content"))
                    .and_then(extract_content_text)
            });
            (q_id, q_sender, q_text)
        } else {
            (None, None, None)
        };

    let mut parsed = ChatMessage {
        id: msg_id,
        client_message_id: ["cliMsgId", "clientId", "client_message_id"]
            .iter()
            .find_map(|key| wire_identity(it.get(*key)))
            .unwrap_or_default(),
        wire_ref: crate::message_model::WireMessageRef::from_raw(it),
        sender: display_sender,
        text: content,
        time,
        is_outgoing,
        ts,
        sticker_id: sticker_meta.map(|(sid, _, _)| sid),
        sticker_cate_id: sticker_meta.map(|(_, cid, _)| cid),
        sticker_type: sticker_meta.map(|(_, _, st)| st),
        reply_to_id,
        reply_to_sender,
        reply_to_text,
        reactions: extract_reactions(it),
        is_pinned: false,
        is_starred: false,
        link_url,
        link_title,
        link_desc,
        link_thumb,
        link_src,
        ..Default::default()
    };

    // [I] End-to-end encrypted payload: never decrypt, never guess. Show the placeholder and drop
    // everything that could leak ciphertext or mis-render it (sticker ids, card fields, quote).
    // `id`/`cli_msg_id`/`sender`/`ts` stay: they are metadata the server sends in clear and are
    // what de-duplication, ordering and reactions need.
    if is_e2ee {
        parsed.text = E2EE_PLACEHOLDER_TEXT.to_string();
        parsed.sticker_id = None;
        parsed.sticker_cate_id = None;
        parsed.sticker_type = None;
        parsed.reply_to_id = None;
        parsed.reply_to_sender = None;
        parsed.reply_to_text = None;
        parsed.link_url = None;
        parsed.link_title = None;
        parsed.link_desc = None;
        parsed.link_thumb = None;
        parsed.link_src = None;
    }

    Some(parsed)
}

fn extract_reactions(it: &Value) -> Vec<String> {
    let mut res = Vec::new();
    if let Some(r) = it.get("reacts").or_else(|| it.get("reactions")) {
        if let Some(arr) = r.get("reactions").and_then(Value::as_array) {
            for item in arr {
                if let Some(icon) = item.get("rIcon").and_then(Value::as_str) {
                    let em = icon_to_emoji(icon);
                    if !res.contains(&em.to_string()) {
                        res.push(em.to_string());
                    }
                }
            }
        }
    }
    res
}

fn is_group_flag(v: &Value) -> bool {
    v.get("isGroup")
        .map(|g| g.as_bool() == Some(true) || g.as_i64() == Some(1))
        .unwrap_or(false)
}

fn extract_message_thread_id_ext(
    entry: &Value,
    m_val: &Value,
    in_group_bucket: bool,
    cloud_thread_id: Option<&str>,
) -> Option<(String, bool)> {
    let is_group = in_group_bucket || is_group_flag(m_val) || is_group_flag(entry);

    // 1. Explicit `grid` field or `groupMsgs` bucket (`e.msgKey = GROUPID_PREFIX + e.idTo` in `sync-v2-sub-worker.js:3500340`)
    for obj in [m_val, entry] {
        if let Some(grid) = pick_str(obj, &["grid", "groupId"]) {
            if !grid.is_empty() && grid != "0" {
                let clean = grid.trim_start_matches("group_").to_string();
                return Some((clean, true));
            }
        }
    }
    if is_group {
        if let Some(gid) = pick_str(m_val, &["idTo", "toid", "toId", "threadId", "conversId"])
            .or_else(|| pick_str(entry, &["idTo", "toid", "toId", "threadId", "conversId"]))
        {
            if !gid.is_empty() && gid != "0" {
                let clean = gid.trim_start_matches("group_").to_string();
                return Some((clean, true));
            }
        }
    }

    // 2. 1-on-1 or self ("Cloud của tôi") message routing
    // [O] `sync-v2-sub-worker.js:3500260`:
    //   `"0" == e.idTo && "0" == e.uidFrom && (e.idTo = "" + this.userId)`
    // [O] `sync-v2-sub-worker.js:829500` (`_filterDuplicatedMessages`):
    //   `e.isGroup ? e.msgKey : "0" == e.idTo ? e.uidFrom : e.idTo`
    let uid_from = pick_str(m_val, &["uidFrom", "fromUid", "fromId", "senderId"])
        .or_else(|| pick_str(entry, &["uidFrom", "fromUid", "fromId", "senderId"]))
        .unwrap_or_default();
    let id_to = pick_str(m_val, &["idTo", "toid", "toId"])
        .or_else(|| pick_str(entry, &["idTo", "toid", "toId"]))
        .unwrap_or_default();

    if (id_to.is_empty() || id_to == "0") && (uid_from.is_empty() || uid_from == "0") {
        if let Some(cid) = cloud_thread_id {
            if !cid.is_empty() {
                return Some((cid.to_string(), false));
            }
        }
    }

    if !uid_from.is_empty()
        && uid_from != "0"
        && (id_to.is_empty() || id_to == "0" || Some(id_to.as_str()) == cloud_thread_id)
    {
        // Incoming 1-1 message: peer sent it to us -> thread ID is `uid_from`
        Some((uid_from, false))
    } else if !id_to.is_empty() && id_to != "0" {
        // Outgoing 1-1 message or conversation wrapper with explicit `idTo` -> thread ID is `id_to`
        Some((id_to, false))
    } else if !uid_from.is_empty() && uid_from != "0" {
        Some((uid_from, false))
    } else {
        pick_str(
            m_val,
            &["threadId", "conversId", "userId", "friendId", "id"],
        )
        .or_else(|| {
            pick_str(
                entry,
                &["threadId", "conversId", "userId", "friendId", "id"],
            )
        })
        .filter(|s| s != "0" && !s.is_empty())
        .map(|s| (s, is_group))
    }
}

/// Fold a `get-last-msgs` or `getChatFromPolling` response into thread previews and conversation messages.
pub fn merge_previews(threads: &mut Vec<Thread>, resp: &Value) {
    let mut entries: Vec<(&Value, bool)> = Vec::new();
    let parsed_str_data: Option<Value> = resp
        .get("data")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s.trim()).ok());
    let mut data_node = parsed_str_data
        .as_ref()
        .unwrap_or_else(|| resp.get("data").unwrap_or(resp));
    if let Some(inner_data) = data_node
        .get("data")
        .filter(|v| v.is_object() || v.is_array())
    {
        data_node = inner_data;
    }
    let cloud_tid: Option<String> = threads
        .iter()
        .find(|t| t.name.contains("Cloud của tôi"))
        .map(|t| t.id.clone());

    if let Some(arr) = data_node.as_array() {
        for item in arr {
            entries.push((item, is_group_flag(item)));
        }
    } else if let Some(obj) = data_node.as_object() {
        let mut found_list = false;
        for (key, is_grp) in [
            ("msgs", false),
            ("groupMsgs", true),
            ("pageMsgs", false),
            ("conversations", false),
            ("threads", false),
            ("allmsgs", false),
        ] {
            if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                for item in arr {
                    entries.push((item, is_grp || is_group_flag(item)));
                }
                found_list = true;
            }
        }
        if let Some(cu_arr) = obj.get("clearUnreads").and_then(Value::as_array) {
            // clearUnreads updates read points; do not auto-create empty stranger threads without messages.
            if !cu_arr.is_empty() {
                found_list = true;
            }
        }
        if !found_list {
            for (_, val) in obj {
                if val.is_object() {
                    entries.push((val, is_group_flag(val)));
                }
            }
        }
    }

    let mut message_indices: Vec<MessageIdentityIndex> = threads
        .iter()
        .map(|thread| {
            let mut index = MessageIdentityIndex::default();
            for (row, message) in thread.messages.iter().enumerate() {
                index.insert(message, row);
            }
            index
        })
        .collect();

    for (entry, is_grp_bucket) in entries {
        let mut raw_msgs: Vec<(&Value, bool)> = Vec::new();
        if let Some(arr) = entry.get("messages").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, is_grp_bucket || is_group_flag(m)));
            }
        }
        if let Some(arr) = entry.get("msgs").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, is_grp_bucket || is_group_flag(m)));
            }
        }
        if let Some(arr) = entry.get("groupMsgs").and_then(Value::as_array) {
            for m in arr {
                raw_msgs.push((m, true));
            }
        }
        if let Some(obj) = entry.get("lastMsg").filter(|v| v.is_object()) {
            raw_msgs.push((obj, is_grp_bucket || is_group_flag(obj)));
        }
        if let Some(obj) = entry.get("message").filter(|v| v.is_object()) {
            raw_msgs.push((obj, is_grp_bucket || is_group_flag(obj)));
        }
        if raw_msgs.is_empty()
            && (entry.get("content").is_some()
                || entry.get("msg").is_some()
                || entry.get("msgId").is_some())
        {
            raw_msgs.push((entry, is_grp_bucket));
        }

        for (m_val, is_grp) in raw_msgs {
            let Some(parsed) = parse_chat_message(m_val) else {
                continue;
            };

            let Some((tid, thread_is_group)) =
                extract_message_thread_id_ext(entry, m_val, is_grp, cloud_tid.as_deref())
            else {
                continue;
            };

            if let Some(thread_index) = threads.iter().position(|thread| thread.id == tid) {
                let t = &mut threads[thread_index];
                let identities = &mut message_indices[thread_index];
                if thread_is_group {
                    t.is_group = true;
                }
                if let Some(row) = identities.find(&t.messages, &parsed) {
                    let existing = &mut t.messages[row];
                    identities.remove(existing, row);
                    existing.merge_reaction_metadata(&parsed);
                    if existing.id.starts_with("out_")
                        && !parsed.id.is_empty()
                        && !parsed.id.starts_with("out_")
                    {
                        existing.id = parsed.id;
                    }
                    if parsed.ts > 0 && existing.ts != parsed.ts {
                        existing.ts = parsed.ts;
                        existing.time = parsed.time;
                    }
                    if parsed.text.trim() == "[Tin nhắn đã thu hồi]" {
                        existing.text = "[Tin nhắn đã thu hồi]".to_string();
                        existing.sticker_id = None;
                        existing.link_url = None;
                        existing.link_thumb = None;
                    }
                    if existing.sticker_id.is_none() && parsed.sticker_id.is_some() {
                        existing.sticker_id = parsed.sticker_id;
                        existing.sticker_cate_id = parsed.sticker_cate_id;
                        existing.sticker_type = parsed.sticker_type;
                    }
                    if existing.reply_to_text.is_none() && parsed.reply_to_text.is_some() {
                        existing.reply_to_id = parsed.reply_to_id;
                        existing.reply_to_sender = parsed.reply_to_sender;
                        existing.reply_to_text = parsed.reply_to_text;
                    }
                    if existing.link_url.is_none() && parsed.link_url.is_some() {
                        existing.link_url = parsed.link_url;
                        existing.link_title = parsed.link_title;
                        existing.link_desc = parsed.link_desc;
                        existing.link_thumb = parsed.link_thumb;
                        existing.link_src = parsed.link_src;
                        if !parsed.text.is_empty() {
                            existing.text = parsed.text;
                        }
                    }
                    if !parsed.reactions.is_empty() {
                        existing.reactions = parsed.reactions;
                    }
                    existing.is_pinned = existing.is_pinned || parsed.is_pinned;
                    existing.is_starred = existing.is_starred || parsed.is_starred;
                    identities.insert(existing, row);
                } else {
                    identities.insert(&parsed, t.messages.len());
                    t.messages.push(parsed);
                }
            } else {
                // New thread detected from incoming message!
                let default_name = if thread_is_group {
                    format!("Nhóm {}", tid)
                } else {
                    format!("Người dùng {}", tid)
                };
                let mut identities = MessageIdentityIndex::default();
                identities.insert(&parsed, 0);
                message_indices.push(identities);
                threads.push(Thread {
                    presentation: Default::default(),
                    unread: Default::default(),
                    id: tid,
                    name: default_name,
                    preview: parsed.text.clone(),
                    time: parsed.time.clone(),
                    avatar: None,
                    is_group: thread_is_group,
                    messages: vec![parsed],
                });
            }
        }
    }

    for t in threads.iter_mut() {
        if !t.messages.is_empty() {
            t.messages.sort_by_key(|m| m.ts);
            if let Some(newest) = t.messages.last() {
                if !newest.text.is_empty() {
                    t.preview = newest.text.replace('\n', " ");
                    t.time = newest.time.clone();
                }
            }
        }
    }
}

/// Merge group history response into a thread's messages, sorted by timestamp and deduplicated by msg_id.
pub fn merge_group_history(thread: &mut Thread, resp: &Value) {
    let mut raw_msgs: Vec<&Value> = Vec::new();
    let parsed_str_data: Option<Value> = resp
        .get("data")
        .and_then(Value::as_str)
        .and_then(|s| serde_json::from_str::<Value>(s.trim()).ok());
    let mut data_node = parsed_str_data
        .as_ref()
        .unwrap_or_else(|| resp.get("data").unwrap_or(resp));
    if let Some(inner_data) = data_node
        .get("data")
        .filter(|v| v.is_object() || v.is_array())
    {
        data_node = inner_data;
    }

    if let Some(arr) = data_node.as_array() {
        raw_msgs.extend(arr);
    } else if let Some(obj) = data_node.as_object() {
        for key in ["msgs", "groupMsgs", "messages"] {
            if let Some(arr) = obj.get(key).and_then(Value::as_array) {
                raw_msgs.extend(arr);
            }
        }
    }

    let clean_thread_id = thread
        .id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .to_string();

    let mut identities = MessageIdentityIndex::default();
    for (row, message) in thread.messages.iter().enumerate() {
        identities.insert(message, row);
    }

    for it in raw_msgs {
        if !clean_thread_id.is_empty() {
            if let Some(id_to) = pick_str(it, &["idTo", "toId", "grid", "groupId"]) {
                let clean_to = id_to
                    .trim_start_matches("group_")
                    .trim_start_matches('g')
                    .trim_start_matches('G');
                if !clean_to.is_empty() && clean_to != "0" && clean_to != clean_thread_id {
                    continue;
                }
            }
        }
        if let Some(m) = parse_chat_message(it) {
            if let Some(row) = identities.find(&thread.messages, &m) {
                let existing = &mut thread.messages[row];
                identities.remove(existing, row);
                existing.merge_reaction_metadata(&m);
                if existing.id.starts_with("out_") && !m.id.is_empty() && !m.id.starts_with("out_")
                {
                    existing.id = m.id.clone();
                }
                if existing.link_url.is_none() && m.link_url.is_some() {
                    existing.link_url = m.link_url;
                    existing.link_title = m.link_title;
                    existing.link_desc = m.link_desc;
                    existing.link_thumb = m.link_thumb;
                    existing.link_src = m.link_src;
                    if !m.text.is_empty() {
                        existing.text = m.text;
                    }
                }
                identities.insert(existing, row);
            } else {
                identities.insert(&m, thread.messages.len());
                thread.messages.push(m);
            }
        }
    }

    thread.messages.sort_by_key(|m| m.ts);

    if let Some(newest) = thread.messages.last() {
        if !newest.text.is_empty() {
            thread.preview = newest.text.replace('\n', " ");
            thread.time = newest.time.clone();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FriendshipStatus {
    #[default]
    Unknown,
    Friend,
    OutgoingPending,
    IncomingPending,
    Stranger,
}

/// Parse `/api/friend/reqstatus` (`cmd 11430`) JSON into `FriendshipStatus`
/// (`compact-app-pc.js:8351970`: `is_friend`, `is_requested` = I requested you, `is_requesting` = you requested me).
pub fn parse_friendship_status_response(resp: &Value) -> FriendshipStatus {
    let d = resp
        .get("data")
        .and_then(|x| x.get("data").or(Some(x)))
        .unwrap_or(resp);
    let is_friend = d
        .get("is_friend")
        .or_else(|| d.get("isFriend"))
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_bool().map(|b| if b { 1 } else { 0 }))
        })
        .unwrap_or(0);
    if is_friend == 1 {
        return FriendshipStatus::Friend;
    }
    let is_requesting = d
        .get("is_requesting")
        .or_else(|| d.get("isRequesting"))
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_bool().map(|b| if b { 1 } else { 0 }))
        })
        .unwrap_or(0);
    if is_requesting == 1 {
        return FriendshipStatus::IncomingPending;
    }
    let is_requested = d
        .get("is_requested")
        .or_else(|| d.get("isRequested"))
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_bool().map(|b| if b { 1 } else { 0 }))
        })
        .unwrap_or(0);
    if is_requested == 1 {
        return FriendshipStatus::OutgoingPending;
    }
    FriendshipStatus::Stranger
}

/// Query `/api/friend/reqstatus` (`cmd 11430`) and return the typed `FriendshipStatus`.
pub async fn check_friendship_status(s: &Session, fid: &str) -> Result<FriendshipStatus> {
    let resp = get_friend_request_status(s, fid).await?;
    Ok(parse_friendship_status_response(&resp))
}
/// Check friend request status with a target user (`/api/friend/reqstatus`, cmd `11430`).
pub async fn get_friend_request_status(s: &Session, fid: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[("fid", json!(fid)), ("imei", json!(s.imei))]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        &s.domains.friend
    };
    let url = format!(
        "{}/api/friend/reqstatus?{}&params={enc}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.get(&url).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Query `/api/social/profile/lastOnline` (`cmd 12090`) and return `(is_online, label)`
/// matching Zalo PC's `getLastActiveText` (`user_active_time = 5 minutes`).
pub async fn get_user_online_status(s: &Session, uid: &str) -> Result<(bool, String)> {
    if s.login.is_none() || uid.trim().is_empty() {
        return Ok((false, String::new()));
    }
    let body = obj(&[
        ("uid", json!(uid.trim())),
        ("conv_type", json!(1)),
        ("imei", json!(s.imei)),
    ]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.profile.is_empty() {
        "https://tt-profile-wpa.chat.zalo.me"
    } else {
        &s.domains.profile
    };
    let url = format!(
        "{}/api/social/profile/lastOnline?{}&params={enc}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let mut dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(d_str) = dec.get("data").and_then(Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<Value>(d_str.trim()) {
            dec["data"] = parsed;
        }
    }
    let d = dec
        .pointer("/data/data")
        .or_else(|| dec.get("data"))
        .unwrap_or(&dec);
    let show_online = d
        .pointer("/settings/show_online_status")
        .and_then(|v| v.as_bool().or_else(|| v.as_i64().map(|n| n != 0)))
        .unwrap_or(true);
    if !show_online {
        return Ok((false, String::new()));
    }
    let last_onl_raw = d
        .get("lastOnline")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<i64>().ok()))
        })
        .unwrap_or(0);
    if last_onl_raw <= 0 {
        return Ok((false, String::new()));
    }
    let last_sec = if last_onl_raw > 10_000_000_000 {
        last_onl_raw / 1000
    } else {
        last_onl_raw
    };
    let now_sec = crate::login::unix_now() as i64;
    let diff_sec = (now_sec - last_sec).max(0);
    if diff_sec <= 300 {
        return Ok((true, "Vừa mới truy cập".into()));
    }
    let mins = diff_sec / 60;
    if mins < 60 {
        return Ok((false, format!("Truy cập {mins} phút trước")));
    }
    let hours = diff_sec / 3600;
    if hours < 24 {
        return Ok((false, format!("Truy cập {hours} giờ trước")));
    }
    let days = (diff_sec / 86400).min(7);
    Ok((false, format!("Truy cập {days} ngày trước")))
}

/// Send a friend request to a target user (`/api/friend/sendreq`, cmd `11424`).
pub async fn send_friend_request(s: &Session, to_id: &str, msg: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[
        ("toid", json!(to_id)),
        ("msg", json!(msg)),
        ("reqsrc", json!(30)),
        ("imei", json!(s.imei)),
        ("language", json!("vi")),
    ]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        &s.domains.friend
    };
    let url = format!(
        "{}/api/friend/sendreq?{}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={enc}")).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = dec.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let err_msg = dec
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("sendreq error {code}: {err_msg}");
        }
    }
    Ok(dec)
}

/// Accept a pending incoming friend request (`/api/friend/accept`, cmd `11427`).
pub async fn accept_friend_request(s: &Session, fid: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[("fid", json!(fid)), ("language", json!("vi"))]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        &s.domains.friend
    };
    let url = format!(
        "{}/api/friend/accept?{}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={enc}")).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = dec.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            let err_msg = dec
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            anyhow::bail!("accept error {code}: {err_msg}");
        }
    }
    Ok(dec)
}

/// Cancel / undo a pending outgoing friend request (`/api/friend/undo`, cmd `11441`).
pub async fn undo_friend_request(s: &Session, fid: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[("fid", json!(fid)), ("imei", json!(s.imei))]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        &s.domains.friend
    };
    let url = format!(
        "{}/api/friend/undo?{}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={enc}")).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    // Cancelling a request is a write action: a refusal must not look like success.
    ensure_action_ok("Huỷ lời mời kết bạn", "chat::undo_friend_request", &dec)?;
    Ok(dec)
}

/// Reject a pending incoming friend request (`/api/friend/reject`, cmd `11428`).
pub async fn reject_friend_request(s: &Session, fid: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = obj(&[("fid", json!(fid)), ("imei", json!(s.imei))]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        &s.domains.friend
    };
    let url = format!(
        "{}/api/friend/reject?{}",
        host.trim_end_matches('/'),
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={enc}")).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    // Same rule as `undo_friend_request`: rejecting is a write, report a refusal.
    ensure_action_ok("Từ chối lời mời kết bạn", "chat::reject_friend_request", &dec)?;
    Ok(dec)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PhoneLookupResult {
    pub user_id: String,
    pub display_name: String,
    pub zalo_name: String,
    pub phone: String,
    pub avatar: String,
    pub status: String,
    #[serde(default)]
    pub friendship: FriendshipStatus,
}

/// Normalize a Vietnamese or international phone string for Zalo's `/api/friend/profile/get`.
///
/// Converts leading `0` to `84` (e.g. `0912345678` -> `84912345678`), strips `+`, spaces, dots, and dashes.
pub fn normalize_phone_for_lookup(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if let Some(rest) = digits.strip_prefix('0') {
        format!("84{rest}")
    } else {
        digits
    }
}

/// Look up a Zalo user profile by phone number (`getUserByPhone` in `compact-app-pc.js:14306443`, cmd `11420`).
///
/// Endpoint: `GET {domains.friend}/api/friend/profile/get?{common}&params={encodeAES({ phone, avatar_size: 240, language: "vi", imei, reqSrc: 40 })}`
pub async fn find_user_by_phone(s: &Session, raw_phone: &str) -> Result<PhoneLookupResult> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let normalized = normalize_phone_for_lookup(raw_phone);
    if normalized.len() < 8 {
        anyhow::bail!("Số điện thoại không hợp lệ (cần ít nhất 9 chữ số)");
    }
    let body = obj(&[
        ("phone", json!(normalized)),
        ("avatar_size", json!(240)),
        ("language", json!("vi")),
        ("imei", json!(s.imei)),
        ("reqSrc", json!(40)),
    ]);
    let enc = s.encoded_params(&body)?;
    let host = if s.domains.friend.is_empty() {
        "https://tt-friend-wpa.chat.zalo.me"
    } else {
        s.domains.friend.trim_end_matches('/')
    };
    let url = format!(
        "{host}/api/friend/profile/get?{}&params={enc}",
        s.common_params()
    );
    let resp = s.get(&url).await?;
    let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
    if let Some(code) = dec.get("error_code").and_then(Value::as_i64) {
        if code != 0 {
            if code == 216 {
                anyhow::bail!(
                    "Số điện thoại chưa đăng ký Zalo hoặc người dùng đã chặn tìm kiếm qua SĐT (mã 216)"
                );
            }
            let err_msg = dec
                .get("error_message")
                .and_then(Value::as_str)
                .unwrap_or("Không tìm thấy tài khoản");
            anyhow::bail!("{err_msg} (mã {code})");
        }
    }

    let data = dec.get("data").unwrap_or(&dec);
    let Some(uid) = pick_str(data, &["uid", "userId", "id", "globalId"]) else {
        anyhow::bail!("Không tìm thấy tài khoản Zalo ứng với số điện thoại này");
    };
    let display_name = pick_str(
        data,
        &["display_name", "displayName", "zalo_name", "zaloName"],
    )
    .unwrap_or_else(|| raw_phone.trim().to_string());
    let zalo_name = pick_str(
        data,
        &["zalo_name", "zaloName", "display_name", "displayName"],
    )
    .unwrap_or_else(|| display_name.clone());
    let avatar = pick_str(data, &["avatar", "avt"]).unwrap_or_default();
    let status = pick_str(data, &["status"]).unwrap_or_default();

    let friendship = check_friendship_status(s, &uid)
        .await
        .unwrap_or(FriendshipStatus::Unknown);

    Ok(PhoneLookupResult {
        user_id: uid,
        display_name,
        zalo_name,
        phone: raw_phone.trim().to_string(),
        avatar,
        status,
        friendship,
    })
}

/// Pin or unpin a conversation (`updatePinnedConversationsV2` @ `compact-app-pc.js:14354215`, cmd `12122`).
pub async fn update_pinned_conversation(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    pin: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim_start_matches('u')
        .trim();
    let prefixed = if is_group {
        format!("g{clean}")
    } else {
        format!("u{clean}")
    };
    let body = json!({
        "actionType": if pin { 1 } else { 2 },
        "conversations": [prefixed],
        "tab": 0
    });
    let params = s.encoded_params(&body)?;
    let url = format!(
        "{}/api/pinconvers/updatev2?{}",
        s.domains.conversation,
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={params}")).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Send a real-time "is typing" indicator to a 1-1 contact (`cmd 11144`, `/api/message/typing`)
/// or a group (`cmd 11196`, `/api/group/typing`), unless blocked by `block_typing`.
pub async fn send_typing_indicator(
    s: &Session,
    thread_id: &str,
    is_group: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    if crate::session::privacy_blocker_state().0 {
        return Ok(json!({"error_code": 0, "data": {}}));
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim_start_matches('u')
        .trim();
    if clean.is_empty() {
        return Ok(json!({"error_code": 0, "data": {}}));
    }
    let (endpoint, path_and_q, body) = if is_group {
        (
            "group",
            format!("/api/group/typing?{}", s.common_params()),
            json!({
                "grid": clean,
                "imei": s.imei,
            }),
        )
    } else {
        (
            "chat",
            format!("/api/message/typing?{}", s.common_params()),
            json!({
                "toid": clean,
                "destType": 0,
                "imei": s.imei,
            }),
        )
    };
    let params = s.encoded_params(&body)?;
    let resp = s
        .post_endpoint_form(endpoint, &path_and_q, format!("params={params}"))
        .await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Send a "seen" receipt (`cmd 12118` `/api/message/seenv2` or `cmd 12124` `/api/group/seenv2`),
/// unless blocked by `block_seen`.
pub async fn send_seen_v2(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    last_msg_id: &str,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    if crate::session::privacy_blocker_state().2 {
        return Ok(json!({"error_code": 0, "data": {}}));
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim_start_matches('u')
        .trim();
    if clean.is_empty() || last_msg_id.trim().is_empty() {
        return Ok(json!({"error_code": 0, "data": {}}));
    }
    let (endpoint, path_and_q, body) = if is_group {
        (
            "group",
            format!("/api/group/seenv2?{}", s.common_params()),
            json!({
                "msgInfos": json!({
                    "data": [{ "gmi": last_msg_id, "cmi": last_msg_id }],
                    "grid": clean,
                }).to_string(),
                "imei": s.imei,
            }),
        )
    } else {
        (
            "chat",
            format!("/api/message/seenv2?{}", s.common_params()),
            json!({
                "msgInfos": json!({
                    "data": [{ "gmi": last_msg_id, "cmi": last_msg_id }],
                    "senderId": clean,
                }).to_string(),
                "imei": s.imei,
            }),
        )
    };
    let params = s.encoded_params(&body)?;
    let resp = s
        .post_endpoint_form(endpoint, &path_and_q, format!("params={params}"))
        .await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Mark a conversation as unread or read (`addMarkAsUnread` / `removeMarkAsUnread` @ `compact-app-pc.js:14299958`, cmd `12013` / `12014`).
pub async fn set_unread_mark(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    unread: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim();
    let (convs_group, convs_user) = if is_group {
        (vec![clean], Vec::<&str>::new())
    } else {
        (Vec::<&str>::new(), vec![clean])
    };
    let (path, inner) = if unread {
        (
            "/api/conv/addUnreadMark",
            json!({
                "convsGroup": convs_group,
                "convsUser": convs_user,
                "imei": s.imei
            }),
        )
    } else {
        (
            "/api/conv/removeUnreadMark",
            json!({
                "convsGroup": convs_group,
                "convsUser": convs_user,
                "convsGroupData": [],
                "convsUserData": []
            }),
        )
    };
    let outer = json!({ "param": inner.to_string() });
    let params = s.encoded_params(&outer)?;
    let url = format!("{}{path}?{}", s.domains.conversation, s.common_params());
    let resp = s.post_form(&url, format!("params={params}")).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Mute or unmute notifications for a conversation (`setMuteConversation` @ `compact-app-pc.js:14304409`, cmd `11288`).
/// `duration_sec`: `3600` (1h), `14400` (4h), seconds until 8:00 AM, or `-1` (until reopened).
/// `mute`: `true` -> `action = 1`, `false` -> `action = 2`.
pub async fn set_mute_conversation(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    duration_sec: i64,
    mute: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim();
    let body = json!({
        "toid": clean,
        "duration": duration_sec,
        "action": if mute { 1 } else { 2 },
        "startTime": crate::login::unix_now(),
        "muteType": if is_group { 2 } else { 1 },
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let path = format!("/api/social/profile/setmute?{}", s.common_params());
    let resp = s
        .post_endpoint_form("profile", &path, format!("params={params}"))
        .await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Fetch the full mute snapshot (`chatEntries` + `groupChatEntries`) from Official Zalo PC
/// (`getMuteConversations` @ `default-login-main-startup-shared-worker.js`, cmd `11280`, #67).
pub async fn get_mute_conversations(s: &Session) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = json!({
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let path = format!(
        "/api/social/profile/getmute?{}&params={params}",
        s.common_params()
    );
    let resp = s.get_endpoint("profile", &path).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Real-time mute/unmute control notification (`act_type: "mute"`, `act: "set" | "unset"`, #67).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MuteControlEvent {
    /// Canonical thread ID (`g<id>` for groups, `<id>` for 1-1 peers).
    pub thread_id: String,
    pub is_group: bool,
    pub muted: bool,
    /// Expiry Unix timestamp (`Some(ts)`), or `None` when indefinite (`duration == -1`) or unmuted.
    pub muted_until: Option<i64>,
    /// Value for `AppSettings::muted_threads` (`Some(-1)` for indefinite, `Some(until)` for timed, `None` when unmuted).
    pub settings_until: Option<i64>,
}

/// Parse `act_type: "mute"` items inside WebSocket `cmd 601`/`602`/`603` or `lp_control`
/// payloads (`_processMuteConversations` in Zalo PC `app.asar`, #67).
pub fn extract_mute_controls(data_node: &Value, now_unix: i64) -> Vec<MuteControlEvent> {
    let mut out = Vec::new();
    let mut candidates: Vec<&Value> = Vec::new();
    if let Some(arr) = data_node.get("controls").and_then(Value::as_array) {
        candidates.extend(arr.iter());
    } else if let Some(arr) = data_node.as_array() {
        for item in arr {
            if let Some(sub) = item.get("controls").and_then(Value::as_array) {
                candidates.extend(sub.iter());
            } else {
                candidates.push(item);
            }
        }
    } else {
        candidates.push(data_node);
    }

    for item in candidates {
        let c = item.get("content").unwrap_or(item);
        let act_type = c
            .get("act_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if act_type != "mute" {
            continue;
        }
        let act = c.get("act").and_then(Value::as_str).unwrap_or_default();
        let raw_data = c.get("data").unwrap_or(c);
        let parsed_data: Value;
        let d = if let Some(s) = raw_data.as_str() {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                parsed_data = v;
                &parsed_data
            } else {
                continue;
            }
        } else {
            raw_data
        };

        let raw_id = d
            .get("toId")
            .or_else(|| d.get("toid"))
            .or_else(|| d.get("id"))
            .and_then(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
            })
            .unwrap_or_default();
        let clean = raw_id
            .trim()
            .trim_start_matches("group_")
            .trim_start_matches('g')
            .trim_start_matches('G');
        if clean.is_empty() || clean == "0" {
            continue;
        }

        let mute_type = d
            .get("muteType")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(1);
        let is_group = mute_type == 2 || raw_id.starts_with('g') || raw_id.starts_with("group_");
        let thread_id = if is_group {
            format!("g{clean}")
        } else {
            clean.to_string()
        };

        let duration = d
            .get("duration")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(-1);
        let mute_mode = d
            .get("muteMode")
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(1);
        let start_time = d
            .get("startTime")
            .or_else(|| d.get("systemTime"))
            .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
            .unwrap_or(now_unix);

        let is_set = match act {
            "set" => mute_mode == 1 && duration != 0,
            "unset" => false,
            _ => {
                let action_code = d.get("action").and_then(Value::as_i64).unwrap_or(1);
                action_code == 1 && mute_mode == 1 && duration != 0
            }
        };

        if !is_set {
            out.push(MuteControlEvent {
                thread_id,
                is_group,
                muted: false,
                muted_until: None,
                settings_until: None,
            });
            continue;
        }

        if duration < 0 {
            out.push(MuteControlEvent {
                thread_id,
                is_group,
                muted: true,
                muted_until: None,
                settings_until: Some(-1),
            });
        } else {
            let until = start_time.saturating_add(duration);
            if until > now_unix {
                out.push(MuteControlEvent {
                    thread_id,
                    is_group,
                    muted: true,
                    muted_until: Some(until),
                    settings_until: Some(until),
                });
            } else {
                out.push(MuteControlEvent {
                    thread_id,
                    is_group,
                    muted: false,
                    muted_until: None,
                    settings_until: None,
                });
            }
        }
    }
    out
}

/// Request mobile message sync (`pullMobileMsg` @ `app.asar`, cmd `12000`, `file` endpoint, #68, #73).
/// Sends a wake-up/sync signal with a 2048-bit RSA SPKI DER public key to the user's phone Zalo client.
pub async fn request_phone_sync(s: &Session, public_key: &str) -> Result<Value> {
    let temp_key = crate::history_sync::latest_mobile_temp_key();
    request_phone_sync_with_temp_key(s, public_key, &temp_key).await
}

/// Request mobile message sync (`pullMobileMsg`, cmd `12000`) with an explicit `temp_key`
/// (received from `transfer_after_login` control event or empty for manual request).
pub async fn request_phone_sync_with_temp_key(
    s: &Session,
    public_key: &str,
    temp_key: &str,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = json!({
        "pc_name": s.computer_name,
        "public_key": public_key,
        "from_seq_id": 0,
        "is_retry": 0,
        "min_seq_id": 0,
        "temp_key": temp_key,
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let path = format!(
        "/api/message/pull_mobile_msg?{}&params={params}",
        s.common_params()
    );
    if let Ok(resp) = s.get_endpoint("file", &path).await {
        let dec = s.maybe_decrypt(resp, s.session_key_bytes()).await;
        if dec.get("error_code").and_then(Value::as_i64) == Some(0) {
            return Ok(dec);
        }
    }
    let post_url = format!(
        "{}/api/message/pull_mobile_msg?{}",
        s.domains.file,
        s.common_params()
    );
    let resp = s.post_form(&post_url, format!("params={params}")).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Request transfer-sync v2 from phone (`requestSyncMessage` @ `app.asar`, cmd `12888`, `file` endpoint, #68, #73).
/// Mirrors `requestSyncMessage(reqId, data)` in `main-startup.js:14372356` & `shared-worker.js:282247`
/// plus WebSocket `cmd 592` (`REQUEST_MOBILE_WAKE_UP`) and `cmd 590` (`SYNC_MESSAGE.REQUEST`).
pub async fn request_phone_sync_v2(s: &Session, _public_key: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let sync_id = crate::param_cipher::random_hex(32);
    let temp_key = crate::history_sync::latest_mobile_temp_key();
    let (ik_pub, ek_pub) = match crate::history_sync::generate_sync_v2_keys() {
        Ok(k) => (k.ik_pub, k.ek_pub),
        Err(_) => (
            crate::history_sync::generate_sync_v2_ed25519_public_key(0xA1),
            crate::history_sync::generate_sync_v2_ed25519_public_key(0xB2),
        ),
    };
    // 1. Wake up mobile master device over WebSocket (`cmd 592`)
    let _ = crate::socket::send_mobile_wakeup(&sync_id);

    // 2. Build SyncFull v2 request payload matching `shared-worker.js:282247` & `314736`
    let sync_data = json!({
        "syncId": sync_id,
        "syncType": 0, // No.d.SyncFull = 0
        "ek": ek_pub,
        "ik": ik_pub,
        "toDevice": 0, // No.c.Mobile = 0
        "tempKey": temp_key,
        "deviceName": s.computer_name,
        "req": {
            "type": "message",
            "priority": 1,
            "queries": [
                {
                    "partition": "",
                    "from": 0,
                    "to": 9_007_199_254_740_991_i64,
                    "limit": 2000
                }
            ],
            "batchSize": 2000
        },
        "ver": 1,
        "ussidx": 0
    });
    let sent_ws = crate::socket::send_sync_message_ws_request(&sync_data);
    Ok(json!({
        "error_code": 0,
        "data": {
            "syncId": sync_id,
            "ws_sent": sent_ws,
        }
    }))
}

/// Cancel an active mobile message sync request (`cancelTransferMessage` @ `app.asar:14372356`, cmd `12700`, #68).
pub async fn cancel_phone_sync(s: &Session, public_key: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = json!({
        "pc_name": s.computer_name,
        "public_key": public_key,
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let path = format!(
        "/api/message/cancel_pull_mobile_msg?{}&params={params}",
        s.common_params()
    );
    let resp = s.get_endpoint("file", &path).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}
/// Notify Zalo server and mobile device that sync snapshot download is completed
/// (`cleanSync` @ `app.asar`, cmd `12003`), dismissing the mobile sync spinner.
pub async fn clean_sync(s: &Session, public_key: &str) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let body = json!({
        "public_key": public_key,
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let path = format!(
        "/api/message/delete_snapshot_mobile_msg?{}&params={params}",
        s.common_params()
    );
    let resp = s.get_endpoint("file", &path).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Mobile sync control event received via WebSocket (`cmd 601`/`602`/`603`) or `lp_control`
/// (`act_type: "transfer_msg" | "syncmsgmb" | "transfer_sync2"`, `main-startup.js:647494`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobileSyncControlEvent {
    /// `act == "transfer_after_login"` — carries `temp_key` issued right after QR scan.
    TransferAfterLogin { temp_key: String },
    /// `act == "transfer_status"` (`status == 3`) — phone displayed confirmation prompt and is waiting for user tap!
    WaitingConfirmOnPhone { sync_id: String },
    /// `act == "user_confirm"` (`user_action == 1 | 3`) or `transfer_status` (`status == 4 | 5`) — user tapped "Đồng bộ" on their phone!
    UserConfirmed,
    /// `act == "user_confirm"` (`user_action == 0`) or `transfer_status` (`status == 6 | 7`) — user declined sync on their phone.
    UserRejected,
    /// `act == "user_confirm"` (`user_action == 2`) or `transfer_status` (`status == 8`) — phone is busy backing up/restoring (`BUSY_RESTORING`).
    MobileBusyRestoring,
    /// `act == "syncmsg_info"` or `act == "upload_batch"` — phone finished uploading backup package (`url`, `file_name`, `encrypted_key`).
    BackupReady {
        url: String,
        file_name: String,
        encrypted_key: String,
        checksum: String,
    },
    /// `act == "upload_batch"` (`act_type == "transfer_sync2"`) — Sync v2 chunk (used by iOS).
    SyncV2Batch {
        url: String,
        sync_id: String,
        batch_type: u8,
        is_last: bool,
    },
}

/// Extract `transfer_msg` / `syncmsgmb` / `transfer_sync2` events from a control payload.
pub fn extract_mobile_sync_controls(data_node: &Value) -> Vec<MobileSyncControlEvent> {
    let mut out = Vec::new();
    let mut candidates: Vec<&Value> = Vec::new();
    if let Some(arr) = data_node.get("controls").and_then(Value::as_array) {
        candidates.extend(arr.iter());
    } else if let Some(arr) = data_node.as_array() {
        for item in arr {
            if let Some(sub) = item.get("controls").and_then(Value::as_array) {
                candidates.extend(sub.iter());
            } else {
                candidates.push(item);
            }
        }
    } else {
        candidates.push(data_node);
    }

    for item in candidates {
        let c = item.get("content").unwrap_or(item);
        let act_type = c
            .get("act_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(act_type, "transfer_msg" | "syncmsgmb" | "transfer_sync2") {
            continue;
        }
        let act = c.get("act").and_then(Value::as_str).unwrap_or_default();
        let parsed_data: Value = match c.get("data") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
            Some(v) => v.clone(),
            None => Value::Null,
        };
        match act {
            "transfer_after_login" => {
                if let Some(tk) = parsed_data
                    .get("temp_key")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    out.push(MobileSyncControlEvent::TransferAfterLogin {
                        temp_key: tk.to_string(),
                    });
                }
            }
            "user_confirm" => {
                let ua = parsed_data
                    .get("user_action")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(-1);
                if ua == 1 || ua == 3 {
                    out.push(MobileSyncControlEvent::UserConfirmed);
                } else if ua == 2 {
                    out.push(MobileSyncControlEvent::MobileBusyRestoring);
                } else if ua == 0 {
                    out.push(MobileSyncControlEvent::UserRejected);
                }
            }
            "transfer_status" => {
                let st = parsed_data
                    .get("status")
                    .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))
                    .unwrap_or(-1);
                let sid = parsed_data
                    .get("syncId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                match st {
                    3 => out.push(MobileSyncControlEvent::WaitingConfirmOnPhone { sync_id: sid }),
                    4 | 5 => out.push(MobileSyncControlEvent::UserConfirmed),
                    6 | 7 => out.push(MobileSyncControlEvent::UserRejected),
                    8 => out.push(MobileSyncControlEvent::MobileBusyRestoring),
                    _ => {}
                }
            }
            "upload_batch" => {
                let url = parsed_data
                    .get("msgUrl")
                    .or_else(|| parsed_data.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let sid = parsed_data
                    .get("syncId")
                    .and_then(Value::as_str)
                    .unwrap_or("sync_v2_batch")
                    .to_string();
                let batch_type = parsed_data
                    .get("batchType")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as u8;
                let is_last = parsed_data
                    .get("isLast")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) == 1;
                if !url.is_empty() {
                    out.push(MobileSyncControlEvent::SyncV2Batch {
                        url,
                        sync_id: sid,
                        batch_type,
                        is_last,
                    });
                } else {
                    out.push(MobileSyncControlEvent::MobileBusyRestoring);
                }
            }
            "syncmsg_info" => {
                let url = parsed_data
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let file_name = parsed_data
                    .get("file_name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let encrypted_key = parsed_data
                    .get("encrypted_key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let checksum = parsed_data
                    .get("checksum_code")
                    .or_else(|| parsed_data.get("checksum"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if !url.is_empty() {
                    out.push(MobileSyncControlEvent::BackupReady {
                        url,
                        file_name,
                        encrypted_key,
                        checksum,
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// Hide or unhide a conversation (`updateItemHiddenChat` @ `compact-app-pc.js:14384423`, cmd `10049`).
pub async fn set_hidden_conversation(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    hide: bool,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim();
    let entry = json!([{
        "thread_id": clean,
        "is_group": if is_group { 1 } else { 0 }
    }]);
    let (add_str, del_str) = if hide {
        (entry.to_string(), "[]".to_string())
    } else {
        ("[]".to_string(), entry.to_string())
    };
    let body = json!({
        "del_threads": del_str,
        "add_threads": add_str,
        "imei": s.imei
    });
    let params = s.encoded_params(&body)?;
    let url = format!(
        "{}/api/hiddenconvers/add-remove?{}",
        s.domains.conversation,
        s.common_params()
    );
    let resp = s.post_form(&url, format!("params={params}")).await?;
    Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
}

/// Delete conversation history for me (`deleteOneOneConversationV2` / `deleteGroupConversationV2` @ `compact-app-pc.js:14374881`, cmd `11778` / `11835`).
pub async fn delete_conversation(
    s: &Session,
    thread_id: &str,
    is_group: bool,
    last_cli_msg_id: &str,
) -> Result<Value> {
    if s.login.is_none() {
        anyhow::bail!("chưa đăng nhập");
    }
    let clean = thread_id
        .trim_start_matches("group_")
        .trim_start_matches('g')
        .trim_start_matches('G')
        .trim();
    let cli_id = if last_cli_msg_id.is_empty() {
        format!("{}", crate::login::unix_now() * 1000)
    } else {
        last_cli_msg_id.to_string()
    };
    if is_group {
        let body = json!({
            "grid": clean,
            "cliMsgId": cli_id,
            "conver": {},
            "onlyMe": 1,
            "imei": s.imei
        });
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{}/api/group/deleteconver?{}",
            s.domains.group,
            s.common_params()
        );
        let resp = s.post_form(&url, format!("params={params}")).await?;
        Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
    } else {
        let body = json!({
            "toid": clean,
            "cliMsgId": cli_id,
            "conver": {},
            "onlyMe": 1,
            "imei": s.imei
        });
        let params = s.encoded_params(&body)?;
        let url = format!(
            "{}/api/message/deleteconver?{}",
            s.domains.chat,
            s.common_params()
        );
        let resp = s.post_form(&url, format!("params={params}")).await?;
        Ok(s.maybe_decrypt(resp, s.session_key_bytes()).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_time_when_parsing_millisecond_timestamps() {
        use chrono::TimeZone;
        let expected = chrono::Local
            .timestamp_millis_opt(1793467800000)
            .single()
            .unwrap()
            .format("%H:%M")
            .to_string();
        let thread = parse_thread(&serde_json::json!({
            "userId": "user-1", "lastActionTime": 1793467800000_i64
        }))
        .unwrap();
        assert_eq!(thread.time, expected);
        let message = parse_chat_message(&serde_json::json!({
            "msgId": "msg-0001", "content": "hello", "ts": 1793467800000_i64
        }))
        .unwrap();
        assert_eq!(message.time, expected);
        assert_eq!(message.ts, 1793467800000_i64);
    }

    #[test]
    fn local_time_when_parsing_second_timestamps_and_invalid_values() {
        let expected = crate::clock::message_time(1793467800000);
        for ts in [
            serde_json::json!(1793467800),
            serde_json::json!("1793467800"),
        ] {
            let thread = parse_thread(&serde_json::json!({
                "userId": "user-1", "lastActionTime": ts
            }))
            .unwrap();
            let message = parse_chat_message(&serde_json::json!({
                "msgId": "msg-0001", "content": "hello", "ts": ts
            }))
            .unwrap();
            assert_eq!(thread.time, expected);
            assert_eq!(message.time, expected);
            assert_eq!(message.ts, 1793467800);
        }
        for ts in [
            serde_json::json!(-1),
            serde_json::json!(0),
            serde_json::json!(i64::MAX),
            serde_json::json!("invalid"),
        ] {
            let thread = parse_thread(&serde_json::json!({
                "userId": "user-1", "lastActionTime": ts
            }))
            .unwrap();
            assert_eq!(thread.time, "");
        }
        let message = parse_chat_message(&serde_json::json!({
            "msgId": "msg-0001", "content": "hello", "ts": i64::MAX
        }))
        .unwrap();
        assert_eq!(message.time, "");
    }

    #[tokio::test]
    async fn unauthenticated_calls_fail_cleanly() {
        let s = Session::new(false, false, false).unwrap();
        let err1 = get_friends(&s).await.unwrap_err();
        assert_eq!(err1.to_string(), "chưa đăng nhập");

        let err2 = get_last_msgs(&s, &["test_tid".into()]).await.unwrap_err();
        assert_eq!(err2.to_string(), "chưa đăng nhập");

        let err3 = send_message(&s, "test_to", "hello").await.unwrap_err();
        assert_eq!(err3.to_string(), "chưa đăng nhập");

        let err4 = get_pinned(&s).await.unwrap_err();
        assert_eq!(err4.to_string(), "chưa đăng nhập");

        let err5 = get_group_history(&s, "test_gid", 20).await.unwrap_err();
        assert_eq!(err5.to_string(), "chưa đăng nhập");
    }

    fn sample_thread(id: &str) -> Thread {
        Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: id.into(),
            name: format!("Name {id}"),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
        }
    }

    #[test]
    fn merge_thread_into_matches_exact_client_reference() {
        let m = |id: &str, text: &str, ts: i64, out: bool| ChatMessage {
            id: id.into(),
            client_message_id: if id == "out_9" || id == "77" {
                "client-9".into()
            } else {
                String::new()
            },
            text: text.into(),
            ts,
            time: format!("{}", ts / 60_000),
            is_outgoing: out,
            ..Default::default()
        };
        let mut target = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "t".into(),
            name: "A".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: vec![
                m("1", "chào", 1_000_000, false),
                m("out_9", "ok", 2_000_000, true),
            ],
        };
        let incoming = Thread {
            messages: vec![
                m("1", "chào", 1_000_000, false),   // same id -> no duplicate
                m("77", "ok", 2_002_000, true),     // exact client reference -> takes server ID
                m("78", "ok", 9_000_000_000, true), // same text, other day -> new message
                m("79", "mới", 3_000_000, false),   // new
            ],
            ..target.clone()
        };
        merge_thread_into(&mut target, incoming, false);
        let ids: Vec<&str> = target.messages.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "77", "79", "78"]);
    }

    #[test]
    fn test_merge_previews_single_message_object() {
        let mut threads = vec![sample_thread("u1")];
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u1",
                    "message": {
                        "msgId": "m_single",
                        "content": "Single message text",
                        "ts": 1729501000000_i64,
                        "uidFrom": "u1"
                    }
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].id, "m_single");
        assert_eq!(threads[0].messages[0].text, "Single message text");
        assert_eq!(threads[0].preview, "Single message text");
    }

    #[test]
    fn test_merge_previews_last_msg_object() {
        let mut threads = vec![sample_thread("u2")];
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u2",
                    "lastMsg": {
                        "msgId": "m_last",
                        "content": "LastMsg text",
                        "ts": 1729502000000_i64,
                        "uidFrom": "u2"
                    }
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].id, "m_last");
        assert_eq!(threads[0].preview, "LastMsg text");
    }

    #[test]
    fn test_merge_previews_messages_array_three_items() {
        let mut threads = vec![sample_thread("u3")];
        // Provide items out of order to verify timestamp ascending sort
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u3",
                    "messages": [
                        {
                            "msgId": "m_item1",
                            "content": "First message",
                            "ts": 1000_i64,
                            "uidFrom": "u3"
                        },
                        {
                            "msgId": "m_item3",
                            "content": "Third message",
                            "ts": 3000_i64,
                            "uidFrom": "u3"
                        },
                        {
                            "msgId": "m_item2",
                            "content": "Second message",
                            "ts": 2000_i64,
                            "uidFrom": "0"
                        }
                    ]
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert_eq!(threads[0].messages.len(), 3);
        assert_eq!(threads[0].messages[0].id, "m_item1");
        assert_eq!(threads[0].messages[0].text, "First message");
        assert_eq!(threads[0].messages[1].id, "m_item2");
        assert_eq!(threads[0].messages[1].text, "Second message");
        assert!(threads[0].messages[1].is_outgoing);
        assert_eq!(threads[0].messages[2].id, "m_item3");
        assert_eq!(threads[0].messages[2].text, "Third message");
        // Preview must be from the newest message (m_item3)
        assert_eq!(threads[0].preview, "Third message");
    }

    #[test]
    fn test_merge_previews_entry_with_no_message() {
        let mut threads = vec![sample_thread("u4")];
        threads[0].preview = "Existing preview".into();
        let resp = json!({
            "error_code": 0,
            "data": [
                {
                    "idTo": "u4"
                },
                {
                    "idTo": "u4",
                    "messages": []
                }
            ]
        });

        merge_previews(&mut threads, &resp);
        assert!(threads[0].messages.is_empty());
        assert_eq!(threads[0].preview, "Existing preview");
    }

    #[test]
    fn test_merge_group_history_fixture() {
        let mut thread = sample_thread("grp_123");
        thread.is_group = true;

        let resp = json!({
            "error_code": 0,
            "data": {
                "groupMsgs": [
                    {
                        "msgId": "gm_2",
                        "content": "Second group message",
                        "ts": 2000_i64,
                        "uidFrom": "member_2"
                    },
                    {
                        "msgId": "gm_1",
                        "content": "First group message",
                        "ts": 1000_i64,
                        "uidFrom": "member_1"
                    },
                    {
                        "msgId": "gm_2",
                        "content": "Duplicate second message",
                        "ts": 2000_i64,
                        "uidFrom": "member_2"
                    }
                ]
            }
        });

        merge_group_history(&mut thread, &resp);
        assert_eq!(thread.messages.len(), 2);
        assert_eq!(thread.messages[0].id, "gm_1");
        assert_eq!(thread.messages[1].id, "gm_2");
        assert_eq!(thread.preview, "Second group message");
    }

    #[test]
    fn test_merge_previews_incoming_1_1_and_group_and_cloud() {
        let mut cloud = sample_thread("9000000000000000001");
        cloud.name = "Cloud của tôi (Lưu trữ)".into();
        let friend = sample_thread("1000000000000000001");
        let mut group = sample_thread("888999000");
        group.is_group = true;
        let mut threads = vec![cloud, friend, group];

        let resp = json!({
            "error_code": 0,
            "data": {
                "msgs": [
                    {
                        "msgId": "in_1",
                        "content": "Tin nhan den tu ban be",
                        "idTo": "0",
                        "uidFrom": "1000000000000000001",
                        "ts": "1729505000000"
                    },
                    {
                        "msgId": "cloud_1",
                        "content": "Tin nhan tu gui vao Cloud",
                        "idTo": "0",
                        "uidFrom": "0",
                        "ts": "1729506000000"
                    }
                ],
                "groupMsgs": [
                    {
                        "msgId": "grp_msg_1",
                        "content": "Tin nhan trong nhom",
                        "idTo": "888999000",
                        "uidFrom": "111222333",
                        "dName": "Nguyen Van B",
                        "ts": "1729507000000"
                    }
                ]
            }
        });

        merge_previews(&mut threads, &resp);
        // Cloud message routed to `9000000000000000001`
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].messages[0].text, "Tin nhan tu gui vao Cloud");
        assert!(threads[0].messages[0].is_outgoing);

        // Incoming 1-1 routed to `1000000000000000001`
        assert_eq!(threads[1].messages.len(), 1);
        assert_eq!(threads[1].messages[0].text, "Tin nhan den tu ban be");
        assert!(!threads[1].messages[0].is_outgoing);

        // Group message routed to `888999000` (not `111222333`)
        assert_eq!(threads[2].messages.len(), 1);
        assert_eq!(threads[2].messages[0].text, "Tin nhan trong nhom");
        assert_eq!(threads[2].messages[0].sender, "Nguyen Van B");
        assert!(!threads[2].messages[0].is_outgoing);
    }

    #[test]
    fn test_poll_state_encode_and_update() {
        let mut ps = PollState::default();
        let encoded = ps.encode_params();
        assert!(encoded.starts_with("1,1,"));
        assert!(encoded.ends_with("%2C1"));

        let resp = json!({
            "data": {
                "lastActionId": "8399086511016",
                "lastActionIdOther": "8399036593265",
                "queueStatus": {
                    "1": {
                        "lastId": "8399086511016",
                        "ids": ["8399086511016"]
                    },
                    "15": {
                        "lastId": "8399036593265",
                        "ids": ["8399036593265"]
                    }
                }
            }
        });
        ps.update_from_response(&resp);
        assert_eq!(ps.last_action_id, "8399086511016");
        assert_eq!(ps.last_action_id_other, "8399036593265");
        assert_eq!(ps.queues.get("1").unwrap().last_id, "8399086511016");
        assert_eq!(ps.queues.get("1").unwrap().ids, vec!["8399086511016".to_string()]);
        // A message queue the server reports ("15" = 510_1) is tracked from the start of what
        // the server kept (real account 02/11: lp_chat accepted 15/16; without them the
        // long-poll repeated the same page).
        assert_eq!(ps.queues.get("15").unwrap().last_id, "1");
    }

    #[test]
    fn test_parse_chat_message_sticker_json_string() {
        let msg_val = json!({
            "msgId": "stk_1",
            "msgType": "chat.sticker",
            "content": "{\"catId\":1002,\"id\":2005,\"type\":1}",
            "uidFrom": "user_a",
            "ts": 1729508000000_i64
        });
        let parsed = parse_chat_message(&msg_val).expect("must parse sticker");
        assert_eq!(parsed.text, "[Nhãn dán]");
        assert_eq!(parsed.id, "stk_1");
        assert_eq!(parsed.sticker_id, Some(2005));
        assert_eq!(parsed.sticker_cate_id, Some(1002));
        assert_eq!(parsed.sticker_type, Some(1));
        assert_eq!(
            parsed.sticker_image_url().as_deref(),
            Some("https://api.zaloapp.com/api/emoticon/sticker/webpc?eid=2005&size=130")
        );
    }

    #[test]
    fn test_parse_chat_message_integer_msg_types() {
        // Integer msgType = 5 (sticker) with empty content
        let stk = json!({
            "msgId": "stk_int",
            "msgType": 5,
            "content": "",
            "uidFrom": "user_b",
            "ts": 1000_i64
        });
        assert_eq!(parse_chat_message(&stk).unwrap().text, "[Nhãn dán]");

        // Integer msgType = 2 (photo)
        let photo = json!({
            "msgId": "photo_int",
            "msgType": 2,
            "content": "",
            "uidFrom": "user_b",
            "ts": 2000_i64
        });
        assert_eq!(parse_chat_message(&photo).unwrap().text, "[Hình ảnh]");

        // Integer msgType = 3 (voice)
        let voice = json!({
            "msgId": "voice_int",
            "msgType": 3,
            "content": "",
            "uidFrom": "user_b",
            "ts": 3000_i64
        });
        assert_eq!(parse_chat_message(&voice).unwrap().text, "[Tin nhắn thoại]");

        // Integer msgType = 19 (file)
        let file = json!({
            "msgId": "file_int",
            "msgType": 19,
            "content": "",
            "uidFrom": "user_b",
            "ts": 4000_i64
        });
        assert_eq!(parse_chat_message(&file).unwrap().text, "[Tập tin]");

        // Integer msgType = 31 (undo)
        let undo = json!({
            "msgId": "undo_int",
            "msgType": 31,
            "content": "",
            "uidFrom": "user_b",
            "ts": 5000_i64
        });
        assert_eq!(
            parse_chat_message(&undo).unwrap().text,
            "[Tin nhắn đã thu hồi]"
        );
    }

    #[test]
    fn test_parse_chat_message_with_plain_message_string() {
        let msg = json!({
            "msgId": "msg_str",
            "message": "Xin chao buoi sang",
            "uidFrom": "user_c",
            "dName": "Charlie",
            "ts": 1000_i64
        });
        let parsed = parse_chat_message(&msg).unwrap();
        assert_eq!(parsed.text, "Xin chao buoi sang");
        assert_eq!(parsed.sender, "Charlie");
    }

    /// Synthetic queue message shaped like `510_1` / `501` entries (no real data).
    fn queue_msg(msg_type: &str, content: Value) -> Value {
        json!({
            "msgId": "m_1",
            "msgType": msg_type,
            "uidFrom": "user_x",
            "ts": "1700000000000",
            "content": content
        })
    }

    #[test]
    fn queue_photo_gets_photo_card_without_url_text() {
        let content = json!({
            "action": "", "childnumber": 0, "description": "", "title": "", "type": "",
            "href": "https://photo.example.zdn.vn/a/full.jpg",
            "thumb": "https://photo.example.zdn.vn/a/thumb.jpg",
            "params": "{\"width\":1280,\"height\":960,\"hd\":\"https://photo.example.zdn.vn/a/hd.jpg\"}"
        });
        let m = parse_chat_message(&queue_msg("chat.photo", content)).unwrap();
        assert_eq!(m.text, "[Hình ảnh]");
        assert_eq!(m.link_src.as_deref(), Some("Hình ảnh Zalo"));
        assert_eq!(m.link_url.as_deref(), Some("https://photo.example.zdn.vn/a/hd.jpg"));
        assert_eq!(m.link_thumb.as_deref(), Some("https://photo.example.zdn.vn/a/thumb.jpg"));
        assert_eq!(m.link_desc, None);
    }

    #[test]
    fn photo_caption_is_kept_but_urls_never_become_text() {
        // Caption in `description`; `hd` over plain http is ignored in favour of `href`.
        let with_caption = json!({
            "description": "Ảnh đi chơi", "title": "",
            "href": "https://p.zdn.vn/full.jpg", "thumb": "https://p.zdn.vn/t.jpg",
            "params": "{\"hd\":\"http://p.zdn.vn/hd.jpg\"}"
        });
        let m = parse_chat_message(&queue_msg("chat.photo", with_caption)).unwrap();
        assert_eq!(m.text, "[Hình ảnh] Ảnh đi chơi");
        assert_eq!(m.link_desc.as_deref(), Some("Ảnh đi chơi"));
        assert_eq!(m.link_url.as_deref(), Some("https://p.zdn.vn/full.jpg"));

        // A URL in title/description is not a caption; a JSON-string content works too.
        let url_title = json!({
            "description": "https://p.zdn.vn/full.jpg", "title": "https://p.zdn.vn/x",
            "href": "https://p.zdn.vn/full.jpg"
        })
        .to_string();
        let m = parse_chat_message(&queue_msg("chat.photo", Value::String(url_title))).unwrap();
        assert_eq!(m.text, "[Hình ảnh]");
        assert_eq!(m.link_desc, None);
        assert_eq!(m.link_thumb.as_deref(), Some("https://p.zdn.vn/full.jpg"));

        // Non-web URLs are dropped.
        let bad = json!({ "href": "file:///C:/x.jpg", "thumb": "javascript:alert(1)" });
        let m = parse_chat_message(&queue_msg("chat.photo", bad)).unwrap();
        assert_eq!((m.link_url, m.link_thumb), (None, None));
        assert_eq!(m.text, "[Hình ảnh]");
    }

    #[test]
    fn queue_video_msg_gets_video_card_with_duration() {
        let content = json!({
            "title": "", "description": "",
            "href": "https://video.example.zdn.vn/v/clip.mp4",
            "thumb": "https://video.example.zdn.vn/v/clip.jpg",
            "params": "{\"duration\":42500,\"video_width\":720,\"video_height\":1280,\"fileSize\":\"9030000\"}"
        });
        let m = parse_chat_message(&queue_msg("chat.video.msg", content)).unwrap();
        assert_eq!(m.text, "[Video]");
        assert_eq!(m.link_src.as_deref(), Some("Video Zalo"));
        assert_eq!(m.link_url.as_deref(), Some("https://video.example.zdn.vn/v/clip.mp4"));
        assert_eq!(m.link_thumb.as_deref(), Some("https://video.example.zdn.vn/v/clip.jpg"));
        assert_eq!(m.link_desc.as_deref(), Some("0:42 · 8,61 MB"));

        // Numeric history type 4 without content still yields the label.
        let bare = json!({ "msgId": "v2", "msgType": 4, "content": "", "uidFrom": "u", "ts": 1 });
        assert_eq!(parse_chat_message(&bare).unwrap().text, "[Video]");
    }

    #[test]
    fn share_file_gets_file_card_with_name_and_size() {
        let content = json!({
            "title": "Bao cao quy 3.pdf",
            "href": "https://dl.example.zdn.vn/f/abc",
            "params": "{\"checksum\":\"00\",\"checksumSha\":\"00\",\"fType\":1,\"fdata\":\"{}\",\"fileExt\":\"pdf\",\"fileSize\":\"25270000\"}"
        });
        let m = parse_chat_message(&queue_msg("share.file", content)).unwrap();
        assert_eq!(m.text, "[Tập tin] Bao cao quy 3.pdf");
        assert_eq!(m.link_src.as_deref(), Some("Tập tin Zalo"));
        assert_eq!(m.link_title.as_deref(), Some("Bao cao quy 3.pdf"));
        assert_eq!(m.link_desc.as_deref(), Some("24,1 MB"));
        assert_eq!(m.link_url.as_deref(), Some("https://dl.example.zdn.vn/f/abc"));
        assert_eq!(m.link_thumb, None);

        // No name: label only, never the URL.
        let nameless = json!({ "title": "", "href": "https://dl.example.zdn.vn/f/abc" });
        let m = parse_chat_message(&queue_msg("chat.file", nameless)).unwrap();
        assert_eq!(m.text, "[Tập tin]");
    }

    #[test]
    fn voice_and_links_and_text_keep_their_shape() {
        let voice = json!({ "href": "https://voice.example.zdn.vn/a.m4a", "params": "{\"duration\":7000}" });
        let m = parse_chat_message(&queue_msg("chat.voice", voice)).unwrap();
        assert_eq!(m.text, "[Tin nhắn thoại]");
        assert_eq!(m.link_url.as_deref(), Some("https://voice.example.zdn.vn/a.m4a"));
        assert_eq!(m.link_desc.as_deref(), Some("0:07"));
        assert_eq!(m.link_src, None);

        // Shared links stay ordinary link cards.
        let link = json!({
            "action": "recommened.link", "title": "Tin tức", "description": "",
            "href": "https://news.example.com/a", "thumb": "https://news.example.com/t.jpg",
            "params": "{\"src\":\"news.example.com\"}"
        });
        let m = parse_chat_message(&queue_msg("chat.recommended", link)).unwrap();
        assert_eq!(m.text, "Tin tức");
        assert_eq!(m.link_url.as_deref(), Some("https://news.example.com/a"));
        assert_eq!(m.link_src.as_deref(), Some("news.example.com"));

        let text = parse_chat_message(&queue_msg("webchat", json!("xin chào"))).unwrap();
        assert_eq!(text.text, "xin chào");
        assert_eq!(text.link_url, None);
    }

    #[test]
    fn legacy_media_url_classifier_is_conservative() {
        use MediaKind::*;
        let k = media_kind_from_legacy_url;
        assert_eq!(k("https://f21-zpc.zdn.vn/123/abc.jpg"), Some(Photo));
        assert_eq!(k("https://s120.zadn.vn/x/y.JPEG?w=1"), Some(Photo));
        assert_eq!(k("https://photo-stal-1.zdn.vn/a/b"), Some(Photo));
        assert_eq!(k("https://f9-zpc.zdn.vn/v/clip.mp4"), Some(Video));
        assert_eq!(k("http://video-hcm.zdn.vn/abc"), Some(Video));
        // Not Zalo's CDN, not media, or suspicious.
        assert_eq!(k("https://example.com/photo.jpg"), None);
        assert_eq!(k("https://zdn.vn.example.com/a.jpg"), None);
        assert_eq!(k("https://evilzdn.vn/a.jpg"), None);
        assert_eq!(k("https://dl.zdn.vn/f/report.pdf"), None);
        assert_eq!(k("https://user@photo.zdn.vn/a.jpg"), None);
        assert_eq!(k("https://f.zdn.vn/a.jpg?x=.mp4"), Some(Photo));
        assert_eq!(k("https://f.zdn.vn/a.webp"), None);
        assert_eq!(k("ftp://photo.zdn.vn/a.jpg"), None);
        assert_eq!(k("xem https://photo.zdn.vn/a.jpg"), None);
        // CDN families seen on the real account (02/11).
        assert_eq!(k("https://video-stal-44.dlmd.me/no/15321b8f/2aObo"), Some(Video));
        assert_eq!(k("https://file-stal-3.dlfl.vn/f/abc/def"), Some(File));
        assert_eq!(k("https://ot46.dlfl.vn/x"), Some(File));
        assert_eq!(k("https://voice-aac-dl.zdn.vn/a/b"), Some(Voice));
        assert_eq!(k("https://cdn.dlmd.me/no/abc"), None, "dlmd.me without a video label");
        assert_eq!(k("https://dlmd.me.example.com/video"), None);
        assert_eq!(k("https://evildlfl.vn/f"), None);

        let row = |text: &str, url: Option<&str>| ChatMessage {
            text: text.into(),
            link_url: url.map(Into::into),
            link_src: Some("f21-zpc.zdn.vn".into()),
            ..Default::default()
        };
        let u = "https://f21-zpc.zdn.vn/123/abc.jpg";
        assert_eq!(legacy_media_kind(&row(u, Some(u))), Some(Photo));
        // Text that is more than the URL, no link_url, or an already-marked row: unchanged.
        assert_eq!(legacy_media_kind(&row(&format!("xem {u}"), Some(u))), None);
        assert_eq!(legacy_media_kind(&row(u, None)), None);
        let mut marked = row(u, Some(u));
        marked.link_src = Some("Hình ảnh Zalo".into());
        assert_eq!(legacy_media_kind(&marked), None);
        assert_eq!(
            legacy_media_kind(&row("https://news.example.com/a", Some("https://news.example.com/a"))),
            None
        );
        // Old file rows kept the file name as text.
        let f = "https://file-stal-3.dlfl.vn/f/abc/def";
        assert_eq!(legacy_media_kind(&row("bao-cao.xlsx", Some(f))), Some(File));
        assert_eq!(legacy_media_kind(&row("[Tập tin] bao-cao.xlsx", Some(f))), None);
        // A video row still needs text == URL.
        let v = "https://video-stal-44.dlmd.me/no/15321b8f/2aObo";
        assert_eq!(legacy_media_kind(&row(v, Some(v))), Some(Video));
        assert_eq!(legacy_media_kind(&row("xem clip", Some(v))), None);
    }

    #[test]
    fn test_parse_thread_boolean_is_group_and_string_time() {
        let val = json!({
            "userId": "grp_test",
            "displayName": "Nhom Chem Gio",
            "isGroup": true,
            "lastActionTime": "1729505000000"
        });
        let thread = parse_thread(&val).unwrap();
        assert!(thread.is_group);
        assert_eq!(thread.name, "Nhom Chem Gio");
        assert!(!thread.time.is_empty());
    }

    #[test]
    fn test_deduplicate_threads_migrates_session_scoped_ids() {
        let old_dtc = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000005".into(),
            name: "Đtc".into(),
            preview: "Hi".into(),
            time: "10:21".into(),
            avatar: Some("https://s120-26-ava-talk.zadn.vn/5/00000000000000000000000000000001.jpg?key=abc&time=1797763127".into()),
            is_group: false,
            messages: vec![ChatMessage {
                id: "m1".into(),
                sender: "Đtc".into(),
                text: "Hi".into(),
                time: "10:21".into(),
                is_outgoing: false,
                ts: 1792578088952,
                ..Default::default()
            }],
        };
        let new_dtc = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000006".into(),
            name: "Đtc".into(),
            preview: "".into(),
            time: "16:48".into(),
            avatar: Some("https://s160-26-ava-talk.zadn.vn/5/00000000000000000000000000000001.jpg?key=xyz&time=1797876325".into()),
            is_group: false,
            messages: Vec::new(),
        };
        let mut threads = vec![old_dtc, new_dtc];
        deduplicate_threads(&mut threads, None);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "1000000000000000006");
        assert_eq!(threads[0].messages.len(), 1);
        assert_eq!(threads[0].preview, "Hi");
    }

    #[test]
    fn test_merge_group_history_parses_stringified_cloud_data() {
        let mut cloud_thread = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "9000000000000000002".into(),
            name: "Cloud của tôi (Lưu trữ)".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
        };
        let raw_cloud_resp = json!({
            "error_code": 0,
            "data": r#"{"error":0,"groupMsgs":[{"msgId":"1000000000001","cliMsgId":"1700000000001","msgType":"chat.recommended","uidFrom":"0","idTo":"9000000000000000002","dName":"User Test","ts":"1700000000001","content":{"title":"","description":"Ghi chú đám mây","href":"https://zalo.me/test","action":"recommened.link"}}]}"#
        });
        merge_group_history(&mut cloud_thread, &raw_cloud_resp);
        assert_eq!(cloud_thread.messages.len(), 1);
        assert!(cloud_thread.messages[0].text.contains("Ghi chú đám mây"));
        assert_eq!(
            cloud_thread.messages[0].link_url.as_deref(),
            Some("https://zalo.me/test")
        );
    }

    #[test]
    fn test_strip_vn_for_sticker_kw() {
        assert_eq!(strip_vn_for_sticker_kw("Xin chào"), "xin chao");
        assert_eq!(strip_vn_for_sticker_kw("Cảm ơn nhiều"), "cam on nhieu");
        assert_eq!(strip_vn_for_sticker_kw("Ngủ ngon nhé"), "ngu ngon nhe");
        assert_eq!(strip_vn_for_sticker_kw("Nguyễn Văn An"), "nguyen van an");
        assert_eq!(
            normalize_sticker_keyword("  iu\u{a0}that\u{a0}  "),
            "iu that"
        );

        let q1 = canonical_vn_sticker_queries("Mệt quá");
        assert!(q1.contains(&"mệt quá".to_string()));
        assert!(q1.contains(&"met qua".to_string()));

        let q2 = canonical_vn_sticker_queries("khoc");
        assert!(q2.contains(&"khoc".to_string()));
        assert!(q2.contains(&"khóc".to_string()));
    }

    #[test]
    fn test_parse_friendship_status_response() {
        let r_friend = json!({"error_code": 0, "data": {"is_friend": 1, "is_requested": 0, "is_requesting": 0}});
        assert_eq!(
            parse_friendship_status_response(&r_friend),
            FriendshipStatus::Friend
        );

        let r_inc = json!({"error_code": 0, "data": {"is_friend": 0, "is_requested": 0, "is_requesting": 1}});
        assert_eq!(
            parse_friendship_status_response(&r_inc),
            FriendshipStatus::IncomingPending
        );

        let r_out = json!({"error_code": 0, "data": {"is_friend": 0, "is_requested": 1, "is_requesting": 0}});
        assert_eq!(
            parse_friendship_status_response(&r_out),
            FriendshipStatus::OutgoingPending
        );

        let r_stranger = json!({"error_code": 0, "data": {"is_friend": 0, "is_requested": 0, "is_requesting": 0}});
        assert_eq!(
            parse_friendship_status_response(&r_stranger),
            FriendshipStatus::Stranger
        );
    }

    #[test]
    fn test_prevent_cloud_message_pollution_across_threads() {
        let mut contact = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000009".into(),
            name: "Người dùng Test".into(),
            preview: String::new(),
            time: String::new(),
            avatar: None,
            is_group: false,
            messages: Vec::new(),
        };
        let raw_cloud_resp = json!({
            "error_code": 0,
            "data": {
                "groupMsgs": [{
                    "msgId": "1000000000002",
                    "msgType": "webchat",
                    "uidFrom": "0",
                    "idTo": "9000000000000000001",
                    "dName": "User Test",
                    "ts": "1700000000002",
                    "content": "Ghi chú Cloud của tôi"
                }]
            }
        });
        merge_group_history(&mut contact, &raw_cloud_resp);
        assert!(contact.messages.is_empty());

        let cloud = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "9000000000000000001".into(),
            name: "Cloud của tôi (Lưu trữ)".into(),
            preview: "Ghi chú Cloud của tôi".into(),
            time: "12:00".into(),
            avatar: None,
            is_group: false,
            messages: vec![ChatMessage {
                id: "1000000000002".into(),
                sender: "Tôi".into(),
                text: "Ghi chú Cloud của tôi".into(),
                time: "12:00".into(),
                is_outgoing: true,
                ts: 1700000000002,
                ..Default::default()
            }],
        };
        let polluted_contact = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000009".into(),
            name: "Người dùng Test".into(),
            preview: "Ghi chú Cloud của tôi".into(),
            time: "12:00".into(),
            avatar: None,
            is_group: false,
            messages: vec![ChatMessage {
                id: "1000000000002".into(),
                sender: "Tôi".into(),
                text: "Ghi chú Cloud của tôi".into(),
                time: "12:00".into(),
                is_outgoing: true,
                ts: 1700000000002,
                ..Default::default()
            }],
        };
        let mut list = vec![cloud, polluted_contact];
        deduplicate_threads(&mut list, None);
        assert_eq!(list[0].messages.len(), 1);
        assert!(list[1].messages.is_empty());
        assert!(list[1].preview.is_empty());
    }

    #[test]
    fn test_duplicate_display_names_with_different_ids_stay_distinct() {
        let contact_a = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000111".into(),
            name: "Nguyễn Văn A".into(),
            preview: "Xin chào từ người 1".into(),
            time: "09:00".into(),
            avatar: Some(
                "https://s120-ava-talk.zadn.vn/1/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jpg".into(),
            ),
            is_group: false,
            messages: vec![ChatMessage {
                id: "m_a".into(),
                sender: "Nguyễn Văn A".into(),
                text: "Xin chào từ người 1".into(),
                time: "09:00".into(),
                is_outgoing: false,
                ts: 1700000001000,
                ..Default::default()
            }],
        };
        // Second contact has the exact same Zalo display name and no avatar URL yet (e.g., from incoming poll)
        let contact_b = Thread {
            presentation: Default::default(),
            unread: Default::default(),
            id: "1000000000000000222".into(),
            name: "Nguyễn Văn A".into(),
            preview: "Tin nhắn mới từ người 2".into(),
            time: "10:30".into(),
            avatar: None,
            is_group: false,
            messages: vec![ChatMessage {
                id: "m_b".into(),
                sender: "Nguyễn Văn A".into(),
                text: "Tin nhắn mới từ người 2".into(),
                time: "10:30".into(),
                is_outgoing: false,
                ts: 1700000002000,
                ..Default::default()
            }],
        };
        assert!(!threads_represent_same_contact(&contact_a, &contact_b));
        let mut threads = vec![contact_a, contact_b];
        deduplicate_threads(&mut threads, None);
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id, "1000000000000000111");
        assert_eq!(threads[1].id, "1000000000000000222");
        assert_eq!(threads[0].messages[0].text, "Xin chào từ người 1");
        assert_eq!(threads[1].messages[0].text, "Tin nhắn mới từ người 2");
    }
}

#[cfg(test)]
mod message_state_defaults_tests {
    use super::*;

    #[test]
    fn legacy_message_when_metadata_is_absent_uses_conservative_defaults() {
        let message: ChatMessage = serde_json::from_value(json!({
            "id": "msg-0001", "sender": "user-1", "text": "Hello",
            "time": "", "is_outgoing": false, "ts": 1
        }))
        .unwrap();
        assert_eq!(message.kind, crate::message_model::MessageKind::Unknown);
        assert_eq!(
            message.delivery,
            crate::message_model::DeliveryState::default()
        );
        assert!(message.wire_ref.fields.is_empty());
        assert!(message.reaction_summaries.is_empty());
        assert!(message.resolved_mention_uids.is_empty());
    }

    #[test]
    fn legacy_thread_when_sidebar_and_unread_are_absent_roundtrips() {
        let thread: Thread = serde_json::from_value(json!({
            "id": "thread-1", "name": "Example", "preview": "", "time": "",
            "avatar": null, "is_group": false, "messages": []
        }))
        .unwrap();
        assert_eq!(thread.presentation, SidebarPresentation::default());
        assert_eq!(thread.unread, crate::unread::UnreadState::default());
        let restored: Thread =
            serde_json::from_value(serde_json::to_value(&thread).unwrap()).unwrap();
        assert_eq!(restored.presentation, thread.presentation);
        assert_eq!(restored.unread, thread.unread);
    }

    // -- Đợt 2: lỗi gửi/nhận không được "làm phẳng" thành thành công --------------

    #[test]
    fn reply_error_reports_only_a_non_zero_verdict() {
        assert_eq!(reply_error(&json!({ "error_code": 0, "data": {} })), None);
        // No `error_code` at all: the envelope carries no verdict, so there is nothing to reject.
        assert_eq!(reply_error(&json!({ "data": {} })), None);
        assert_eq!(
            reply_error(&json!({ "error_code": 245, "error_message": "  Quá nhanh  " })),
            Some((245, "Quá nhanh".to_string()))
        );
        // The message text is optional; the code is not.
        assert_eq!(
            reply_error(&json!({ "error_code": -102 })),
            Some((-102, String::new()))
        );
        // String-encoded codes are not what Zalo sends here; do not invent a verdict.
        assert_eq!(reply_error(&json!({ "error_code": "245" })), None);
    }

    #[test]
    fn failed_action_message_is_vietnamese_and_keeps_the_code() {
        assert_eq!(
            failed("Thả cảm xúc", (245, "Quá nhanh".into())).to_string(),
            "Thả cảm xúc thất bại (mã 245: Quá nhanh)"
        );
        assert_eq!(
            failed("Gửi tệp", (5, String::new())).to_string(),
            "Gửi tệp thất bại (mã 5)"
        );
    }

    #[test]
    fn quote_target_accepts_only_server_message_ids() {
        let msg = |id: &str| ChatMessage {
            id: id.into(),
            cli_msg_id: Some("1793705979715".into()),
            ts: 1793705979842,
            ..Default::default()
        };
        assert_eq!(
            quote_target_ids(&msg("8447783931887")).unwrap(),
            (8447783931887u64, 1793705979715u64, 1793705979842u64)
        );
        // A message that has not reached the server has no id to quote: refuse, never invent.
        for id in ["", "out_1793705979842", "local_3", "khong-phai-so"] {
            let err = quote_target_ids(&msg(id)).unwrap_err().to_string();
            assert!(err.starts_with("Không trích dẫn được tin này"), "{id}: {err}");
        }
    }

    #[test]
    fn quote_target_falls_back_to_send_time_and_refuses_without_one() {
        let mut m = ChatMessage {
            id: "8447783931887".into(),
            ts: 1793705979842,
            ..Default::default()
        };
        // No `cliMsgId` captured: Zalo's own fallback is the quoted message's send time.
        assert_eq!(
            quote_target_ids(&m).unwrap(),
            (8447783931887u64, 1793705979842u64, 1793705979842u64)
        );
        // Neither id nor time: the quote would point at nothing.
        m.ts = 0;
        let err = quote_target_ids(&m).unwrap_err().to_string();
        assert!(err.contains("thiếu thời điểm gửi"), "{err}");
    }

    #[test]
    fn poll_timeout_is_not_an_empty_page() {
        let timeout = poll_timeout_marker();
        assert!(is_poll_timeout(&timeout));
        assert_eq!(timeout["error_code"], json!(0), "không phải lỗi mạng");
        assert_eq!(lp_reply_message_count(&timeout), 0);
        assert!(!lp_reply_wants_more(&timeout));
        // The cursor did not move, so the round must stop instead of re-polling the same range.
        let before = PollState::default();
        let after = before.clone();
        assert!(!lp_should_continue(&before, &after, &timeout));

        // An empty page the server actually produced is not the marker.
        let empty_page = json!({ "error_code": 0, "data": {} });
        assert!(!is_poll_timeout(&empty_page));
        assert_ne!(empty_page.get(LP_TIMEOUT_KEY), timeout.get(LP_TIMEOUT_KEY));
    }

    // -- Đợt 2 (tiếp): mọi hành động ghi đều kiểm `error_code` ---------------------

    #[test]
    fn ensure_action_ok_rejects_a_failed_write() {
        // Explicit success and a reply without a verdict both pass.
        assert!(ensure_action_ok("Xoá tin nhắn", "test::scope", &json!({ "error_code": 0 })).is_ok());
        assert!(ensure_action_ok("Xoá tin nhắn", "test::scope", &json!({ "data": {} })).is_ok());

        let err = ensure_action_ok(
            "Xoá tin nhắn",
            "test::scope",
            &json!({ "error_code": 144, "error_message": "Không tìm thấy tin nhắn" }),
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            err,
            "Xoá tin nhắn thất bại (mã 144: Không tìm thấy tin nhắn)"
        );

        // Recall / friend-request actions use the same helper and keep their own wording.
        for (action, code, expected) in [
            ("Thu hồi tin nhắn", 5, "Thu hồi tin nhắn thất bại (mã 5)"),
            (
                "Huỷ lời mời kết bạn",
                216,
                "Huỷ lời mời kết bạn thất bại (mã 216)",
            ),
            (
                "Từ chối lời mời kết bạn",
                -102,
                "Từ chối lời mời kết bạn thất bại (mã -102)",
            ),
        ] {
            let err = ensure_action_ok(action, "test::scope", &json!({ "error_code": code }))
                .unwrap_err()
                .to_string();
            assert_eq!(err, expected);
        }
    }

    // -- Đợt 3: an toàn hiển thị E2EE ([I], chưa có tin mã hoá thật để kiểm chứng) ------

    #[test]
    fn e2ee_flag_only_from_explicit_fields() {
        // Explicit evidence: non-zero number, numeric string, boolean true.
        assert!(e2ee_placeholder_needed(&json!({ "e2eeStatus": 1 })));
        assert!(e2ee_placeholder_needed(&json!({ "e2eeStatus": 7 })));
        assert!(e2ee_placeholder_needed(&json!({ "e2eeStatus": "2" })));
        assert!(e2ee_placeholder_needed(&json!({ "e2eeStatus": " 1 " })));
        assert!(e2ee_placeholder_needed(&json!({ "e2ee": true })));
        assert!(e2ee_placeholder_needed(&json!({ "e2ee": 1 })));

        // No evidence: zero, empty, unparsable, null, missing.
        assert!(!e2ee_placeholder_needed(&json!({ "e2eeStatus": 0 })));
        assert!(!e2ee_placeholder_needed(&json!({ "e2eeStatus": "0" })));
        assert!(!e2ee_placeholder_needed(&json!({ "e2eeStatus": "" })));
        assert!(!e2ee_placeholder_needed(
            &json!({ "e2eeStatus": "khong-phai-so" })
        ));
        assert!(!e2ee_placeholder_needed(&json!({ "e2eeStatus": null })));
        assert!(!e2ee_placeholder_needed(&json!({ "e2ee": false })));
        assert!(!e2ee_placeholder_needed(&json!({ "e2ee": 0 })));
        assert!(!e2ee_placeholder_needed(&json!({ "e2ee": "" })));
        assert!(!e2ee_placeholder_needed(&json!({ "msgId": "1" })));

        // Anti-regression: base64-looking content and an unknown `msgType` are NOT evidence.
        assert!(!e2ee_placeholder_needed(
            &json!({ "content": "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=" })
        ));
        assert!(!e2ee_placeholder_needed(&json!({
            "msgType": "chat.encrypt",
            "content": "aGVsbG8gd29ybGQ=",
            "msgId": "8447783931887"
        })));
    }

    #[test]
    fn e2ee_message_becomes_a_placeholder_bubble() {
        const CIPHERTEXT: &str = "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=";
        let v = json!({
            "msgId": "8447783931887",
            "cliMsgId": "1793705979715",
            "uidFrom": "222",
            "dName": "Người gửi",
            "ts": "1793705979842",
            "msgType": "webchat",
            "e2eeStatus": 1,
            "content": CIPHERTEXT,
            "quote": { "msg": "trích dẫn", "cliMsgId": "1" },
        });
        let m = parse_chat_message(&v).expect("vẫn phải hiện bong bóng");
        assert_eq!(m.text, E2EE_PLACEHOLDER_TEXT);
        assert!(!m.text.contains(CIPHERTEXT), "không được lộ chuỗi mã hoá");
        // Clear metadata still builds a usable row: dedup, ordering and reactions need it.
        assert_eq!(m.id, "8447783931887");
        assert_eq!(m.cli_msg_id.as_deref(), Some("1793705979715"));
        assert_eq!(m.sender, "Người gửi");
        assert!(!m.is_outgoing);
        assert_eq!(m.ts, 1793705979842);
        // Nothing that could carry ciphertext or render a broken card survives.
        assert_eq!(
            (m.link_url, m.link_title, m.link_desc, m.link_thumb, m.link_src),
            (None, None, None, None, None)
        );
        assert_eq!(
            (m.sticker_id, m.sticker_cate_id, m.sticker_type),
            (None, None, None)
        );
        assert_eq!(
            (m.reply_to_id, m.reply_to_sender, m.reply_to_text),
            (None, None, None)
        );
    }

    #[test]
    fn e2ee_string_flag_and_bool_flag_also_get_a_placeholder() {
        // `e2eeStatus: "2"` and `e2ee: true` are the other two explicit shapes.
        for extra in [json!({ "e2eeStatus": "2" }), json!({ "e2ee": true })] {
            let mut v = json!({
                "msgId": "8447783936000",
                "uidFrom": "222",
                "ts": "1793705979842",
                "msgType": "webchat",
                "content": "QUJDREVGR0hJSktMTU5PUA==",
            });
            for (k, val) in extra.as_object().unwrap() {
                v[k.as_str()] = val.clone();
            }
            let m = parse_chat_message(&v).expect("phải hiện bong bóng");
            assert_eq!(m.text, E2EE_PLACEHOLDER_TEXT, "{v}");
        }
    }

    #[test]
    fn ordinary_messages_keep_their_text_when_e2ee_is_absent_or_off() {
        // (d) status 0, (e) no field at all (pre-E2EE server), (f) `e2ee: false`.
        for extra in [
            json!({ "e2eeStatus": 0 }),
            json!({ "e2eeStatus": "0" }),
            json!({}),
            json!({ "e2ee": false }),
        ] {
            let mut v = json!({
                "msgId": "8447783937000",
                "uidFrom": "0",
                "idTo": "222",
                "ts": "1793705979842",
                "msgType": "webchat",
                "content": "xin chào",
            });
            for (k, val) in extra.as_object().unwrap() {
                v[k.as_str()] = val.clone();
            }
            let m = parse_chat_message(&v).expect("tin thường phải parse được");
            assert_eq!(m.text, "xin chào", "{v}");
            assert!(m.is_outgoing, "{v}");
        }
    }

    // -- Đợt 2 (dọn nốt): id phía máy chủ + bỏ ghi cache khi không đổi --------------

    #[test]
    fn delete_and_recall_refuse_client_only_ids() {
        let msg = |id: &str| ChatMessage {
            id: id.into(),
            ..Default::default()
        };
        // A server id passes through, trimmed.
        assert_eq!(
            server_message_id(&msg("8447783931887"), "Không xoá được tin này").unwrap(),
            "8447783931887"
        );
        assert_eq!(
            server_message_id(&msg(" 8447783931887 "), "Không thu hồi được tin này").unwrap(),
            "8447783931887"
        );
        // A row that exists only on this client is refused, with the caller's own wording.
        for id in ["", "   ", "out_1793705979842", "local_3"] {
            let err = server_message_id(&msg(id), "Không xoá được tin này")
                .unwrap_err()
                .to_string();
            assert_eq!(err, "Không xoá được tin này (tin chưa có mã từ máy chủ)");
            let err = server_message_id(&msg(id), "Không thu hồi được tin này")
                .unwrap_err()
                .to_string();
            assert_eq!(err, "Không thu hồi được tin này (tin chưa có mã từ máy chủ)");
        }
    }

    #[test]
    fn cache_write_is_skipped_only_for_an_unchanged_snapshot() {
        // First write in this process, and changed content, both write.
        assert!(cache_write_needed(7, None, true), "first write");
        assert!(cache_write_needed(7, Some(9), true), "content changed");
        // Same content with the file still there is the case the skip exists for.
        assert!(!cache_write_needed(7, Some(7), true), "unchanged snapshot");
        // The file vanished: the remembered hash no longer describes what is on disk.
        assert!(cache_write_needed(7, Some(7), false), "file gone");
        // Hash contract: equal text → equal hash, different text → different hash.
        assert_eq!(cache_content_hash("[]"), cache_content_hash("[]"));
        assert_ne!(cache_content_hash("[]"), cache_content_hash("[{}]"));
    }
}

#[cfg(test)]
mod send_identity_tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_when_sends_share_a_clock_tick() {
        let ids: Vec<_> = (0..10_000).map(|_| next_client_message_id()).collect();
        assert!(ids.windows(2).all(|ids| ids[0] < ids[1]));
    }

    #[tokio::test]
    async fn request_client_id_survives_send_variants_and_fallbacks() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::time::Duration;

        // Only loopback fixture traffic is used; all keys and identifiers are synthetic.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let fixture = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for _ in 0..9 {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    headers.push(byte[0]);
                    assert!(headers.len() < 16_384);
                }
                let headers = String::from_utf8(headers).unwrap();
                let size: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap();
                assert!(size < 16_384);
                let mut body = vec![0; size];
                stream.read_exact(&mut body).unwrap();
                let query = reqwest::Url::parse(&format!(
                    "http://fixture.invalid/?{}",
                    String::from_utf8(body).unwrap()
                ))
                .unwrap();
                let encrypted = query
                    .query_pairs()
                    .find(|(key, _)| key == "params")
                    .unwrap()
                    .1
                    .into_owned();
                let body: Value =
                    serde_json::from_str(&crate::crypto::decode_aes(&[7; 32], &encrypted).unwrap())
                        .unwrap();
                let path = headers
                    .lines()
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_string();
                let fail = path.contains("/link?") || path.contains("/quote?");
                requests.push((path, body));
                let response = if fail {
                    r#"{"error_code":114}"#
                } else {
                    r#"{"error_code":0,"data":{"msgId":"server-a"}}"#
                };
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", response.len(), response).unwrap();
            }
            requests
        });
        let mut session = Session::new(false, false, false).unwrap();
        session.login = Some(crate::session::LoginInfo::default());
        session.secret_key = Some(vec![7; 32]);
        session.domains.chat = host.clone();
        session.domains.group = host;
        let client_id = 1_793_425_680_123;
        send_message_with_client_id(&session, "contact-1", "Text", client_id)
            .await
            .unwrap();
        send_group_message_with_client_id(&session, "group-1", "Text", client_id)
            .await
            .unwrap();
        send_link_message_with_client_id(
            &session,
            "contact-1",
            "Text",
            &LinkMetadata {
                url: "https://example.invalid/link".into(),
                title: "Title".into(),
                desc: String::new(),
                thumb: String::new(),
                src: String::new(),
            },
            false,
            client_id,
        )
        .await
        .unwrap();
        send_sticker_with_client_id(&session, "contact-1", 1, 2, 7, false, client_id)
            .await
            .unwrap();
        send_quote_message_with_client_id(
            &session,
            "contact-1",
            "Text",
            &ChatMessage {
                id: "server-quote".into(),
                ..Default::default()
            },
            false,
            client_id,
        )
        .await
        .unwrap();
        forward_message_with_client_id(
            &session,
            "contact-1",
            false,
            &ChatMessage {
                text: "Forward".into(),
                ..Default::default()
            },
            client_id,
        )
        .await
        .unwrap();
        forward_message_with_client_id(
            &session,
            "contact-1",
            false,
            &ChatMessage {
                sticker_id: Some(1),
                ..Default::default()
            },
            client_id,
        )
        .await
        .unwrap();
        let requests = fixture.join().unwrap();
        assert_eq!(requests.len(), 9);
        for (_, body) in requests {
            assert_eq!(body["clientId"], json!(client_id));
        }
    }

    #[test]
    fn extract_file_done_controls_parses_object_and_string_data() {
        let payload = json!({
            "controls": [
                { "content": { "act_type": "file_done", "fileId": 1234567, "data": { "url": "https://f.zdn.vn/a.pdf" } } },
                { "content": { "act_type": "file_done", "fileId": "89", "data": "{\"url\":\"https://f.zdn.vn/b.mp4\"}" } },
                { "content": { "act_type": "voip", "fileId": 1, "data": { "url": "https://x" } } },
                { "content": { "act_type": "file_done", "fileId": 2, "data": {} } }
            ]
        });
        assert_eq!(
            extract_file_done_controls(&payload),
            vec![
                ("1234567".to_string(), "https://f.zdn.vn/a.pdf".to_string()),
                ("89".to_string(), "https://f.zdn.vn/b.mp4".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn file_done_resolves_waiters_registered_before_and_after_the_event() {
        let ev = |id: &str, url: &str| json!({ "controls": [{ "content": { "act_type": "file_done", "fileId": id, "data": { "url": url } } }] });

        // Event arrives while the upload is already waiting.
        let waiter = tokio::spawn(async {
            wait_for_file_done("test_fd_after", std::time::Duration::from_secs(5)).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        dispatch_file_done_controls(&ev("test_fd_after", "https://f/after"));
        assert_eq!(waiter.await.unwrap().as_deref(), Some("https://f/after"));

        // Event arrives before the upload registers its waiter.
        dispatch_file_done_controls(&ev("test_fd_early", "https://f/early"));
        assert_eq!(
            wait_for_file_done("test_fd_early", std::time::Duration::from_secs(5))
                .await
                .as_deref(),
            Some("https://f/early")
        );

        // No event: times out and leaves no stale waiter behind.
        assert_eq!(
            wait_for_file_done("test_fd_none", std::time::Duration::from_millis(20)).await,
            None
        );
        assert!(!FILE_DONE_REGISTRY
            .lock()
            .unwrap()
            .waiters
            .contains_key("test_fd_none"));
    }

    #[test]
    fn extract_mute_controls_parses_set_and_unset_events_for_peers_and_groups() {
        let now = 1700000000_i64;
        let payload = json!({
            "controls": [
                {
                    "content": {
                        "act_type": "mute",
                        "act": "set",
                        "data": {
                            "toId": "123456789",
                            "muteType": 1,
                            "duration": -1,
                            "startTime": now,
                            "muteMode": 1
                        }
                    }
                },
                {
                    "content": {
                        "act_type": "mute",
                        "act": "set",
                        "data": "{\"toId\":\"987654321\",\"muteType\":2,\"duration\":3600,\"startTime\":1700000000,\"muteMode\":1}"
                    }
                },
                {
                    "content": {
                        "act_type": "mute",
                        "act": "unset",
                        "data": {
                            "toId": "123456789",
                            "muteType": 1,
                            "duration": 0,
                            "muteMode": 0
                        }
                    }
                }
            ]
        });

        let events = extract_mute_controls(&payload, now);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0],
            MuteControlEvent {
                thread_id: "123456789".into(),
                is_group: false,
                muted: true,
                muted_until: None,
                settings_until: Some(-1),
            }
        );
        assert_eq!(
            events[1],
            MuteControlEvent {
                thread_id: "g987654321".into(),
                is_group: true,
                muted: true,
                muted_until: Some(now + 3600),
                settings_until: Some(now + 3600),
            }
        );
        assert_eq!(
            events[2],
            MuteControlEvent {
                thread_id: "123456789".into(),
                is_group: false,
                muted: false,
                muted_until: None,
                settings_until: None,
            }
        );
    }

    #[test]
    fn parse_photo_message_hides_raw_cdn_url_and_sets_photo_source() {
        let raw_photo = json!({
            "msgId": "photo_72",
            "msgType": "chat.photo",
            "uidFrom": "1000001",
            "ts": 1700000000000_i64,
            "content": {
                "title": "",
                "description": "",
                "href": "https://photo-stal-zmp.zdn.vn/jpg/55022364c4202d7e7431/999888777.jpg",
                "thumb": "https://photo-stal-zmp.zdn.vn/jpg/55022364c4202d7e7431/999888777_thumb.jpg",
                "params": "{\"width\":1280,\"height\":960}"
            }
        });
        let msg = parse_chat_message(&raw_photo).expect("must parse photo message");
        assert_eq!(msg.link_src.as_deref(), Some("Hình ảnh Zalo"));
        assert_eq!(msg.text, "[Hình ảnh]");
        assert!(is_zalo_photo_cdn_url(msg.link_url.as_deref().unwrap()));
    }

    #[test]
    fn parse_share_file_preserves_sender_doc_filename_instead_of_renew_media() {
        let raw_file = json!({
            "msgId": "file_74",
            "msgType": "share.file",
            "uidFrom": "1000002",
            "ts": 1700000001000_i64,
            "content": {
                "title": "Bao_cao_do_an.doc",
                "description": "",
                "href": "https://f16-zpc.zdn.vn/renewMedia?p=abc123xyz",
                "thumb": "",
                "params": "{\"fileSize\":\"54272\",\"checksum\":\"deadbeef\",\"fileExt\":\"doc\"}"
            }
        });
        let msg = parse_chat_message(&raw_file).expect("must parse file message");
        assert_eq!(msg.link_src.as_deref(), Some("Tập tin Zalo"));
        assert_eq!(msg.link_title.as_deref(), Some("Bao_cao_do_an.doc"));
        assert_eq!(msg.text, "[Tập tin] Bao_cao_do_an.doc");
        let resolved = resolve_attachment_file_name(
            msg.link_title.as_deref(),
            &msg.text,
            msg.link_url.as_deref().unwrap_or_default(),
        );
        assert_eq!(resolved, "Bao_cao_do_an.doc");

        // Also verify when `title` omits `.doc` but `params.fileExt` has `"doc"`
        let raw_file_no_ext = json!({
            "msgId": "file_74_b",
            "msgType": "share.file",
            "uidFrom": "1000002",
            "ts": 1700000002000_i64,
            "content": {
                "title": "De_cuong_mon_hoc",
                "description": "",
                "href": "https://f16-zpc.zdn.vn/renewMedia?p=def456",
                "params": "{\"fileSize\":\"12345\",\"fileExt\":\"doc\"}"
            }
        });
        let msg_b = parse_chat_message(&raw_file_no_ext).expect("must parse file message");
        assert_eq!(msg_b.link_src.as_deref(), Some("Tập tin Zalo"));
        assert_eq!(msg_b.link_title.as_deref(), Some("De_cuong_mon_hoc.doc"));
    }

    #[test]
    fn extract_mobile_sync_controls_parses_transfer_confirm_and_backup_ready() {
        let payload = json!({
            "controls": [
                {
                    "content": {
                        "act_type": "transfer_msg",
                        "act": "transfer_after_login",
                        "data": "{\"temp_key\":\"tk_login_999\"}"
                    }
                },
                {
                    "content": {
                        "act_type": "syncmsgmb",
                        "act": "user_confirm",
                        "data": { "user_action": 1, "pc_name": "DESKTOP" }
                    }
                },
                {
                    "content": {
                        "act_type": "syncmsgmb",
                        "act": "syncmsg_info",
                        "data": {
                            "url": "https://f1-zpc.zdn.vn/backup.zdb",
                            "file_name": "backup.zdb",
                            "encrypted_key": "enc_key_b64",
                            "checksum_code": "md5_123"
                        }
                    }
                }
            ]
        });
        let evs = extract_mobile_sync_controls(&payload);
        assert_eq!(evs.len(), 3);
        assert_eq!(
            evs[0],
            MobileSyncControlEvent::TransferAfterLogin {
                temp_key: "tk_login_999".into()
            }
        );
        assert_eq!(evs[1], MobileSyncControlEvent::UserConfirmed);
        assert_eq!(
            evs[2],
            MobileSyncControlEvent::BackupReady {
                url: "https://f1-zpc.zdn.vn/backup.zdb".into(),
                file_name: "backup.zdb".into(),
                encrypted_key: "enc_key_b64".into(),
                checksum: "md5_123".into(),
            }
        );
    }
}

#[cfg(test)]
mod generic_name_tests {
    use super::*;
    #[test]
    fn placeholders_are_generic_but_real_names_starting_with_nhom_are_kept() {
        for name in ["", "  ", "12345", "Nhóm 6611225", "Người dùng 42"] {
            assert!(is_generic_thread_name(name), "{name}");
        }
        for name in ["Nhóm bạn thân", "Nhóm 12A1 cũ", "Người dùng mới", "An"] {
            assert!(!is_generic_thread_name(name), "{name}");
        }
    }
    #[test]
    fn group_member_count_uses_valid_server_values_and_survives_merge() {
        let mut group = Thread { id: "group_1".into(), is_group: true, ..Default::default() };
        group.apply_group_sidebar_metadata(&serde_json::json!({"totalMember": " 42 "}));
        assert_eq!(group.presentation.member_count, Some(42));
        group.apply_group_sidebar_metadata(&serde_json::json!({"totalMember": 0}));
        assert_eq!(group.presentation.member_count, Some(42));
        let mut existing = Thread { id: "group_1".into(), is_group: true, ..Default::default() };
        merge_thread_into(&mut existing, group, true);
        assert_eq!(existing.presentation.member_count, Some(42));
    }

}
