use super::*;

impl ZaloGuiApp {
    /// Start loading the full-resolution texture the image viewer shows. Message bubbles and
    /// the media grid keep a 480 px copy; the `hd_` key keeps this one (and its disk cache) apart.
    pub(super) fn request_hd_texture(&mut self, full_url: &str) -> Option<String> {
        let lower_u = full_url.to_lowercase();
        let is_direct_img = lower_u.contains("zdn.vn")
            || lower_u.ends_with(".png")
            || lower_u.ends_with(".jpg")
            || lower_u.ends_with(".jpeg")
            || lower_u.ends_with(".webp")
            || lower_u.ends_with(".gif")
            || lower_u.ends_with(".bmp")
            || (!lower_u.starts_with("http://") && !lower_u.starts_with("https://"));
        if full_url.is_empty() || !is_direct_img {
            return None;
        }
        let hk = format!("hd_{}", crate::crypto::md5_hex(full_url));
        if !self.sticker_textures.contains_key(&hk) && self.loading_stickers.insert(hk.clone()) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key: hk.clone(),
                    url: full_url.to_string(),
                });
            }
        }
        Some(hk)
    }

    pub(super) fn render_chat_workspace(&mut self, ctx: &egui::Context, pal: &ThemePalette) {
        // Responsive breakpoints based on live viewport width
        // -------------------------------------------------------------------
        let screen_rect = ctx.screen_rect();
        let screen_w = screen_rect.width();
        let is_compact_w = false;
        let is_narrow_w = false;
        let left_rail_w = 62.0;
        // The right info drawer only fits on wide windows.
        let drawer_allowed = screen_w >= 1000.0;

        // -------------------------------------------------------------------
        // 1. LEFT NAVIGATION SIDEBAR + 2. CONVERSATION LIST (both collapsible)
        // -------------------------------------------------------------------
        // Ctrl/Cmd+K focuses conversation search (expanding the list if needed).
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::K)) {
            if self.settings.list_collapsed {
                self.settings.list_collapsed = false;
                self.settings.save();
            }
            self.search_focus_pending = true;
        }
        // Below ~760 px both columns must collapse (hard). Between 760 and 1100 px the nav
        // auto-collapses to its icon rail unless the user expanded it manually this session; the
        // automatic state is never written back to the persisted preference.
        let nav_forced = screen_w < 760.0;
        let nav_auto_collapse = screen_w < 1100.0;
        let list_forced = screen_w < 760.0;
        let nav_expanded = !nav_forced
            && match self.nav_user_override {
                Some(expanded) => expanded,
                None => !self.settings.nav_collapsed && !nav_auto_collapse,
            };
        let list_expanded = !self.settings.list_collapsed && !list_forced;
        self.render_nav_sidebar(ctx, pal, nav_expanded, nav_forced);
        let list_max_w = (screen_w * 0.42).clamp(260.0, 440.0);
        self.render_conversation_list(ctx, pal, list_expanded, list_forced, list_max_w);

        if self.contacts_view_open {
            self.render_contacts_page(ctx, pal);
            self.render_profile_popover(ctx, pal);
            self.render_wipe_confirm(ctx, pal);
            self.render_toast_overlay(ctx, pal);
            self.render_forward_dialog(ctx, pal);
            self.render_call_overlay(ctx, pal);
            self.render_image_viewer_modal(ctx, pal);
            self.render_phone_lookup_modal(ctx, pal);
            self.render_settings_modal(ctx, pal);
            self.render_schedule_modal(ctx, pal);
            return;
        }
        if self.feed_modal_open {
            self.render_feed_page(ctx, pal);
            self.render_profile_popover(ctx, pal);
            self.render_wipe_confirm(ctx, pal);
            self.render_toast_overlay(ctx, pal);
            self.render_forward_dialog(ctx, pal);
            self.render_call_overlay(ctx, pal);
            self.render_image_viewer_modal(ctx, pal);
            self.render_phone_lookup_modal(ctx, pal);
            self.render_settings_modal(ctx, pal);
            return;
        }
        // Extract lightweight metadata for Header, Right Info Drawer, and Central Chat Stream
        let thread_meta = self.threads.get(self.selected_thread_index).map(|t| {
            (
                t.id.clone(),
                t.name.clone(),
                t.avatar.clone(),
                t.is_group,
                t.messages.len(),
            )
        });

        // -------------------------------------------------------------------
        // 3. COLLAPSIBLE RIGHT CONVERSATION INFO DRAWER (responsive width)
        // -------------------------------------------------------------------
        let mut open_chat_member: Option<crate::chat::SidebarMember> = None;
        let mut view_profile_member: Option<String> = None;
        let show_info_drawer = self.info_drawer_open && drawer_allowed;
        let drawer_w = (screen_w * 0.26).clamp(240.0, 310.0);

        if show_info_drawer {
            if let Some((ref d_id, ref d_name, ref d_ava, d_is_group, d_msg_count)) = thread_meta {
                let shared_photos: Vec<(String, String, String, String)> = self
                    .threads
                    .get(self.selected_thread_index)
                    .map(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .filter_map(|m| {
                                // Rows saved before v0.1.2 hold a Zalo-CDN URL as text.
                                let legacy = legacy_media_view(m);
                                let m = legacy.as_ref().unwrap_or(m);
                                let is_photo = m.link_src.as_deref() == Some("Hình ảnh Zalo")
                                    || m.text.trim() == "[Hình ảnh]"
                                    || m.text.trim().starts_with("[Hình ảnh] ");
                                if is_sticker_message(m).is_some() {
                                    return None;
                                }
                                if !is_photo {
                                    return None;
                                }
                                let thumb = m
                                    .link_thumb
                                    .as_deref()
                                    .or(m.link_url.as_deref())
                                    .filter(|u| !u.is_empty())?;
                                let full = m.link_url.as_deref().unwrap_or(thumb).to_string();
                                let title = m
                                    .link_title
                                    .clone()
                                    .unwrap_or_else(|| "Hình ảnh đã chia sẻ".into());
                                Some((
                                    format!("thumb_{}", crate::crypto::md5_hex(thumb)),
                                    thumb.to_string(),
                                    full,
                                    title,
                                ))
                            })
                            .take(9)
                            .collect()
                    })
                    .unwrap_or_default();

                egui::SidePanel::right("conversation_info_drawer")
                    .resizable(false)
                    .exact_width(drawer_w)
                    .frame(
                        egui::Frame::none()
                            .fill(pal.bg_list)
                            .stroke(Stroke::new(1.0_f32, pal.border)),
                    )
                    .show(ctx, |ui| {
                        ui.set_max_width(drawer_w);
                        egui::Frame::none()
                            .inner_margin(Margin::symmetric(16.0, 18.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    let (av_rect, _) = ui
                                        .allocate_exact_size(Vec2::new(64.0, 64.0), Sense::hover());
                                    paint_user_avatar(
                                        ui.painter(),
                                        av_rect.center(),
                                        32.0,
                                        d_name,
                                        d_is_group,
                                        d_ava.as_deref(),
                                        &self.sticker_textures,
                                        &mut self.loading_stickers,
                                    );
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new(d_name)
                                            .size(14.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.add_space(2.0);
                                    let d_is_cloud = d_name.contains("Cloud của tôi");
                                    let d_fr_status = if d_is_group || d_is_cloud {
                                        crate::chat::FriendshipStatus::Friend
                                    } else {
                                        self.friend_status_map
                                            .get(d_id)
                                            .copied()
                                            .unwrap_or(crate::chat::FriendshipStatus::Unknown)
                                    };
                                    let d_onl = self.user_online_map.get(d_id);
                                    let (sub_txt, sub_col): (String, Color32) = if d_is_group {
                                        ("Nhóm đang hoạt động".into(), pal.accent_online)
                                    } else if d_is_cloud {
                                        ("Lưu trữ đám mây cá nhân".into(), pal.primary_hover)
                                    } else {
                                        match d_fr_status {
                                            crate::chat::FriendshipStatus::Friend => match d_onl {
                                                Some((true, lbl)) if !lbl.is_empty() => (
                                                    format!("✓ Đã kết bạn · {lbl}"),
                                                    pal.accent_online,
                                                ),
                                                Some((false, lbl)) if !lbl.is_empty() => (
                                                    format!("✓ Đã kết bạn · {lbl}"),
                                                    pal.text_secondary,
                                                ),
                                                _ => ("✓ Đã kết bạn".into(), pal.text_secondary),
                                            },
                                            crate::chat::FriendshipStatus::IncomingPending => (
                                                "📩 Đã gửi lời mời kết bạn cho bạn".into(),
                                                pal.primary_hover,
                                            ),
                                            crate::chat::FriendshipStatus::OutgoingPending => {
                                                ("⏳ Đã gửi lời mời kết bạn".into(), pal.warning)
                                            }
                                            crate::chat::FriendshipStatus::Stranger => (
                                                "Người lạ · Chưa kết bạn".into(),
                                                Color32::from_rgb(249, 115, 22),
                                            ),
                                            crate::chat::FriendshipStatus::Unknown => {
                                                ("Đang kiểm tra kết bạn…".into(), pal.text_muted)
                                            }
                                        }
                                    };
                                    ui.label(RichText::new(sub_txt).size(12.0).color(sub_col));

                                    ui.add_space(14.0);
                                    // Quick Action Buttons (Sync & Pin/Star Filter)
                                    ui.horizontal(|ui| {
                                        ui.add_space(34.0);
                                        let sync_btn = egui::Button::new(
                                            RichText::new("Làm mới")
                                                .size(11.5)
                                                .color(pal.text_secondary),
                                        )
                                        .fill(pal.bg_elevated)
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(92.0, 30.0));
                                        if ui
                                            .add(sync_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::RefreshThreads);
                                                let _ = tx.send(WorkerCmd::LoadGroupHistory {
                                                    group_id: d_id.clone(),
                                                    is_group: d_is_group,
                                                    have: d_msg_count,
                                                });
                                            }
                                            self.show_toast("Đang làm mới hội thoại…");
                                        }

                                        let star_active = self.filter_starred_only;
                                        let star_btn =
                                            egui::Button::new(
                                                RichText::new(if star_active {
                                                    "Hiện tất cả"
                                                } else {
                                                    "Tin đánh dấu"
                                                })
                                                .size(11.5)
                                                .color(if star_active {
                                                    Color32::WHITE
                                                } else {
                                                    pal.text_secondary
                                                }),
                                            )
                                            .fill(if star_active {
                                                pal.primary
                                            } else {
                                                pal.bg_elevated
                                            })
                                            .stroke(Stroke::new(1.0_f32, pal.border))
                                            .rounding(Rounding::same(8.0))
                                            .min_size(Vec2::new(92.0, 30.0));
                                        if ui
                                            .add(star_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.filter_starred_only = !self.filter_starred_only;
                                        }
                                    });
                                    if !d_is_group && !d_is_cloud {
                                        ui.add_space(6.0);
                                        let prof_btn = egui::Button::new(
                                            RichText::new(format!(
                                                "{} Xem Hồ sơ & Nhật ký",
                                                lucide::NEWSPAPER
                                            ))
                                            .size(11.5)
                                            .color(pal.primary_hover),
                                        )
                                        .fill(pal.primary.linear_multiply(0.12))
                                        .stroke(Stroke::new(1.0_f32, pal.primary))
                                        .rounding(Rounding::same(8.0))
                                        .min_size(Vec2::new(192.0, 28.0));
                                        if ui
                                            .add(prof_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.feed_modal_open = true;
                                            self.feed_modal_tab = FeedModalTab::Profile;
                                            self.feed_target_uid = d_id.clone();
                                            self.feed_loading_profile = true;
                                            self.feed_profile_error = None;
                                            self.trigger_feed_timeline_load();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchSocialProfile {
                                                    target_uid: d_id.clone(),
                                                });
                                            }
                                        }
                                    }
                                });
                            });

                        ui.add(egui::Separator::default().spacing(1.0));

                        // Group Members List (`Thành viên nhóm`)
                        if d_is_group {
                            let group_members = self
                                .group_members_map
                                .get(d_id)
                                .cloned()
                                .or_else(|| {
                                    self.threads
                                        .get(self.selected_thread_index)
                                        .map(|t| t.presentation.members.clone())
                                })
                                .unwrap_or_default();
                            let is_fetching = self.fetching_group_members.contains(d_id);

                            egui::Frame::none()
                                .inner_margin(Margin::symmetric(16.0, 14.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("Thành viên nhóm")
                                                .size(12.5)
                                                .color(pal.text_primary)
                                                .strong(),
                                        );
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let refresh_resp = ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(lucide::REFRESH)
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                    )
                                                    .fill(Color32::TRANSPARENT)
                                                    .frame(false),
                                                )
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Tải lại danh sách thành viên");
                                            if refresh_resp.clicked() {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::FetchGroupMembers {
                                                        group_id: d_id.clone(),
                                                    });
                                                }
                                                self.fetching_group_members.insert(d_id.clone());
                                            }

                                            ui.label(
                                                RichText::new(format!("({})", group_members.len()))
                                                    .size(11.5)
                                                    .color(pal.primary_hover),
                                            );
                                        });
                                    });
                                    ui.add_space(8.0);

                                    if is_fetching && group_members.is_empty() {
                                        ui.horizontal(|ui| {
                                            ui.spinner();
                                            ui.label(
                                                RichText::new("Đang tải danh sách thành viên…")
                                                    .size(11.5)
                                                    .color(pal.text_muted),
                                            );
                                        });
                                    } else if group_members.is_empty() {
                                        let load_btn = egui::Button::new(
                                            RichText::new("Tải danh sách thành viên")
                                                .size(11.5)
                                                .color(pal.primary_hover),
                                        )
                                        .fill(pal.primary.linear_multiply(0.12))
                                        .stroke(Stroke::new(1.0_f32, pal.primary))
                                        .rounding(Rounding::same(6.0));
                                        if ui.add(load_btn).clicked() {
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::FetchGroupMembers {
                                                    group_id: d_id.clone(),
                                                });
                                            }
                                            self.fetching_group_members.insert(d_id.clone());
                                        }
                                    } else {
                                        if group_members.len() > 5 {
                                            ui.add(
                                                egui::TextEdit::singleline(&mut self.group_member_search)
                                                    .hint_text("🔍 Tìm thành viên…")
                                                    .margin(Margin::symmetric(6.0, 3.0)),
                                            );
                                            ui.add_space(4.0);
                                        }

                                        let q = self.group_member_search.trim().to_lowercase();
                                        let filtered_members: Vec<&crate::chat::SidebarMember> = group_members
                                            .iter()
                                            .filter(|m| q.is_empty() || m.name.to_lowercase().contains(&q) || m.id.contains(&q))
                                            .collect();

                                        egui::ScrollArea::vertical()
                                            .id_salt("info_drawer_members_scroll")
                                            .max_height(200.0)
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.spacing_mut().item_spacing = Vec2::new(0.0, 5.0);
                                                for m in filtered_members {
                                                    ui.horizontal(|ui| {
                                                        let (av_r, _) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::hover());
                                                        paint_user_avatar(
                                                            ui.painter(),
                                                            av_r.center(),
                                                            13.0,
                                                            &m.name,
                                                            false,
                                                            m.avatar.as_deref(),
                                                            &self.sticker_textures,
                                                            &mut self.loading_stickers,
                                                        );

                                                        ui.add_space(4.0);
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    RichText::new(truncate_chars(&m.name, 16))
                                                                        .size(12.0)
                                                                        .color(pal.text_primary)
                                                                        .strong(),
                                                                );
                                                                if m.role == "creator" {
                                                                    ui.label(
                                                                        RichText::new("Trưởng nhóm")
                                                                            .size(9.5)
                                                                            .color(Color32::from_rgb(245, 158, 11)),
                                                                    );
                                                                } else if m.role == "admin" {
                                                                    ui.label(
                                                                        RichText::new("Phó nhóm")
                                                                            .size(9.5)
                                                                            .color(pal.primary_hover),
                                                                    );
                                                                }
                                                            });
                                                        });

                                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                            let chat_btn = ui.add(
                                                                egui::Button::new(
                                                                    RichText::new(lucide::SEND)
                                                                        .size(11.0)
                                                                        .color(pal.text_secondary),
                                                                )
                                                                .fill(Color32::TRANSPARENT)
                                                                .frame(false),
                                                            )
                                                            .on_hover_cursor(CursorIcon::PointingHand)
                                                            .on_hover_text("Nhắn tin riêng");
                                                            if chat_btn.clicked() {
                                                                open_chat_member = Some(m.clone());
                                                            }

                                                            let prof_btn = ui.add(
                                                                egui::Button::new(
                                                                    RichText::new("🪪")
                                                                        .size(10.5)
                                                                )
                                                                .fill(Color32::TRANSPARENT)
                                                                .frame(false),
                                                            )
                                                            .on_hover_cursor(CursorIcon::PointingHand)
                                                            .on_hover_text("Xem trang cá nhân");
                                                            if prof_btn.clicked() {
                                                                view_profile_member = Some(m.id.clone());
                                                            }
                                                        });
                                                    });
                                                }
                                            });
                                    }
                                });

                            ui.add(egui::Separator::default().spacing(1.0));
                        }

                        // Shared Media Grid (`Ảnh / Video đã chia sẻ`)
                        egui::Frame::none()
                            .inner_margin(Margin::symmetric(16.0, 14.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Ảnh / Video đã chia sẻ")
                                            .size(12.5)
                                            .color(pal.text_primary)
                                            .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("({})", shared_photos.len()))
                                                .size(11.5)
                                                .color(pal.primary_hover),
                                        );
                                    });
                                });
                                ui.add_space(8.0);

                                if shared_photos.is_empty() {
                                    ui.label(
                                        RichText::new("Chưa có ảnh hoặc video chia sẻ.")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                } else {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                                        for (tkey, thumb_u, full_u, ptitle) in &shared_photos {
                                            if !self.sticker_textures.contains_key(tkey)
                                                && self.loading_stickers.insert(tkey.clone())
                                            {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::FetchStickerImage {
                                                        key: tkey.clone(),
                                                        url: thumb_u.clone(),
                                                    });
                                                }
                                            }
                                            let (cell_r, cell_resp) = ui.allocate_exact_size(
                                                Vec2::splat(74.0),
                                                Sense::click(),
                                            );
                                            ui.painter().rect_filled(
                                                cell_r,
                                                Rounding::same(8.0),
                                                pal.bg_elevated,
                                            );
                                            if let Some(tex) = self.sticker_textures.get(tkey) {
                                                ui.painter().image(
                                                    tex.id(),
                                                    cell_r,
                                                    Rect::from_min_max(
                                                        Pos2::ZERO,
                                                        Pos2::new(1.0, 1.0),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            } else {
                                                draw_icon_image(
                                                    ui.painter(),
                                                    cell_r.center(),
                                                    20.0,
                                                    pal.text_muted,
                                                );
                                            }
                                            ui.painter().rect_stroke(
                                                cell_r,
                                                Rounding::same(8.0),
                                                Stroke::new(
                                                    1.0_f32,
                                                    if cell_resp.hovered() {
                                                        pal.primary
                                                    } else {
                                                        pal.border
                                                    },
                                                ),
                                            );
                                            if cell_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Xem ảnh phóng to")
                                                .clicked()
                                            {
                                                self.image_viewer = Some(ImageViewerModal {
                                                    tex_key: tkey.clone(),
                                                    hd_tex_key: self.request_hd_texture(full_u),
                                                    full_url: full_u.clone(),
                                                    title: ptitle.clone(),
                                                    sender: d_name.clone(),
                                                    time: String::new(),
                                                    zoom: 1.0,
                                                    pan: Vec2::ZERO,
                                                });
                                            }
                                        }
                                    });
                                }
                            });

                        // Push technical metadata to the bottom of the drawer
                        let rem = (ui.available_height() - 88.0).max(8.0);
                        ui.add_space(rem);
                        ui.add(egui::Separator::default().spacing(1.0));

                        egui::Frame::none()
                            .fill(pal.bg_app)
                            .inner_margin(Margin::symmetric(16.0, 12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Tổng số tin nhắn:")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("{d_msg_count} tin nhắn"))
                                                .size(11.5)
                                                .color(pal.text_secondary)
                                                .strong(),
                                        );
                                    });
                                });
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Mã định danh (ID):")
                                            .size(11.5)
                                            .color(pal.text_muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let char_cnt = d_id.chars().count();
                                        let short_id = if char_cnt > 10 {
                                            let prefix: String = d_id.chars().take(4).collect();
                                            let suffix: String =
                                                d_id.chars().skip(char_cnt - 4).collect();
                                            format!("{prefix}...{suffix}")
                                        } else {
                                            d_id.clone()
                                        };
                                        let (id_rect, id_resp) = ui.allocate_exact_size(
                                            Vec2::new(98.0, 22.0),
                                            Sense::click(),
                                        );
                                        ui.painter().rect_filled(
                                            id_rect,
                                            Rounding::same(5.0),
                                            if id_resp.hovered() {
                                                pal.bg_hover
                                            } else {
                                                pal.bg_elevated
                                            },
                                        );
                                        ui.painter().text(
                                            Pos2::new(id_rect.left() + 7.0, id_rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            short_id,
                                            FontId::monospace(10.5),
                                            pal.text_secondary,
                                        );
                                        draw_icon_copy(
                                            ui.painter(),
                                            Pos2::new(id_rect.right() - 10.0, id_rect.center().y),
                                            11.5,
                                            pal.text_secondary,
                                        );
                                        if id_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text(format!("Sao chép ID: {d_id}"))
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(d_id.clone());
                                            self.show_toast(
                                                "Đã sao chép ID hội thoại vào bộ nhớ tạm",
                                            );
                                        }
                                    });
                                });
                            });
                    });
            }
        }

        if let Some(m) = open_chat_member {
            if let Some(idx) = self.threads.iter().position(|t| t.id == m.id) {
                self.selected_thread_index = idx;
            } else {
                self.threads.insert(
                    0,
                    Thread {
                        presentation: Default::default(),
                        unread: Default::default(),
                        id: m.id.clone(),
                        name: m.name.clone(),
                        preview: "Bắt đầu cuộc trò chuyện".to_string(),
                        time: crate::ui::clock(),
                        avatar: m.avatar.clone(),
                        is_group: false,
                        messages: Vec::new(),
                    },
                );
                self.selected_thread_index = 0;
                self.persist_threads_async();
            }
            self.show_toast(format!("Đã mở cuộc trò chuyện với {}", m.name));
        }

        if let Some(uid) = view_profile_member {
            self.feed_modal_open = true;
            self.feed_modal_tab = FeedModalTab::Profile;
            self.feed_target_uid = uid.clone();
            self.feed_loading_profile = true;
            self.feed_profile_error = None;
            self.trigger_feed_timeline_load();
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchSocialProfile {
                    target_uid: uid,
                });
            }
        }

        // -------------------------------------------------------------------
        // 4. CENTRAL CHAT AREA (Clean Header, Messages Stream, Icon Toolbar)
        // -------------------------------------------------------------------
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(pal.bg_chat_canvas))
            .show(ctx, |ui| {
                // Soft diagonal gradient behind the whole chat column (header, stream, composer).
                paint_diagonal_gradient(ui.painter(), ui.max_rect(), pal.canvas_top, pal.canvas_bottom);
                // Breakpoints follow the real chat-column width (columns may be collapsed or not).
                let chat_w = ui.available_width();
                let is_compact_w = chat_w < 560.0;
                if let Some((thread_id, thread_name, thread_avatar, is_group, _msg_count)) =
                    thread_meta
                {
                    let is_cloud_thread = thread_name.contains("Cloud của tôi");
                    if !is_group
                        && !is_cloud_thread
                        && !self.friend_status_map.contains_key(&thread_id)
                        && self.checking_friend_status.insert(thread_id.clone())
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::CheckFriendStatus {
                                user_id: thread_id.clone(),
                            });
                        }
                    }
                    if is_group
                        && !self.group_members_map.contains_key(&thread_id)
                        && self.fetching_group_members.insert(thread_id.clone())
                    {
                        if let Some(tx) = WORKER_TX.get() {
                            let _ = tx.send(WorkerCmd::FetchGroupMembers {
                                group_id: thread_id.clone(),
                            });
                        }
                    }
                    let cur_friendship = if is_group || is_cloud_thread {
                        crate::chat::FriendshipStatus::Friend
                    } else {
                        self.friend_status_map
                            .get(&thread_id)
                            .copied()
                            .unwrap_or(crate::chat::FriendshipStatus::Unknown)
                    };
                    // 1. Clean Conversation Header Bar (No raw 19-digit ID, no bulky sync button)
                    let header_frame = egui::Frame::none()
                        .fill(if pal.is_dark {
                            Color32::from_rgba_unmultiplied(33, 37, 43, 200)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 120)
                        })
                        .inner_margin(Margin::symmetric(18.0, 12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Header avatar (44×44)
                                let (av_rect, _) =
                                    ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
                                paint_user_avatar(
                                    ui.painter(),
                                    av_rect.center(),
                                    20.0,
                                    &thread_name,
                                    is_group,
                                    thread_avatar.as_deref(),
                                    &self.sticker_textures,
                                    &mut self.loading_stickers,
                                );
                                let cur_onl = self.user_online_map.get(&thread_id);
                                if !is_group
                                    && cur_onl.map(|(onl, _)| *onl).unwrap_or(false)
                                {
                                    let dot_pos =
                                        Pos2::new(av_rect.right() - 4.0, av_rect.bottom() - 4.0);
                                    ui.painter().circle_filled(dot_pos, 6.2, pal.bg_sidebar);
                                    ui.painter().circle_filled(dot_pos, 4.2, pal.accent_online);
                                }
                                ui.add_space(6.0);
                                // Reserve room for the right-hand action cluster so the name/tag
                                // block can never be overlapped by the call buttons.
                                let actions_w = if is_compact_w { 126.0 } else { 182.0 };
                                let left_info_w =
                                    (ui.available_width() - actions_w).max(60.0);
                                ui.allocate_ui_with_layout(
                                    Vec2::new(left_info_w, 44.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.spacing_mut().item_spacing.y = 1.0;
                                        ui.add_space(2.0);
                                        ui.horizontal(|ui| {
                                            // Only relationship states that need attention get a pill.
                                            let tag = if is_group || is_cloud_thread {
                                                None
                                            } else {
                                                match cur_friendship {
                                                    crate::chat::FriendshipStatus::IncomingPending => Some((
                                                        "Chờ đồng ý kết bạn",
                                                        pal.primary,
                                                    )),
                                                    crate::chat::FriendshipStatus::OutgoingPending => Some((
                                                        "Đã gửi lời mời",
                                                        pal.warning,
                                                    )),
                                                    crate::chat::FriendshipStatus::Stranger => Some((
                                                        "Người lạ",
                                                        Color32::from_rgb(234, 108, 20),
                                                    )),
                                                    _ => None,
                                                }
                                            };
                                            let tag_w = if tag.is_some() && left_info_w > 200.0 { 120.0 } else { 0.0 };
                                            let g = crate::gui::sidebar::single_line_galley(
                                                ui,
                                                &thread_name,
                                                16.5,
                                                pal.text_primary,
                                                (left_info_w - tag_w).max(40.0),
                                            );
                                            let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x, 22.0), Sense::hover());
                                            ui.painter().galley(Pos2::new(r.left(), r.center().y - g.size().y / 2.0), g, pal.text_primary);
                                            if let (Some((lbl, col)), true) = (tag, tag_w > 0.0) {
                                                egui::Frame::none()
                                                    .fill(col.linear_multiply(0.14))
                                                    .rounding(Rounding::same(8.0))
                                                    .inner_margin(Margin::symmetric(7.0, 1.5))
                                                    .show(ui, |ui| {
                                                        ui.label(RichText::new(lbl).size(10.8).color(col));
                                                    });
                                            }
                                        });
                                        let is_typing_now = self.settings.show_typing_indicator
                                            && self.typing_map.get(&thread_id)
                                                .map(|(_, until)| *until > std::time::Instant::now())
                                                .unwrap_or(false);
                                        let (sub_status_lbl, sub_status_col): (String, Color32) =
                                            if is_group {
                                                let count = self.group_member_counts.get(&thread_id).map(|n| *n as u64)
                                                    .or_else(|| self.threads.get(self.selected_thread_index).and_then(|t| t.presentation.member_count));
                                                (count.map(|n| format!("{n} thành viên")).unwrap_or_else(|| "Nhóm trò chuyện".into()), pal.text_secondary)
                                            } else if is_cloud_thread {
                                                ("Lưu trữ đám mây cá nhân".into(), pal.text_secondary)
                                            } else {
                                                match cur_friendship {
                                                    crate::chat::FriendshipStatus::Friend => {
                                                        match cur_onl {
                                                            Some((true, lbl)) if !lbl.is_empty() => (
                                                                format!("Đã kết bạn · {lbl}"),
                                                                pal.accent_online,
                                                            ),
                                                            Some((false, lbl)) if !lbl.is_empty() => (
                                                                format!("Đã kết bạn · {lbl}"),
                                                                pal.text_secondary,
                                                            ),
                                                            _ => (
                                                                "Đã kết bạn".into(),
                                                                pal.text_secondary,
                                                            ),
                                                        }
                                                    }
                                                    crate::chat::FriendshipStatus::IncomingPending => (
                                                        "Đã gửi lời mời kết bạn cho bạn".into(),
                                                        pal.primary_hover,
                                                    ),
                                                    crate::chat::FriendshipStatus::OutgoingPending => (
                                                        "Đã gửi lời mời kết bạn · Chờ đồng ý".into(),
                                                        pal.warning,
                                                    ),
                                                    crate::chat::FriendshipStatus::Stranger => (
                                                        "Người lạ · Chưa kết bạn trên Zalo".into(),
                                                        Color32::from_rgb(249, 115, 22),
                                                    ),
                                                    crate::chat::FriendshipStatus::Unknown => {
                                                        match cur_onl {
                                                            Some((true, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.accent_online,
                                                            ),
                                                            Some((false, lbl)) if !lbl.is_empty() => (
                                                                lbl.clone(),
                                                                pal.text_secondary,
                                                            ),
                                                            _ => (
                                                                "Trò chuyện cá nhân".into(),
                                                                pal.text_secondary,
                                                            ),
                                                        }
                                                    }
                                                }
                                            };
                                        let sub_resp = ui.add(
                                            egui::Label::new(
                                                RichText::new(sub_status_lbl)
                                                    .size(11.6)
                                                    .color(sub_status_col),
                                            )
                                            .sense(if is_group { egui::Sense::click() } else { egui::Sense::hover() }),
                                        );
                                        if is_group {
                                            let sub_resp = sub_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Xem thông tin và thành viên nhóm");
                                            if sub_resp.clicked() {
                                                self.info_drawer_open = true;
                                                self.settings.info_drawer_open = true;
                                                self.settings.save();
                                            }
                                        }
                                    },
                                );

                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Info Drawer Toggle Button (hidden on narrow windows: auto-collapsed)
                                    if drawer_allowed {
                                        let (drw_rect, drw_resp) = ui.allocate_exact_size(
                                            Vec2::new(34.0, 34.0),
                                            Sense::click(),
                                        );
                                        ui.painter().rect_filled(
                                            drw_rect,
                                            Rounding::same(8.0),
                                            if self.info_drawer_open {
                                                pal.primary.linear_multiply(0.18)
                                            } else if drw_resp.hovered() {
                                                pal.bg_hover
                                            } else {
                                                Color32::TRANSPARENT
                                            },
                                        );
                                        draw_icon_drawer(
                                            ui.painter(),
                                            drw_rect.center(),
                                            17.0,
                                            if self.info_drawer_open {
                                                pal.primary_hover
                                            } else if drw_resp.hovered() {
                                                pal.text_primary
                                            } else {
                                                pal.text_secondary
                                            },
                                        );
                                        if drw_resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Thông tin hội thoại")
                                            .clicked()
                                        {
                                            self.info_drawer_open = !self.info_drawer_open;
                                            self.settings.info_drawer_open = self.info_drawer_open;
                                            self.settings.save();
                                        }
                                    }

                                    ui.add_space(4.0);

                                    // Video Call Button
                                    let (vid_rect, vid_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    if vid_resp.hovered() {
                                        ui.painter().rect_filled(
                                            vid_rect,
                                            Rounding::same(8.0),
                                            pal.bg_hover,
                                        );
                                    }
                                    draw_icon_video(
                                        ui.painter(),
                                        vid_rect.center(),
                                        17.5,
                                        if vid_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if vid_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Gọi Video")
                                        .clicked()
                                    {
                                        let ff_ok = crate::media::is_ffmpeg_available();
                                        let (_, cams) = crate::media::list_media_devices(false);
                                        let can_cam = ff_ok && !cams.is_empty();
                                        if !ff_ok {
                                            self.show_toast(
                                                "⚠️ Không tìm thấy ffmpeg trong PATH để bật Camera (cài bằng: winget install Gyan.FFmpeg)",
                                            );
                                        } else if cams.is_empty() {
                                            self.show_toast(
                                                "⚠️ Không phát hiện thiết bị Camera trên máy — đang gọi không hình",
                                            );
                                        }
                                        self.active_call = Some(ActiveCallOverlay {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            is_video: true,
                                            is_group,
                                            state_label: "Đang kết nối cuộc gọi video…".into(),
                                            started_at: std::time::Instant::now(),
                                            mic_muted: false,
                                            cam_enabled: can_cam,
                                            is_incoming_ringing: false,
                                            error_code: None,
                                            error_reason: None,
                                            speaker_view: false,
                                            pinned_tile_idx: 0,
                                        });
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::StartCall {
                                                partner_id: thread_id.clone(),
                                                partner_name: thread_name.clone(),
                                                partner_avatar: None,
                                                is_video: true,
                                                is_group,
                                            });
                                        }
                                    }

                                    // Voice Call Button
                                    let (aud_rect, aud_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    if aud_resp.hovered() {
                                        ui.painter().rect_filled(
                                            aud_rect,
                                            Rounding::same(8.0),
                                            pal.bg_hover,
                                        );
                                    }
                                    draw_icon_phone(
                                        ui.painter(),
                                        aud_rect.center(),
                                        17.5,
                                        if aud_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if aud_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Gọi thoại")
                                        .clicked()
                                    {
                                        self.active_call = Some(ActiveCallOverlay {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            is_video: false,
                                            is_group,
                                            state_label: "Đang kết nối cuộc gọi thoại…".into(),
                                            started_at: std::time::Instant::now(),
                                            mic_muted: false,
                                            cam_enabled: false,
                                            is_incoming_ringing: false,
                                            error_code: None,
                                            error_reason: None,
                                            speaker_view: false,
                                            pinned_tile_idx: 0,
                                        });
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::StartCall {
                                                partner_id: thread_id.clone(),
                                                partner_name: thread_name.clone(),
                                                partner_avatar: None,
                                                is_video: false,
                                                is_group,
                                            });
                                        }
                                    }
                                    // Search Messages in Conversation Button (`Ctrl+F`)
                                    let (srch_rect, srch_resp) = ui.allocate_exact_size(
                                        Vec2::new(34.0, 34.0),
                                        Sense::click(),
                                    );
                                    ui.painter().rect_filled(
                                        srch_rect,
                                        Rounding::same(8.0),
                                        if self.in_chat_search_open {
                                            pal.primary.linear_multiply(0.18)
                                        } else if srch_resp.hovered() {
                                            pal.bg_hover
                                        } else {
                                            Color32::TRANSPARENT
                                        },
                                    );
                                    draw_icon_search(
                                        ui.painter(),
                                        srch_rect.center(),
                                        16.5,
                                        if self.in_chat_search_open {
                                            pal.primary_hover
                                        } else if srch_resp.hovered() {
                                            pal.text_primary
                                        } else {
                                            pal.text_secondary
                                        },
                                    );
                                    if srch_resp
                                        .on_hover_cursor(CursorIcon::PointingHand)
                                        .on_hover_text("Tìm kiếm tin nhắn trong hội thoại này (Ctrl+F)")
                                        .clicked()
                                        || ui.input(|i| {
                                            (i.modifiers.ctrl || i.modifiers.command)
                                                && i.key_pressed(Key::F)
                                        })
                                    {
                                        self.in_chat_search_open = !self.in_chat_search_open;
                                        if self.in_chat_search_open {
                                            self.in_chat_search_focus_pending = true;
                                        } else {
                                            self.in_chat_search_query.clear();
                                        }
                                    }
                                });
                            });
                        });
                    ui.painter().hline(
                        header_frame.response.rect.x_range(),
                        header_frame.response.rect.bottom(),
                        Stroke::new(1.0_f32, pal.border),
                    );
                    // 1a. Stranger / Pending Friend Request Banner (matching Zalo PC's top relationship bar)
                    if !is_group
                        && !is_cloud_thread
                        && matches!(
                            cur_friendship,
                            crate::chat::FriendshipStatus::Stranger
                                | crate::chat::FriendshipStatus::IncomingPending
                                | crate::chat::FriendshipStatus::OutgoingPending
                        )
                    {
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .inner_margin(Margin::symmetric(18.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    match cur_friendship {
                                        crate::chat::FriendshipStatus::IncomingPending => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}  {} đã gửi lời mời kết bạn cho bạn",
                                                    lucide::USER_PLUS,
                                                    truncate_chars(&thread_name, 26)
                                                ))
                                                .size(12.2)
                                                .color(pal.primary_hover)
                                                .strong(),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.spacing_mut().item_spacing.x = 6.0;
                                                    let rej_btn = egui::Button::new(
                                                        RichText::new("Từ chối")
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                    )
                                                    .fill(pal.bg_input)
                                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                                    .rounding(Rounding::same(6.0))
                                                    .min_size(Vec2::new(68.0, 26.0));
                                                    if ui
                                                        .add(rej_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(
                                                                WorkerCmd::RejectFriend {
                                                                    user_id: thread_id.clone(),
                                                                    user_name: thread_name.clone(),
                                                                },
                                                            );
                                                        }
                                                    }

                                                    let acc_btn = egui::Button::new(
                                                        RichText::new(format!(
                                                            "{} Đồng ý kết bạn",
                                                            lucide::CHECK
                                                        ))
                                                        .size(11.8)
                                                        .color(Color32::WHITE)
                                                        .strong(),
                                                    )
                                                    .fill(pal.accent_online)
                                                    .rounding(Rounding::same(6.0))
                                                    .min_size(Vec2::new(124.0, 26.0));
                                                    if ui
                                                        .add(acc_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(
                                                                WorkerCmd::AcceptFriend {
                                                                    user_id: thread_id.clone(),
                                                                    user_name: thread_name.clone(),
                                                                },
                                                            );
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                        crate::chat::FriendshipStatus::OutgoingPending => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Đã gửi lời mời kết bạn tới {} · Đang chờ đồng ý",
                                                    truncate_chars(&thread_name, 24)
                                                ))
                                                .size(12.0)
                                                .color(pal.warning),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let undo_btn = egui::Button::new(
                                                        RichText::new("Hủy lời mời")
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                    )
                                                    .fill(pal.bg_input)
                                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                                    .rounding(Rounding::same(6.0))
                                                    .min_size(Vec2::new(86.0, 26.0));
                                                    if ui
                                                        .add(undo_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(WorkerCmd::UndoFriend {
                                                                user_id: thread_id.clone(),
                                                                user_name: thread_name.clone(),
                                                            });
                                                        }
                                                    }
                                                },
                                            );
                                        }
                                        crate::chat::FriendshipStatus::Stranger => {
                                            ui.label(
                                                RichText::new(format!(
                                                    "Bạn và {} chưa là bạn bè trên Zalo",
                                                    truncate_chars(&thread_name, 26)
                                                ))
                                                .size(12.0)
                                                .color(pal.text_secondary),
                                            );
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let add_btn = egui::Button::new(
                                                        RichText::new(format!(
                                                            "{} Kết bạn",
                                                            lucide::USER_PLUS
                                                        ))
                                                        .size(11.8)
                                                        .color(Color32::WHITE)
                                                        .strong(),
                                                    )
                                                    .fill(pal.primary)
                                                    .rounding(Rounding::same(6.0))
                                                    .min_size(Vec2::new(92.0, 26.0));
                                                    if ui
                                                        .add(add_btn)
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .clicked()
                                                    {
                                                        if let Some(tx) = WORKER_TX.get() {
                                                            let _ = tx.send(WorkerCmd::AddFriend {
                                                                user_id: thread_id.clone(),
                                                                user_name: thread_name.clone(),
                                                            });
                                                        }
                                                        self.show_toast(format!(
                                                            "Đã gửi lời mời kết bạn tới {}",
                                                            thread_name
                                                        ));
                                                    }
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                });
                            });
                    }
                    // 1b. Pinned Message Banner (if any message is pinned in this thread)
                    let pinned_msg_info = self
                        .threads
                        .get(self.selected_thread_index)
                        .and_then(|t| {
                            t.messages
                                .iter()
                                .enumerate()
                                .find(|(_, m)| m.is_pinned)
                                .map(|(idx, m)| {
                                    (
                                        idx,
                                        if m.is_outgoing {
                                            "Tôi".to_string()
                                        } else if !m.sender.is_empty() && m.sender != "0" {
                                            m.sender.clone()
                                        } else {
                                            thread_name.clone()
                                        },
                                        if m.sticker_id.is_some() {
                                            "[Nhãn dán]".to_string()
                                        } else {
                                            truncate_chars(&m.text, 80).into_owned()
                                        },
                                    )
                                })
                        });

                    let mut unpin_idx: Option<usize> = None;
                    if let Some((pidx, psender, ptext)) = pinned_msg_info {
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .stroke(Stroke::new(1.0_f32, pal.primary.linear_multiply(0.4)))
                            .inner_margin(Margin::symmetric(14.0, 7.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{} Tin ghim:", lucide::PIN))
                                            .size(12.5)
                                            .color(pal.primary)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(format!("{psender}: {ptext}"))
                                            .size(12.5)
                                            .color(pal.text_primary),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button(format!("Bỏ ghim {}", lucide::X))
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            unpin_idx = Some(pidx);
                                        }
                                    });
                                });
                            });
                    }
                    if let Some(uidx) = unpin_idx {
                        if let Some(real) = self
                            .threads
                            .get_mut(self.selected_thread_index)
                            .and_then(|t| t.messages.get_mut(uidx))
                        {
                            real.is_pinned = false;
                        }
                        self.persist_threads_async();
                        self.status_msg = "Đã bỏ ghim tin nhắn".into();
                    }

                    // 1c-0. GitHub Release Update Available Notification Banner
                    if let Some(upd) = self.update_info.clone() {
                        if upd.has_update
                            && !self.update_banner_dismissed
                            && self.settings.skipped_update_version != upd.latest_version
                        {
                            egui::Frame::none()
                                .fill(pal.accent_online.linear_multiply(0.14))
                                .stroke(Stroke::new(1.0_f32, pal.accent_online))
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let msg_txt = if self.update_downloading {
                                            let (dl, tot) = self.update_progress.unwrap_or((0, 0));
                                            if tot > 0 {
                                                let pct = ((dl as f64 / tot as f64) * 100.0)
                                                    .clamp(0.0, 100.0);
                                                format!(
                                                    "⏳ Đang tải bản cập nhật v{} từ GitHub ({pct:.0}%)…",
                                                    upd.latest_version
                                                )
                                            } else {
                                                format!(
                                                    "⏳ Đang tải bản cập nhật v{} từ GitHub…",
                                                    upd.latest_version
                                                )
                                            }
                                        } else {
                                            format!(
                                                "🚀 Đã có bản cập nhật mới ctdozlite v{} trên GitHub Releases!",
                                                upd.latest_version
                                            )
                                        };
                                        let banner_lbl = ui.label(
                                            RichText::new(msg_txt)
                                                .size(12.0)
                                                .color(pal.accent_online)
                                                .strong(),
                                        );
                                        if !upd.release_notes.trim().is_empty() {
                                            banner_lbl.on_hover_text(upd.release_notes.trim());
                                        }
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(lucide::X)
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Ẩn thanh thông báo cập nhật")
                                                    .clicked()
                                                {
                                                    self.update_banner_dismissed = true;
                                                }
                                                if ui
                                                    .small_button("Chi tiết")
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    self.update_modal_open = true;
                                                }
                                                if !self.update_downloading {
                                                    if let (Some(dl_url), Some(asset_name)) =
                                                        (upd.download_url.clone(), upd.asset_name.clone())
                                                    {
                                                        if ui
                                                            .add(
                                                                egui::Button::new(
                                                                    RichText::new(format!(
                                                                        "⬇ Cập nhật v{}",
                                                                        upd.latest_version
                                                                    ))
                                                                    .size(11.5)
                                                                    .color(Color32::WHITE)
                                                                    .strong(),
                                                                )
                                                                .fill(pal.accent_online)
                                                                .rounding(Rounding::same(5.0)),
                                                            )
                                                            .on_hover_cursor(
                                                                CursorIcon::PointingHand,
                                                            )
                                                            .clicked()
                                                        {
                                                            self.update_downloading = true;
                                                            self.update_progress =
                                                                Some((0, upd.asset_size));
                                                            self.update_modal_open = true;
                                                            if let Some(tx) = WORKER_TX.get() {
                                                                let _ = tx.send(
                                                                    WorkerCmd::DownloadAndApplyUpdate {
                                                                        download_url: dl_url,
                                                                        asset_name,
                                                                        target_version: upd
                                                                            .latest_version
                                                                            .clone(),
                                                                        expected_sha256: upd
                                                                            .expected_sha256
                                                                            .clone(),
                                                                    },
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                    }

                    // 1c. Starred filter active banner
                    if self.filter_starred_only {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(234, 179, 8).linear_multiply(0.12))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(234, 179, 8)))
                            .inner_margin(Margin::symmetric(14.0, 6.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{} Đang lọc tin nhắn đã đánh dấu sao", lucide::STAR))
                                            .size(12.0)
                                            .color(Color32::from_rgb(234, 179, 8))
                                            .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui
                                            .small_button(format!("Xem tất cả {}", lucide::X))
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            self.filter_starred_only = false;
                                        }
                                    });
                                });
                            });
                    }

                    // 1d. In-Conversation Message Search Bar (`self.in_chat_search_open`)
                    if self.in_chat_search_open {
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            self.in_chat_search_open = false;
                            self.in_chat_search_query.clear();
                        }
                    }
                    if self.in_chat_search_open {
                        let q_low = self.in_chat_search_query.trim().to_lowercase();
                        let q_folded = fold_vn_search(&q_low);
                        let match_cnt = if q_low.is_empty() {
                            0
                        } else {
                            self.threads
                                .get(self.selected_thread_index)
                                .map(|t| {
                                    t.messages
                                        .iter()
                                        .filter(|m| {
                                            matches_vn_query(&m.text, &q_low, &q_folded)
                                                || matches_vn_query(&m.sender, &q_low, &q_folded)
                                        })
                                        .count()
                                })
                                .unwrap_or(0)
                        };
                        egui::Frame::none()
                            .fill(pal.bg_sidebar)
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .inner_margin(Margin::symmetric(18.0, 8.0))
                            .show(ui, |ui| {
                                let bar_avail_w = ui.available_width();
                                ui.set_width(bar_avail_w);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    let right_reserve = if q_low.is_empty() {
                                        86.0
                                    } else {
                                        174.0
                                    };
                                    let pill_w = (bar_avail_w - right_reserve).max(140.0);
                                    egui::Frame::none()
                                        .fill(pal.bg_input)
                                        .rounding(Rounding::same(8.0))
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            pal.primary.linear_multiply(0.65),
                                        ))
                                        .inner_margin(Margin::symmetric(10.0, 5.0))
                                        .show(ui, |ui| {
                                            let inner_w = (pill_w - 20.0).max(110.0);
                                            ui.set_width(inner_w);
                                            ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 6.0;
                                                let (ir, _) = ui.allocate_exact_size(
                                                    Vec2::new(15.0, 18.0),
                                                    Sense::hover(),
                                                );
                                                draw_icon_search(
                                                    ui.painter(),
                                                    ir.center(),
                                                    13.5,
                                                    pal.primary_hover,
                                                );
                                                let has_txt =
                                                    !self.in_chat_search_query.is_empty();
                                                let te_w = if has_txt {
                                                    (inner_w - 44.0).max(60.0)
                                                } else {
                                                    (inner_w - 22.0).max(80.0)
                                                };
                                                let te_resp = ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut self.in_chat_search_query,
                                                    )
                                                    .id(Id::new((
                                                        "in_chat_search_input",
                                                        &thread_id,
                                                    )))
                                                    .frame(false)
                                                    .hint_text(
                                                        "Tìm tin nhắn trong hội thoại này...",
                                                    )
                                                    .desired_width(te_w),
                                                );
                                                if self.in_chat_search_focus_pending {
                                                    te_resp.request_focus();
                                                    self.in_chat_search_focus_pending = false;
                                                }
                                                if has_txt {
                                                    let (clr_r, clr_resp) = ui
                                                        .allocate_exact_size(
                                                            Vec2::splat(16.0),
                                                            Sense::click(),
                                                        );
                                                    if clr_resp.hovered() {
                                                        ui.painter().circle_filled(
                                                            clr_r.center(),
                                                            8.0,
                                                            pal.bg_hover,
                                                        );
                                                    }
                                                    draw_lucide_icon(
                                                        ui.painter(),
                                                        clr_r.center(),
                                                        lucide::X,
                                                        11.5,
                                                        pal.text_muted,
                                                    );
                                                    if clr_resp
                                                        .on_hover_cursor(CursorIcon::PointingHand)
                                                        .on_hover_text("Xóa từ khóa")
                                                        .clicked()
                                                    {
                                                        self.in_chat_search_query.clear();
                                                    }
                                                }
                                            });
                                        });

                                    if !q_low.is_empty() {
                                        let badge_col = if match_cnt > 0 {
                                            pal.accent_online
                                        } else {
                                            pal.danger
                                        };
                                        egui::Frame::none()
                                            .fill(badge_col.linear_multiply(0.14))
                                            .rounding(Rounding::same(6.0))
                                            .inner_margin(Margin::symmetric(8.0, 4.0))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(if match_cnt > 0 {
                                                        format!("{match_cnt} kết quả")
                                                    } else {
                                                        "0 kết quả".to_string()
                                                    })
                                                    .size(11.5)
                                                    .color(badge_col)
                                                    .strong(),
                                                );
                                            });
                                    }

                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let close_btn = egui::Button::new(
                                            RichText::new(format!("{} Đóng", lucide::X))
                                                .size(11.8)
                                                .color(pal.text_secondary)
                                                .strong(),
                                        )
                                        .fill(pal.bg_elevated)
                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                        .rounding(Rounding::same(6.0))
                                        .min_size(Vec2::new(66.0, 28.0));
                                        if ui
                                            .add(close_btn)
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Đóng thanh tìm kiếm (Esc)")
                                            .clicked()
                                        {
                                            self.in_chat_search_open = false;
                                            self.in_chat_search_query.clear();
                                        }
                                    });
                                });
                            });
                    }

                    // 2. Scrollable Messages Area
                    let contextual_stickers = self.resolve_contextual_sticker_suggestions(ctx);
                    let composer_link_preview = self.resolve_composer_link_preview(ctx);
                    // Do NOT subtract `contextual_stickers` height from `picker_reserve`!
                    // The sticker strip is a true floating overlay (`egui::Area`) above the composer bar
                    // (matching Zalo PC's `PopoverIdentitiesDefine.STICKER_SUGGESTION`), so the message scroll
                    // area and composer input box never shift or lose IME focus while typing.
                    let picker_reserve = (if self.sticker_picker_open { 268.0 } else { 0.0 })
                        + (if self.replying_to.is_some() { 48.0 } else { 0.0 })
                        + (if self.pending_clipboard_image.is_some() {
                            64.0
                        } else {
                            0.0
                        });
                    let mut pending_action: Option<(usize, BubbleAction)> = None;
                    let mut load_older_clicked = false;
                    let mut custom_toast: Option<String> = None;
                    // each frame, so a 2 000-message group re-laid out 2 000 galleys per repaint.
                    // "Tải thêm tin nhắn cũ" widens the window before asking the server.
                    if self.chat_render_window.0 != thread_id {
                        self.chat_render_window = (thread_id.clone(), CHAT_RENDER_STEP);
                    }
                    let jump_index = self.quote_jump.as_ref().and_then(|jump| {
                        self.threads
                            .get(self.selected_thread_index)
                            .and_then(|thread| jump.target_index(&thread_id, &thread.messages))
                    });
                    if jump_index.is_none() {
                        self.quote_jump = None;
                    }
                    let jump_scroll_pending = self
                        .quote_jump
                        .as_ref()
                        .map(|jump| jump.scroll_pending)
                        .unwrap_or(false);
                    let mut jump_scrolled = false;
                    if let Some(index) = jump_index {
                        let count = self
                            .threads
                            .get(self.selected_thread_index)
                            .map(|thread| thread.messages.len())
                            .unwrap_or(0);
                        self.chat_render_window.1 = crate::message_navigation::render_limit_for_target(
                            self.chat_render_window.1,
                            count,
                            index,
                        );
                    }
                    let render_limit = self.chat_render_window.1;
                    let mut widen_window = false;
                    // Group sender -> avatar and display name map, built once per frame
                    let (member_avatar_by_key, member_name_by_uid): (HashMap<String, String>, HashMap<String, String>) = if is_group {
                        let mut avas = HashMap::new();
                        let mut names = HashMap::new();
                        for th in self.threads.iter().filter(|th| !th.is_group) {
                            if let Some(ref ava) = th.avatar {
                                if !ava.is_empty() {
                                    avas.insert(th.id.clone(), ava.clone());
                                    avas.insert(th.name.clone(), ava.clone());
                                }
                            }
                            names.insert(th.id.clone(), th.name.clone());
                        }
                        let empty_vec = Vec::new();
                        let members = self.group_members_map.get(&thread_id).unwrap_or_else(|| {
                            self.threads.get(self.selected_thread_index).map(|t| &t.presentation.members).unwrap_or(&empty_vec)
                        });
                        for m in members {
                            if let Some(ref ava) = m.avatar {
                                if !ava.is_empty() {
                                    avas.insert(m.id.clone(), ava.clone());
                                    avas.insert(m.name.clone(), ava.clone());
                                }
                            }
                            if !m.name.is_empty() {
                                names.insert(m.id.clone(), m.name.clone());
                            }
                        }
                        (avas, names)
                    } else {
                        (HashMap::new(), HashMap::new())
                    };
                    let composer_lines = self
                        .input_message
                        .split('\n')
                        .count()
                        .clamp(1, 5);
                    let composer_extra_h = (composer_lines.saturating_sub(1) as f32) * 20.0;
                    let typing_name = self.typing_indicators.get(&thread_id).cloned();
                    let typing_reserve = if typing_name.is_some() { 24.0 } else { 0.0 };
                    let msg_area_height = (ui.available_height()
                        - typing_reserve
                        - 68.0
                        - composer_extra_h
                        - picker_reserve)
                        .max(100.0);
                    let chat_scroll_out = egui::ScrollArea::vertical()
                        .id_salt(Id::new(("chat_scroll", &thread_id)))
                        .max_height(msg_area_height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(jump_index.is_none())
                        .show(ui, |ui| {
                            egui::Frame::none()
                                .inner_margin(Margin {
                                    left: 20.0,
                                    right: 22.0,
                                    top: 14.0,
                                    bottom: 10.0,
                                })
                                .show(ui, |ui| {
                                    if let Some(thread) =
                                        self.threads.get(self.selected_thread_index)
                                    {
                                        if thread.messages.is_empty() {
                                            ui.vertical_centered(|ui| {
                                                ui.add_space(60.0);
                                                ui.label(
                                                    RichText::new(
                                                        "Chưa có tin nhắn trong cuộc trò chuyện này.",
                                                    )
                                                    .size(14.0)
                                                    .color(pal.text_muted),
                                                );
                                            });
                                        } else {
                                            if !self.in_chat_search_open
                                                && !self.filter_starred_only
                                            {
                                                let has_more_in_memory = thread.messages.len() > render_limit;
                                                let reached_start = self.history_reached_start.contains(&thread_id);

                                                if reached_start && !has_more_in_memory {
                                                    ui.vertical_centered(|ui| {
                                                        ui.add_space(8.0);
                                                        ui.label(
                                                            RichText::new("— Đây là tin nhắn đầu tiên của cuộc trò chuyện —")
                                                                .size(11.5)
                                                                .color(pal.text_muted),
                                                        );
                                                        ui.add_space(8.0);
                                                    });
                                                } else {
                                                    ui.vertical_centered(|ui| {
                                                        let hist_btn = egui::Button::new(
                                                            RichText::new(format!(
                                                                "{} Tải thêm tin nhắn cũ",
                                                                lucide::REFRESH
                                                            ))
                                                            .size(11.5)
                                                            .color(pal.text_secondary),
                                                        )
                                                        .fill(pal.bg_elevated)
                                                        .stroke(Stroke::new(1.0_f32, pal.border))
                                                        .rounding(Rounding::same(12.0));
                                                        if ui
                                                            .add(hist_btn)
                                                            .on_hover_cursor(CursorIcon::PointingHand)
                                                            .on_hover_text(
                                                                "Đồng bộ thêm lịch sử tin nhắn cũ từ máy chủ Zalo",
                                                            )
                                                            .clicked()
                                                        {
                                                            if has_more_in_memory {
                                                                widen_window = true;
                                                                let new_count = (render_limit + CHAT_RENDER_STEP).min(thread.messages.len());
                                                                custom_toast = Some(format!("Đã hiển thị thêm tin nhắn cũ ({new_count}/{})", thread.messages.len()));
                                                            } else {
                                                                if let Some(tx) = WORKER_TX.get() {
                                                                    let _ = tx.send(
                                                                        WorkerCmd::LoadGroupHistory {
                                                                            group_id: thread_id.clone(),
                                                                            is_group,
                                                                            have: thread.messages.len(),
                                                                        },
                                                                    );
                                                                }
                                                                load_older_clicked = true;
                                                            }
                                                        }
                                                        ui.add_space(6.0);
                                                    });
                                                }
                                            }
                                            let only_starred = self.filter_starred_only;
                                            let in_chat_q = if self.in_chat_search_open {
                                                self.in_chat_search_query.trim().to_lowercase()
                                            } else {
                                                String::new()
                                            };
                                            let in_chat_folded = fold_vn_search(&in_chat_q);
                                            let visible_indices: Vec<usize> = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .filter(|(_, m)| {
                                                    (!only_starred || m.is_starred)
                                                        && (in_chat_q.is_empty()
                                                            || matches_vn_query(
                                                                &m.text,
                                                                &in_chat_q,
                                                                &in_chat_folded,
                                                            )
                                                            || matches_vn_query(
                                                                &m.sender,
                                                                &in_chat_q,
                                                                &in_chat_folded,
                                                            ))
                                                })
                                                .map(|(i, _)| i)
                                                .collect();
                                            // Search and the starred filter still scan everything.
                                            let visible_indices: Vec<usize> =
                                                if only_starred || !in_chat_q.is_empty() {
                                                    visible_indices
                                                } else {
                                                    let skip = visible_indices
                                                        .len()
                                                        .saturating_sub(render_limit);
                                                    visible_indices[skip..].to_vec()
                                                };
                                            if visible_indices.is_empty() && !in_chat_q.is_empty() {
                                                ui.vertical_centered(|ui| {
                                                    ui.add_space(56.0);
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "Không tìm thấy tin nhắn nào khớp từ khóa \"{}\".",
                                                            self.in_chat_search_query.trim()
                                                        ))
                                                        .size(13.5)
                                                        .color(pal.text_muted),
                                                    );
                                                });
                                            }
                                            let last_outgoing_idx = thread
                                                .messages
                                                .iter()
                                                .enumerate()
                                                .rev()
                                                .find(|(_, m)| {
                                                    m.is_outgoing && !is_system_notice_message(m)
                                                })
                                                .map(|(i, _)| i);

                                            for (pos, &idx) in visible_indices.iter().enumerate() {
                                                let msg = &thread.messages[idx];
                                                let prev_msg = pos
                                                    .checked_sub(1)
                                                    .and_then(|p| visible_indices.get(p))
                                                    .map(|&pi| &thread.messages[pi]);
                                                let next_msg = visible_indices
                                                    .get(pos + 1)
                                                    .map(|&ni| &thread.messages[ni]);

                                                if should_show_time_divider(prev_msg, msg) {
                                                    if pos > 0 {
                                                        ui.add_space(6.0);
                                                    }
                                                    render_date_divider(ui, prev_msg, msg, pal);
                                                    ui.add_space(6.0);
                                                }

                                                let row = ui.scope(|ui| {
                                                    if is_system_notice_message(msg) {
                                                        render_system_notice_pill(ui, msg, pal);
                                                        ui.add_space(6.0);
                                                        return;
                                                    }

                                                    let prev_same_sender = prev_msg
                                                        .map(|pm| same_message_cluster(pm, msg))
                                                        .unwrap_or(false);
                                                    let next_same_sender = next_msg
                                                        .map(|nm| same_message_cluster(msg, nm))
                                                        .unwrap_or(false);
                                                    let is_latest_outgoing =
                                                        last_outgoing_idx == Some(idx);

                                                    let s_id = if is_group {
                                                        crate::chat::sidebar_sender_id(msg).or_else(|| {
                                                            if msg.sender.chars().all(|c| c.is_ascii_digit()) {
                                                                Some(msg.sender.clone())
                                                            } else {
                                                                None
                                                            }
                                                        })
                                                    } else {
                                                        None
                                                    };
                                                    let sender_ava_opt = if !is_group {
                                                        thread_avatar.as_deref()
                                                    } else {
                                                        s_id.as_deref()
                                                            .and_then(|id| member_avatar_by_key.get(id))
                                                            .or_else(|| member_avatar_by_key.get(msg.sender.as_str()))
                                                            .map(|s| s.as_str())
                                                    };
                                                    let display_sender_fallback = if is_group {
                                                        s_id.as_deref()
                                                            .and_then(|id| member_name_by_uid.get(id))
                                                            .map(|s| s.as_str())
                                                            .unwrap_or(&thread_name)
                                                    } else {
                                                        &thread_name
                                                    };
                                                    if let Some(action) = render_chat_bubble(
                                                        ui,
                                                        msg,
                                                        prev_same_sender,
                                                        next_same_sender,
                                                        is_latest_outgoing,
                                                        is_group,
                                                        display_sender_fallback,
                                                        sender_ava_opt,
                                                        pal,
                                                        &self.sticker_textures,
                                                        &mut self.loading_stickers,
                                                        &self.emoji_textures,
                                                        &self.downloads,
                                                        idx,
                                                    ) {
                                                        pending_action = Some((idx, action));
                                                    }
                                                    ui.add_space(if next_same_sender {
                                                        2.5
                                                    } else {
                                                        9.0
                                                    });
                                                });
                                                if jump_index == Some(idx) {
                                                    render_quote_target(
                                                        ui,
                                                        row.response.rect,
                                                        jump_scroll_pending,
                                                        pal.primary,
                                                    );
                                                    jump_scrolled |= jump_scroll_pending;
                                                }
                                            }
                                            let (bottom_rect, _) = ui.allocate_exact_size(Vec2::new(1.0, 1.0), Sense::hover());
                                            if self.force_scroll_to_bottom {
                                                ui.scroll_to_rect(bottom_rect, Some(Align::BOTTOM));
                                                self.force_scroll_to_bottom = false;
                                            }
                                        }
                                    }
                                });
                        });

                    let dist_from_bottom = (chat_scroll_out.content_size.y
                        - (chat_scroll_out.state.offset.y + chat_scroll_out.inner_rect.height()))
                        .max(0.0);
                    if dist_from_bottom > 180.0 {
                        let btn_size = 36.0;
                        let btn_pos = Pos2::new(
                            chat_scroll_out.inner_rect.right() - btn_size - 22.0,
                            chat_scroll_out.inner_rect.bottom() - btn_size - 16.0,
                        );
                        egui::Area::new(Id::new(("auto_scroll_btn", &thread_id)))
                            .order(egui::Order::Middle)
                            .fixed_pos(btn_pos)
                            .show(ctx, |ui| {
                                egui::Frame::none()
                                    .fill(pal.bg_elevated)
                                    .stroke(Stroke::new(1.0_f32, pal.border))
                                    .rounding(Rounding::same(btn_size / 2.0))
                                    .shadow(egui::epaint::Shadow {
                                        offset: Vec2::new(0.0, 2.0),
                                        blur: 8.0,
                                        spread: 0.0,
                                        color: Color32::from_black_alpha(if pal.is_dark { 80 } else { 35 }),
                                    })
                                    .show(ui, |ui| {
                                        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(btn_size), Sense::click());
                                        if resp.hovered() {
                                            ui.painter().circle_filled(rect.center(), btn_size / 2.0, pal.bg_hover);
                                        }
                                        draw_lucide_icon(
                                            ui.painter(),
                                            rect.center(),
                                            lucide::CHEVRON_DOWN,
                                            16.0,
                                            if resp.hovered() { pal.primary_hover } else { pal.text_primary },
                                        );
                                        if resp
                                            .on_hover_cursor(CursorIcon::PointingHand)
                                            .on_hover_text("Cuộn xuống tin nhắn mới nhất")
                                            .clicked()
                                        {
                                            self.force_scroll_to_bottom = true;
                                            ui.ctx().request_repaint();
                                        }
                                    });
                            });
                    }
                    if jump_scrolled {
                        if let Some(jump) = &mut self.quote_jump {
                            jump.scroll_pending = false;
                        }
                    }
                    if widen_window {
                        self.chat_render_window.1 += CHAT_RENDER_STEP;
                    }
                    if let Some(msg) = custom_toast {
                        self.show_toast(msg);
                    } else if load_older_clicked {
                        self.show_toast("Đang kiểm tra tin nhắn cũ từ máy chủ Zalo…");
                    }
                    if let Some((idx, action)) = pending_action {
                        let target = self
                            .threads
                            .get(self.selected_thread_index)
                            .and_then(|t| t.messages.get(idx))
                            .cloned();
                        if let Some(m) = target {
                            match action {
                                BubbleAction::Download {
                                    url,
                                    suggested_name,
                                } => {
                                    self.start_attachment_download(
                                        url,
                                        suggested_name,
                                        thread_id.clone(),
                                    );
                                }
                                BubbleAction::DownloadAndOpen {
                                    url,
                                    suggested_name,
                                } => {
                                    self.download_and_open(url, suggested_name, thread_id.clone());
                                }
                                BubbleAction::OpenVideo { url, title } => {
                                    self.open_video(url, title);
                                }
                                BubbleAction::OpenDownloaded(path) => {
                                    self.open_downloaded_path(&path);
                                }
                                BubbleAction::RevealDownloaded(path) => {
                                    self.reveal_downloaded_path(&path);
                                }
                                BubbleAction::Reply => self.replying_to = Some(m),
                                BubbleAction::JumpToQuoted(reference) => {
                                    let index = self
                                        .threads
                                        .get(self.selected_thread_index)
                                        .and_then(|thread| {
                                            crate::message_navigation::unique_reference_index(&thread.messages, &reference)
                                        });
                                    if index.is_some() {
                                        self.filter_starred_only = false;
                                        self.in_chat_search_open = false;
                                        self.in_chat_search_query.clear();
                                        self.quote_jump = Some(QuoteJump {
                                            thread_id: thread_id.clone(),
                                            reference,
                                            highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
                                            scroll_pending: true,
                                        });
                                        ctx.request_repaint();
                                    } else {
                                        self.show_toast("Chưa có tin nhắn gốc đã tải với ID duy nhất. Hãy tải thêm lịch sử rồi thử lại.");
                                    }
                                }
                                BubbleAction::Forward => {
                                    self.forward_msg = Some(m);
                                    self.forward_filter.clear();
                                }
                                BubbleAction::React(r) => {
                                    self.status_msg = format!("Đã gửi cảm xúc {r}");
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::React {
                                            to_id: thread_id.clone(),
                                            is_group,
                                            msg: m,
                                            reaction: r.to_string(),
                                        });
                                    }
                                }
                                BubbleAction::TogglePin | BubbleAction::ToggleStar => {
                                    if let Some(real) = self
                                        .threads
                                        .get_mut(self.selected_thread_index)
                                        .and_then(|t| t.messages.get_mut(idx))
                                    {
                                        if action == BubbleAction::TogglePin {
                                            real.is_pinned = !real.is_pinned;
                                            self.status_msg = if real.is_pinned {
                                                "Đã ghim tin nhắn lên đầu hội thoại".into()
                                            } else {
                                                "Đã bỏ ghim tin nhắn".into()
                                            };
                                        } else {
                                            real.is_starred = !real.is_starred;
                                            self.status_msg = if real.is_starred {
                                                "Đã đánh dấu sao tin nhắn".into()
                                            } else {
                                                "Đã bỏ đánh dấu sao".into()
                                            };
                                        }
                                    }
                                    self.persist_threads_async();
                                }
                                BubbleAction::CallAgain { is_video } => {
                                    self.active_call = Some(ActiveCallOverlay {
                                        partner_id: thread_id.clone(),
                                        partner_name: thread_name.clone(),
                                        is_video,
                                        is_group,
                                        state_label: if is_video {
                                            "Đang kết nối cuộc gọi video…".into()
                                        } else {
                                            "Đang kết nối cuộc gọi thoại…".into()
                                        },
                                        started_at: std::time::Instant::now(),
                                        mic_muted: false,
                                        cam_enabled: is_video,
                                        is_incoming_ringing: false,
                                        error_code: None,
                                        error_reason: None,
                                        speaker_view: false,
                                        pinned_tile_idx: 0,
                                    });
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::StartCall {
                                            partner_id: thread_id.clone(),
                                            partner_name: thread_name.clone(),
                                            partner_avatar: None,
                                            is_video,
                                            is_group,
                                        });
                                    }
                                }
                                BubbleAction::DeleteForMe => {
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::DeleteForMe {
                                            to_id: thread_id.clone(),
                                            is_group,
                                            msg: m,
                                        });
                                    }
                                }
                                BubbleAction::RecallForEveryone => {
                                    if let Some(real) = self
                                        .threads
                                        .get_mut(self.selected_thread_index)
                                        .and_then(|t| t.messages.get_mut(idx))
                                    {
                                        if !real.text.starts_with("[Đã thu hồi]") {
                                            if real.text.trim().is_empty() {
                                                real.text = "[Tin nhắn đã thu hồi]".into();
                                            } else {
                                                real.text = format!("[Đã thu hồi] {}", real.text);
                                            }
                                        }
                                    }
                                    self.persist_threads_async();
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::RecallMessage {
                                            to_id: thread_id.clone(),
                                            is_group,
                                            msg: m,
                                        });
                                    }
                                }
                                BubbleAction::OpenImagePopup {
                                    tex_key,
                                    full_url,
                                    title,
                                    sender,
                                    time,
                                } => {
                                    let hd_tex_key = self.request_hd_texture(&full_url);
                                    self.image_viewer = Some(ImageViewerModal {
                                        tex_key,
                                        hd_tex_key,
                                        full_url,
                                        title,
                                        sender,
                                        time,
                                        zoom: 1.0,
                                        pan: Vec2::ZERO,
                                    });
                                }
                                BubbleAction::DownloadFile { url, file_name } => {
                                    self.show_toast(format!(
                                        "⏳ Đang tải tập tin \"{file_name}\"…"
                                    ));
                                    if let Some(tx) = WORKER_TX.get() {
                                        let _ = tx.send(WorkerCmd::DownloadFile {
                                            url,
                                            file_name,
                                            download_dir: self.settings.download_dir.clone(),
                                        });
                                    }
                                }
                            }
                        }
                    }

                    // 2b. Typing indicator ("••• Anh Minh đang nhập")
                    if let Some(name) = typing_name.as_deref() {
                        let (row, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), typing_reserve),
                            Sense::hover(),
                        );
                        let t = ui.input(|i| i.time) as f32;
                        let base = Pos2::new(row.left() + 66.0, row.center().y);
                        for k in 0..3 {
                            let phase = (t * 4.0 - k as f32 * 0.7).sin() * 0.5 + 0.5;
                            ui.painter().circle_filled(
                                Pos2::new(base.x + k as f32 * 7.0, base.y),
                                2.6,
                                pal.text_muted.linear_multiply(0.35 + 0.65 * phase),
                            );
                        }
                        ui.painter().text(
                            Pos2::new(base.x + 24.0, base.y),
                            egui::Align2::LEFT_CENTER,
                            format!("{name} đang nhập"),
                            FontId::proportional(12.5),
                            pal.text_muted,
                        );
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
                    }

                    // 3. Optional Sticker Picker Drawer
                    if self.sticker_picker_open {
                        self.render_sticker_picker(ui, &thread_id, is_group, pal);
                    }

                    // 3b. Reply banner (quoted message preview above the composer)
                    if self.replying_to.is_some() {
                        let mut cancel = false;
                        if let Some(q) = &self.replying_to {
                            let who = if q.is_outgoing {
                                "chính bạn".to_string()
                            } else if !q.sender.is_empty() && q.sender != "0" {
                                q.sender.clone()
                            } else {
                                thread_name.clone()
                            };
                            egui::Frame::none()
                                .fill(pal.bg_elevated)
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .stroke(Stroke::new(1.0_f32, pal.border))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(format!("Đang trả lời {who}"))
                                                    .size(12.0)
                                                    .color(pal.primary)
                                                    .strong(),
                                            );
                                            let preview = if q.sticker_id.is_some() {
                                                "[Nhãn dán]".to_string()
                                            } else {
                                                truncate_chars(&q.text, 100).into_owned()
                                            };
                                            ui.label(
                                                RichText::new(preview)
                                                    .size(12.0)
                                                    .color(pal.text_muted),
                                            );
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(lucide::X)
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Hủy trả lời")
                                                    .clicked()
                                                {
                                                    cancel = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                        if cancel {
                            self.replying_to = None;
                        }
                    }
                    // 3b-2. Live Typing Indicator Banner
                    if self.settings.show_typing_indicator {
                        if let Some((typing_who, until)) = self.typing_map.get(&thread_id) {
                            if *until > std::time::Instant::now() {
                                egui::Frame::none()
                                    .fill(pal.bg_elevated)
                                    .inner_margin(Margin::symmetric(14.0, 5.0))
                                    .rounding(Rounding::same(8.0))
                                    .stroke(Stroke::new(1.0_f32, pal.primary.linear_multiply(0.4)))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("✍  {typing_who} đang soạn thảo…"))
                                                    .size(12.5)
                                                    .italics()
                                                    .color(pal.primary),
                                            );
                                        });
                                    });
                                ui.add_space(3.0);
                                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                            }
                        }
                    }
                    // 3c. Clipboard Image Preview Banner (`Ctrl+V` / `Shift+Insert` screenshot or image file paste)
                    if is_clipboard_paste_triggered(ctx) {
                        if let Some((path, w, h, col_img)) = read_clipboard_image_to_png() {
                            // Consume any text Paste event on this frame so the composer doesn't paste a file path
                            ctx.input_mut(|i| {
                                i.events.retain(|e| !matches!(e, egui::Event::Paste(_)));
                            });
                            let tex = ctx.load_texture(
                                format!("clip_preview_{}", crate::login::unix_now()),
                                col_img,
                                TextureOptions::LINEAR,
                            );
                            self.pending_clipboard_image = Some((path, w, h, tex));
                            self.composer_focus_pending = true;
                            self.show_toast(format!(
                                "Đã dán ảnh ({w}×{h} px) từ Clipboard — nhấn Enter để gửi"
                            ));
                        }
                    }
                    if self.pending_clipboard_image.is_some() {
                        let mut clear_clip = false;
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            clear_clip = true;
                        }
                        if let Some((ref path, w, h, ref tex)) = self.pending_clipboard_image {
                            egui::Frame::none()
                                .fill(pal.bg_elevated)
                                .inner_margin(Margin::symmetric(14.0, 6.0))
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    pal.primary.linear_multiply(0.55),
                                ))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Image::new(tex)
                                                .fit_to_exact_size(Vec2::new(48.0, 48.0))
                                                .rounding(Rounding::same(6.0)),
                                        );
                                        ui.add_space(6.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} Ảnh chụp từ bộ nhớ tạm (Ctrl+V)",
                                                    lucide::IMAGE
                                                ))
                                                .size(12.2)
                                                .color(pal.primary)
                                                .strong(),
                                            );
                                            let fname = path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("clipboard.png");
                                            ui.label(
                                                RichText::new(format!(
                                                    "{fname} · {w}×{h} px — Nhập chú thích bên dưới rồi nhấn Enter để gửi"
                                                ))
                                                .size(11.3)
                                                .color(pal.text_secondary),
                                            );
                                        });
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button(format!("{} Hủy", lucide::X))
                                                    .on_hover_cursor(CursorIcon::PointingHand)
                                                    .on_hover_text("Hủy ảnh dán (Esc)")
                                                    .clicked()
                                                {
                                                    clear_clip = true;
                                                }
                                            },
                                        );
                                    });
                                });
                        }
                        if clear_clip {
                            self.pending_clipboard_image = None;
                        }
                    }

                    // 3d. Pending Scheduled Messages Banner (#38)
                    let thread_sched: Vec<crate::scheduler::ScheduledMessage> = self
                        .scheduled_items
                        .iter()
                        .filter(|s| s.thread_id == thread_id)
                        .cloned()
                        .collect();
                    if let Some(next_sched) = thread_sched.first() {
                        let mut open_sched_mgr = false;
                        let mut cancel_sched_id: Option<i64> = None;
                        egui::Frame::none()
                            .fill(pal.bg_elevated)
                            .inner_margin(Margin::symmetric(14.0, 5.0))
                            .stroke(Stroke::new(1.0_f32, pal.border))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "🕒 Hẹn giờ gửi ({}) · Tiếp theo lúc {}: \"{}\"",
                                            thread_sched.len(),
                                            crate::scheduler::format_schedule_time(
                                                next_sched.deliver_at
                                            ),
                                            truncate_chars(&next_sched.text, 36)
                                        ))
                                        .size(11.6)
                                        .color(pal.primary_hover)
                                        .strong(),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.small_button("Hủy").clicked() {
                                            cancel_sched_id = Some(next_sched.id);
                                        }
                                        if ui.small_button("Quản lý").clicked() {
                                            open_sched_mgr = true;
                                        }
                                    });
                                });
                            });
                        if let Some(cid) = cancel_sched_id {
                            let _ =
                                crate::scheduler::cancel_scheduled_message("zalo_history.db", cid);
                            self.refresh_scheduled_items();
                            self.show_toast("Đã hủy tin nhắn hẹn giờ");
                        }
                        if open_sched_mgr {
                            self.schedule_modal_open = true;
                        }
                    }

                    // 4. Bottom Message Composer Bar (Compact Icon Actions + Spacious Input + Padded Send Button)
                    let composer_frame = egui::Frame::none()
                        .fill(if pal.is_dark {
                            Color32::from_rgba_unmultiplied(33, 37, 43, 190)
                        } else {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 96)
                        })
                        .inner_margin(Margin::symmetric(16.0, 11.0))
                        .show(ui, |ui| {
                            // Handle drag-and-drop files onto the chat window
                            let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|i| {
                                i.raw
                                    .dropped_files
                                    .iter()
                                    .filter_map(|df| df.path.clone())
                                    .collect()
                            });
                            for dpath in dropped {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                        to_id: thread_id.clone(),
                                        is_group,
                                        images_only: false,
                                        caption: String::new(),
                                        preset_path: Some(dpath),
                                        original_hd: self.send_hd_original,
                                    });
                                }
                            }

                            let is_ime_busy = self.ime_active || self.ime_just_committed;
                            let trigger_screenshot_hotkey = ui.input(|i| {
                                (i.modifiers.alt && i.key_pressed(Key::A))
                                    || ((i.modifiers.command || i.modifiers.ctrl) && i.modifiers.shift && i.key_pressed(Key::A))
                                    || ((i.modifiers.command || i.modifiers.ctrl) && i.modifiers.shift && i.key_pressed(Key::X))
                            });
                            if trigger_screenshot_hotkey {
                                if let Some(tx) = WORKER_TX.get() {
                                    let _ = tx.send(WorkerCmd::TriggerScreenshot);
                                    self.show_toast("Đang bật chế độ chụp màn hình… Kéo chuột để chọn vùng cần chụp");
                                }
                            }
                            let is_cmd_enter_mode = self.settings.send_message_key == "cmd_enter";
                            let (send_via_enter, insert_newline_via_enter) = ui.input(|i| {
                                let ent = i.key_pressed(Key::Enter) && !is_ime_busy;
                                let cmd_or_ctrl = i.modifiers.command || i.modifiers.ctrl;
                                if is_cmd_enter_mode {
                                    (ent && cmd_or_ctrl, ent && !cmd_or_ctrl)
                                } else {
                                    (ent && !i.modifiers.shift, ent && i.modifiers.shift)
                                }
                            });
                            if send_via_enter || insert_newline_via_enter {
                                ui.input_mut(|i| {
                                    i.events.retain(|ev| {
                                        !matches!(
                                            ev,
                                            egui::Event::Key {
                                                key: Key::Enter,
                                                pressed: true,
                                                ..
                                            }
                                        ) && !matches!(
                                            ev,
                                            egui::Event::Text(t)
                                                if t == "\n" || t == "\r" || t == "\r\n"
                                        )
                                    });
                                });
                            }

                            egui::Frame::none()
                                .show(ui, |ui| {
                                    let mut send_clicked = false;
                                    let mut send_selected_sticker: Option<
                                        crate::chat::StickerItem,
                                    > = None;
                                    let composer_id =
                                        Id::new(("chat_composer_input", &thread_id));

                                    let response = ui
                                        .horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;

                                            // 1) Compact Sticker Icon Button
                                            let (stk_rect, stk_resp) = ui.allocate_exact_size(
                                                Vec2::new(34.0, 34.0),
                                                Sense::click(),
                                            );
                                            if self.sticker_picker_open || stk_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    stk_rect,
                                                    Rounding::same(8.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            draw_icon_sticker(
                                                ui.painter(),
                                                stk_rect.center(),
                                                18.0,
                                                if self.sticker_picker_open || stk_resp.hovered() {
                                                    Color32::from_rgb(251, 191, 36)
                                                } else {
                                                    pal.text_secondary
                                                },
                                            );
                                            if stk_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Nhãn dán (Sticker)")
                                                .clicked()
                                            {
                                                self.sticker_picker_open =
                                                    !self.sticker_picker_open;
                                            }

                                            // 2) Compact Photo Upload Icon Button
                                            let (img_rect, img_resp) = ui.allocate_exact_size(
                                                Vec2::new(34.0, 34.0),
                                                Sense::click(),
                                            );
                                            if img_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    img_rect,
                                                    Rounding::same(8.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            draw_icon_image(
                                                ui.painter(),
                                                img_rect.center(),
                                                18.0,
                                                if img_resp.hovered() {
                                                    pal.accent_online
                                                } else {
                                                    pal.text_secondary
                                                },
                                            );
                                            if img_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Gửi hình ảnh")
                                                .clicked()
                                            {
                                                let cap = self.input_message.trim().to_string();
                                                self.input_message.clear();
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ =
                                                        tx.send(WorkerCmd::PickAndSendAttachment {
                                                            to_id: thread_id.clone(),
                                                            is_group,
                                                            images_only: true,
                                                            caption: cap,
                                                            preset_path: None,
                                                            original_hd: self.send_hd_original,
                                                        });
                                                }
                                            }

                                            // HD Original Quality Toggle Button
                                            let hd_active = self.send_hd_original;
                                            let (hd_rect, hd_resp) = ui.allocate_exact_size(
                                                Vec2::new(32.0, 24.0),
                                                Sense::click(),
                                            );
                                            let hd_bg = if hd_active {
                                                if hd_resp.hovered() {
                                                    pal.primary_hover
                                                } else {
                                                    pal.primary
                                                }
                                            } else if hd_resp.hovered() {
                                                pal.bg_hover
                                            } else {
                                                pal.bg_input
                                            };
                                            let hd_text_col = if hd_active {
                                                Color32::WHITE
                                            } else {
                                                pal.text_secondary
                                            };
                                            ui.painter().rect_filled(
                                                hd_rect,
                                                Rounding::same(6.0),
                                                hd_bg,
                                            );
                                            if !hd_active {
                                                ui.painter().rect_stroke(
                                                    hd_rect,
                                                    Rounding::same(6.0),
                                                    Stroke::new(1.0_f32, pal.border),
                                                );
                                            }
                                            ui.painter().text(
                                                hd_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "HD",
                                                FontId::proportional(11.0),
                                                hd_text_col,
                                            );
                                            let hd_tooltip = if hd_active {
                                                "Chế độ gửi ảnh gốc Original / HD: ĐANG BẬT (Ảnh không bị nén, giữ nguyên 100% chất lượng)"
                                            } else {
                                                "Chế độ gửi ảnh gốc Original / HD: Đang TẮT (Bấm để bật gửi ảnh chất lượng cao không nén)"
                                            };
                                            if hd_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text(hd_tooltip)
                                                .clicked()
                                            {
                                                self.send_hd_original = !self.send_hd_original;
                                                let msg = if self.send_hd_original {
                                                    "Đã BẬT gửi ảnh gốc Original / HD (không nén)"
                                                } else {
                                                    "Đã TẮT gửi ảnh gốc Original (chế độ thường)"
                                                };
                                                self.show_toast(msg.to_string());
                                            }

                                            // 3) Compact File Upload Icon Button
                                            let (att_rect, att_resp) = ui.allocate_exact_size(
                                                Vec2::new(34.0, 34.0),
                                                Sense::click(),
                                            );
                                            if att_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    att_rect,
                                                    Rounding::same(8.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            draw_icon_attachment(
                                                ui.painter(),
                                                att_rect.center(),
                                                17.0,
                                                if att_resp.hovered() {
                                                    pal.primary_hover
                                                } else {
                                                    pal.text_secondary
                                                },
                                            );
                                            if att_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Đính kèm File / Video")
                                                .clicked()
                                            {
                                                let cap = self.input_message.trim().to_string();
                                                self.input_message.clear();
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ =
                                                        tx.send(WorkerCmd::PickAndSendAttachment {
                                                            to_id: thread_id.clone(),
                                                            is_group,
                                                            images_only: false,
                                                            caption: cap,
                                                            preset_path: None,
                                                            original_hd: self.send_hd_original,
                                                        });
                                                }
                                            }

                                            // 4) Screenshot Icon Button (Chụp ảnh màn hình)
                                            let (cam_rect, cam_resp) = ui.allocate_exact_size(
                                                Vec2::new(34.0, 34.0),
                                                Sense::click(),
                                            );
                                            if cam_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    cam_rect,
                                                    Rounding::same(8.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            draw_icon_camera(
                                                ui.painter(),
                                                cam_rect.center(),
                                                18.0,
                                                if cam_resp.hovered() {
                                                    pal.primary_hover
                                                } else {
                                                    pal.text_secondary
                                                },
                                            );
                                            if cam_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Chụp ảnh màn hình (Alt+A hoặc click)")
                                                .clicked()
                                            {
                                                if let Some(tx) = WORKER_TX.get() {
                                                    let _ = tx.send(WorkerCmd::TriggerScreenshot);
                                                    self.show_toast("Đang bật chế độ chụp màn hình… Kéo chuột để chọn vùng cần chụp");
                                                }
                                            }

                                            let is_multiline = self.input_message.contains('\n');
                                            let vis_lines = if is_multiline {
                                                self.input_message
                                                    .split('\n')
                                                    .count()
                                                    .clamp(2, 5)
                                            } else {
                                                1
                                            };
                                            let box_h = if is_multiline {
                                                14.0 + (vis_lines as f32 * 20.0)
                                            } else {
                                                34.0
                                            };

                                            // Vertical hairline separator
                                            let (sep_r, _) = ui.allocate_exact_size(
                                                Vec2::new(8.0, (box_h - 10.0).clamp(20.0, 72.0)),
                                                Sense::hover(),
                                            );
                                            ui.painter().line_segment(
                                                [sep_r.center_top(), sep_r.center_bottom()],
                                                Stroke::new(1.0_f32, pal.border),
                                            );

                                            let send_btn_w =
                                                if is_narrow_w { 52.0 } else { 76.0 };
                                            let sched_btn_w = 34.0;
                                            let input_w = (ui.available_width()
                                                - send_btn_w
                                                - sched_btn_w
                                                - 16.0)
                                                .max(50.0);
                                            let hint_str = format!(
                                                "Nhập tin nhắn tới {}...",
                                                truncate_chars(&thread_name, 18)
                                            );
                                            let resp = if is_multiline {
                                                ui.allocate_ui_with_layout(
                                                    Vec2::new(input_w, box_h),
                                                    Layout::top_down(Align::Min),
                                                    |ui| {
                                                        ui.set_min_size(Vec2::new(input_w, box_h));
                                                        ui.set_max_size(Vec2::new(input_w, box_h));
                                                        egui::ScrollArea::vertical()
                                                            .id_salt((
                                                                "composer_fixed_scroll",
                                                                &thread_id,
                                                            ))
                                                            .max_width(input_w)
                                                            .max_height(box_h)
                                                            .auto_shrink([false, false])
                                                            .stick_to_bottom(true)
                                                            .show(ui, |ui| {
                                                                ui.add(
                                                                    egui::TextEdit::multiline(
                                                                        &mut self.input_message,
                                                                    )
                                                                    .id(composer_id)
                                                                    .desired_rows(vis_lines)
                                                                    .frame(false)
                                                                    .hint_text(hint_str)
                                                                    .margin(Margin::symmetric(
                                                                        6.0, 6.0,
                                                                    ))
                                                                    .desired_width(input_w - 10.0),
                                                                )
                                                            })
                                                            .inner
                                                    },
                                                )
                                                .inner
                                            } else {
                                                ui.add_sized(
                                                    [input_w, 34.0],
                                                    egui::TextEdit::singleline(
                                                        &mut self.input_message,
                                                    )
                                                    .id(composer_id)
                                                    .frame(false)
                                                    .hint_text(hint_str)
                                                    .margin(Margin::symmetric(6.0, 7.0))
                                                    .desired_width(input_w),
                                                )
                                            };
                                            if resp.changed()
                                                && !self.input_message.trim().is_empty()
                                                && !self.settings.block_typing
                                            {
                                                let should_emit = self
                                                    .last_typing_sent_at
                                                    .as_ref()
                                                    .map(|(last_tid, t)| {
                                                        last_tid != &thread_id
                                                            || t.elapsed()
                                                                >= std::time::Duration::from_millis(
                                                                    3200,
                                                                )
                                                    })
                                                    .unwrap_or(true);
                                                if should_emit {
                                                    self.last_typing_sent_at = Some((
                                                        thread_id.clone(),
                                                        std::time::Instant::now(),
                                                    ));
                                                    if let Some(tx) = WORKER_TX.get() {
                                                        let _ = tx.send(WorkerCmd::SendTyping {
                                                            thread_id: thread_id.clone(),
                                                            is_group,
                                                        });
                                                    }
                                                }
                                            }

                                            // Schedule Send Button (`🕒`)
                                            let (sched_rect, sched_resp) = ui.allocate_exact_size(
                                                Vec2::new(32.0, 34.0),
                                                Sense::click(),
                                            );
                                            if self.schedule_modal_open || sched_resp.hovered() {
                                                ui.painter().rect_filled(
                                                    sched_rect,
                                                    Rounding::same(8.0),
                                                    pal.bg_hover,
                                                );
                                            }
                                            ui.painter().text(
                                                sched_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "🕒",
                                                FontId::proportional(14.5),
                                                if self.schedule_modal_open || sched_resp.hovered()
                                                {
                                                    pal.primary_hover
                                                } else {
                                                    pal.text_secondary
                                                },
                                            );
                                            if sched_resp
                                                .on_hover_cursor(CursorIcon::PointingHand)
                                                .on_hover_text("Hẹn giờ gửi tin nhắn & Nhắc hẹn")
                                                .clicked()
                                            {
                                                self.schedule_modal_open =
                                                    !self.schedule_modal_open;
                                            }

                                            // Send Button
                                            let can_send = !self.input_message.trim().is_empty()
                                                || self.pending_clipboard_image.is_some();
                                            let (send_rect, send_resp) = ui.allocate_exact_size(
                                                Vec2::new(send_btn_w, 34.0),
                                                Sense::click(),
                                            );
                                            if send_resp.secondary_clicked() {
                                                self.schedule_modal_open = true;
                                            }
                                            let send_bg = if !can_send {
                                                pal.primary.linear_multiply(0.42)
                                            } else if send_resp.hovered() {
                                                pal.primary_hover
                                            } else {
                                                pal.primary
                                            };
                                            ui.painter().rect_filled(
                                                send_rect,
                                                Rounding::same(8.0),
                                                send_bg,
                                            );
                                            if !is_narrow_w {
                                                ui.painter().text(
                                                    Pos2::new(
                                                        send_rect.left() + 15.0,
                                                        send_rect.center().y,
                                                    ),
                                                    egui::Align2::LEFT_CENTER,
                                                    "Gửi",
                                                    FontId::proportional(13.0),
                                                    Color32::WHITE,
                                                );
                                                draw_icon_send(
                                                    ui.painter(),
                                                    Pos2::new(
                                                        send_rect.right() - 17.0,
                                                        send_rect.center().y,
                                                    ),
                                                    14.0,
                                                    Color32::WHITE,
                                                );
                                            } else {
                                                draw_icon_send(
                                                    ui.painter(),
                                                    send_rect.center(),
                                                    16.0,
                                                    Color32::WHITE,
                                                );
                                            }
                                            send_clicked = send_resp
                                                .on_hover_cursor(if can_send {
                                                    CursorIcon::PointingHand
                                                } else {
                                                    CursorIcon::Default
                                                })
                                                .clicked();
                                            resp
                                        })
                                        .inner;

                                    if is_ime_busy && response.lost_focus() {
                                        response.request_focus();
                                    }
                                    if insert_newline_via_enter
                                        && (response.has_focus() || response.lost_focus())
                                    {
                                        if let Some(mut st) =
                                            egui::TextEdit::load_state(ui.ctx(), composer_id)
                                        {
                                            let char_idx = st
                                                .cursor
                                                .char_range()
                                                .map(|r| r.primary.index)
                                                .unwrap_or_else(|| {
                                                    self.input_message.chars().count()
                                                });
                                            let byte_idx = self
                                                .input_message
                                                .char_indices()
                                                .nth(char_idx)
                                                .map(|(i, _)| i)
                                                .unwrap_or(self.input_message.len());
                                            self.input_message.insert(byte_idx, '\n');
                                            let next_c = egui::text::CCursor::new(char_idx + 1);
                                            st.cursor.set_char_range(Some(
                                                egui::text::CCursorRange::one(next_c),
                                            ));
                                            st.store(ui.ctx(), composer_id);
                                        } else {
                                            self.input_message.push('\n');
                                        }
                                        response.request_focus();
                                    }
                                    if response.changed() {
                                        ensure_cjk_font(
                                            ui.ctx(),
                                            std::iter::once(self.input_message.as_str()),
                                        );
                                        if !self.input_message.trim().is_empty()
                                            && self.last_typing_ping_at.elapsed()
                                                > std::time::Duration::from_secs(3)
                                        {
                                            self.last_typing_ping_at = std::time::Instant::now();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::SendTyping {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                });
                                            }
                                        }
                                    }
                                    if self.composer_focus_pending {
                                        response.request_focus();
                                        self.composer_focus_pending = false;
                                    }
                                    response.context_menu(|ui| {
                                        if ui
                                            .button(format!(
                                                "{} Dán hình ảnh từ Clipboard (Ctrl+V)",
                                                lucide::IMAGE
                                            ))
                                            .clicked()
                                        {
                                            ui.close_menu();
                                            if let Some((path, w, h, col_img)) =
                                                read_clipboard_image_to_png()
                                            {
                                                let tex = ctx.load_texture(
                                                    format!(
                                                        "clip_preview_{}",
                                                        crate::login::unix_now()
                                                    ),
                                                    col_img,
                                                    TextureOptions::LINEAR,
                                                );
                                                self.pending_clipboard_image =
                                                    Some((path, w, h, tex));
                                                self.composer_focus_pending = true;
                                                self.show_toast(format!(
                                                    "Đã dán ảnh ({w}×{h} px) từ Clipboard — nhấn Enter để gửi"
                                                ));
                                            } else {
                                                self.show_toast(
                                                    "Không tìm thấy hình ảnh nào trong Clipboard",
                                                );
                                            }
                                        }
                                    });

                                    if !contextual_stickers.is_empty() && response.has_focus() {
                                        let len = contextual_stickers.len();
                                        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                                            self.composer_sticker_selected = Some(0);
                                        } else if let Some(sel) = self.composer_sticker_selected {
                                            if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                                                self.composer_sticker_selected =
                                                    Some((sel + 1) % len);
                                            } else if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                                                self.composer_sticker_selected =
                                                    Some((sel + len - 1) % len);
                                            } else if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                                                self.composer_sticker_selected = None;
                                            }
                                        }
                                    } else {
                                        self.composer_sticker_selected = None;
                                    }

                                    let enter_pressed = (response.lost_focus()
                                        || response.has_focus())
                                        && send_via_enter
                                        && !is_ime_busy;
                                    if enter_pressed {
                                        if let Some(sel) = self.composer_sticker_selected {
                                            if let Some(st) = contextual_stickers.get(sel) {
                                                send_selected_sticker = Some(st.clone());
                                            }
                                        }
                                    }
                                    let can_send = !self.input_message.trim().is_empty()
                                        || self.pending_clipboard_image.is_some();

                                    if let Some(chosen) = send_selected_sticker {
                                        self.input_message.clear();
                                        self.dismissed_sticker_suggest_for.clear();
                                        self.composer_sticker_selected = None;
                                        response.request_focus();
                                        if let Some(tx) = WORKER_TX.get() {
                                            let _ = tx.send(WorkerCmd::SendSticker {
                                                to_id: thread_id.clone(),
                                                sticker_id: chosen.sticker_id,
                                                cate_id: chosen.cate_id,
                                                sticker_type: chosen.sticker_type,
                                                is_group,
                                            });
                                        }
                                    } else if (send_clicked || enter_pressed)
                                        && self.pending_clipboard_image.is_some()
                                    {
                                        if let Some((clip_path, _, _, _)) =
                                            self.pending_clipboard_image.take()
                                        {
                                            let cap = self.input_message.trim().to_string();
                                            self.input_message.clear();
                                            self.dismissed_sticker_suggest_for.clear();
                                            self.composer_sticker_selected = None;
                                            response.request_focus();
                                            if let Some(tx) = WORKER_TX.get() {
                                                let _ = tx.send(WorkerCmd::PickAndSendAttachment {
                                                    to_id: thread_id.clone(),
                                                    is_group,
                                                    images_only: true,
                                                    caption: cap,
                                                    preset_path: Some(clip_path),
                                                    original_hd: self.send_hd_original,
                                                });
                                            }
                                        }
                                    } else if (send_clicked || enter_pressed) && can_send {
                                        let text = self.input_message.trim().to_string();
                                        self.input_message.clear();
                                        self.dismissed_sticker_suggest_for.clear();
                                        self.composer_sticker_selected = None;
                                        response.request_focus();
                                        let quoted = self.replying_to.take();
                                        if let Some(tx) = WORKER_TX.get() {
                                            if let Some(quoted) = quoted {
                                                let _ = tx.send(WorkerCmd::SendQuote {
                                                    to_id: thread_id.clone(),
                                                    text,
                                                    quoted,
                                                    is_group,
                                                });
                                            } else {
                                                let _ = tx.send(WorkerCmd::SendMessage {
                                                    to_id: thread_id.clone(),
                                                    text,
                                                    is_group,
                                                });
                                            }
                                        }
                                    } else if enter_pressed {
                                        response.request_focus();
                                    }
                                });
                        });

                    ui.painter().hline(
                        composer_frame.response.rect.x_range(),
                        composer_frame.response.rect.top(),
                        Stroke::new(1.0_f32, pal.border),
                    );
                    // 4b. Floating Contextual Sticker Suggestion Popover (`egui::Area`, rendered AFTER composer bar)
                    if !contextual_stickers.is_empty() && !self.sticker_picker_open {
                        self.render_contextual_sticker_popover(
                            ctx,
                            composer_frame.response.rect,
                            &contextual_stickers,
                            &thread_id,
                            is_group,
                            pal,
                        );
                    }
                    if let Some(ref link_meta) = composer_link_preview {
                        if contextual_stickers.is_empty() && !self.sticker_picker_open {
                            self.render_composer_link_preview_popover(
                                ctx,
                                composer_frame.response.rect,
                                link_meta,
                                pal,
                            );
                        }
                    }
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(120.0);
                        ui.label(
                            RichText::new("Chọn một hội thoại bên trái để bắt đầu trò chuyện")
                                .size(15.0)
                                .color(pal.text_muted),
                        );
                    });
                }
            });

        self.render_profile_popover(ctx, pal);
        self.render_wipe_confirm(ctx, pal);
        self.render_toast_overlay(ctx, pal);
        self.render_forward_dialog(ctx, pal);
        self.render_call_overlay(ctx, pal);
        self.render_image_viewer_modal(ctx, pal);
        self.render_media_windows(ctx, pal);
        self.render_phone_lookup_modal(ctx, pal);
        self.render_schedule_modal(ctx, pal);
        self.render_settings_modal(ctx, pal);
    }
}

/// Highlight the exact resolved row and optionally center it in the message viewport.
fn render_quote_target(ui: &egui::Ui, rect: egui::Rect, scroll_pending: bool, color: Color32) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        Rounding::same(8.0),
        Stroke::new(2.0_f32, color),
    );
    if scroll_pending {
        ui.scroll_to_rect(rect, Some(Align::Center));
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(100));
}

#[cfg(test)]
mod quote_navigation_tests {
    use super::*;

    #[test]
    fn quote_navigation_invalidates_when_thread_changes_or_highlight_expires() {
        let messages = [ChatMessage {
            id: "original".into(),
            ..Default::default()
        }];
        let mut jump = QuoteJump {
            thread_id: "thread-1".into(),
            reference: "original".into(),
            highlight_until: std::time::Instant::now() + std::time::Duration::from_secs(3),
            scroll_pending: true,
        };
        assert_eq!(jump.target_index("thread-1", &messages), Some(0));
        assert_eq!(jump.target_index("thread-2", &messages), None);
        jump.highlight_until = std::time::Instant::now();
        assert_eq!(jump.target_index("thread-1", &messages), None);
    }

    #[test]
    fn quote_navigation_keeps_animated_jump_visible_after_highlight_expires() {
        for (count, target) in [(1000, 12), (1000, 900), (150, 12)] {
            let ctx = egui::Context::default();
            let color = Color32::from_rgb(1, 2, 3);
            let mut before = 0.0;
            let mut after = 0.0;
            let mut highlighted = false;
            let mut target_visible = false;
            for frame in 0..10 {
                let jumping = frame > 0;
                let highlighting = jumping && frame < 7;
                let limit = if jumping {
                    crate::message_navigation::render_limit_for_target(150, count, target)
                } else {
                    150
                };
                if count == 1000 && target == 12 {
                    assert!(jumping || count - limit > target);
                }
                let output = ctx.run(
                    egui::RawInput {
                        time: Some(frame as f64 * 0.5),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(400.0, 300.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let scroll = egui::ScrollArea::vertical()
                                .id_salt("quote_jump_test")
                                .max_height(200.0)
                                .stick_to_bottom(!highlighting)
                                .show(ui, |ui| {
                                    for index in count - limit..count {
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(200.0, 30.0),
                                            Sense::hover(),
                                        );
                                        if jumping && index == target {
                                            if highlighting {
                                                render_quote_target(ui, rect, frame == 1, color);
                                                highlighted = true;
                                            }
                                            if frame == 9 {
                                                target_visible =
                                                    ui.clip_rect().contains(rect.center());
                                            }
                                        }
                                    }
                                });
                            if jumping {
                                after = scroll.state.offset.y;
                            } else {
                                before = scroll.state.offset.y;
                            }
                        });
                    },
                );
                if frame == 4 {
                    assert!(output.shapes.iter().any(|shape| {
                    matches!(&shape.shape, egui::Shape::Rect(rect) if rect.stroke.color == color)
                }), "Exact target must have a visible highlight shape");
                }
            }
            assert!(highlighted);
            assert!(
                before > 1000.0,
                "Initial viewport must start at the newest messages"
            );
            assert!(
                after < before && target_visible,
                "Jump must center the older target, not remain at the bottom: {after}"
            );
        }
    }
}
