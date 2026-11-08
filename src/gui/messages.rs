use super::*;

// ---------------------------------------------------------------------------
// Chat Bubble & Sticker Widget Rendering
// ---------------------------------------------------------------------------

pub(super) fn is_sticker_message(msg: &ChatMessage) -> Option<(u64, u64)> {
    if let Some(sid) = msg.sticker_id {
        if sid > 0 {
            return Some((sid, msg.sticker_cate_id.unwrap_or(0)));
        }
    }
    let check_ami = |s: &str| -> Option<(u64, u64)> {
        let pos = s.find("ami_fat_cat_")?;
        let num_str = s.get(pos + 12..pos + 14)?;
        let idx = num_str.parse::<u64>().ok()?;
        if (1..=15).contains(&idx) {
            Some((999000 + idx, AMI_FAT_CAT_CATE_ID))
        } else {
            None
        }
    };
    if let Some(hit) = check_ami(&msg.text)
        .or_else(|| msg.link_desc.as_deref().and_then(check_ami))
        .or_else(|| msg.link_title.as_deref().and_then(check_ami))
        .or_else(|| msg.link_url.as_deref().and_then(check_ami))
        .or_else(|| msg.link_thumb.as_deref().and_then(check_ami))
    {
        return Some(hit);
    }
    if msg.text.trim() == "[Nhãn dán]" {
        return Some((18009, 10130));
    }
    None
}

pub(super) fn render_sticker_widget(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
) -> bool {
    let Some((sid, cid)) = is_sticker_message(msg) else {
        return false;
    };

    let key = format!("stk_{cid}_{sid}");
    let url = if cid == 0 && (1..=30).contains(&sid) {
        format!("https://stc-chat.zdn.vn/images/stickers/default/thumb/{sid}.png")
    } else {
        format!("https://api.zaloapp.com/api/emoticon/sticker/webpc?eid={sid}&size=130")
    };

    if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchStickerImage {
                key: key.clone(),
                url,
            });
        }
    }

    if let Some(tex) = sticker_textures.get(&key) {
        ui.image((tex.id(), Vec2::new(116.0, 116.0)));
        true
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub(super) struct CallCardView {
    pub(super) title: String,
    pub(super) status_badge: String,
    pub(super) reason: Option<String>,
    pub(super) is_video: bool,
    pub(super) is_missed_or_failed: bool,
}

/// Detect whether `msg` is a voice/video call log entry and parse its structured metadata.
pub(super) fn extract_call_card_view(msg: &ChatMessage) -> Option<CallCardView> {
    let raw = msg.text.trim();
    let is_call_id = msg.id.starts_with("call_");
    let is_call_text = raw == "Cuộc gọi"
        || raw == "Cuộc gọi thoại"
        || raw == "Cuộc gọi video"
        || raw.starts_with("📞")
        || raw.starts_with("📹")
        || raw.starts_with("Cuộc gọi đi")
        || raw.starts_with("Cuộc gọi đến")
        || raw.starts_with("Cuộc gọi video");
    if !is_call_id && !is_call_text {
        return None;
    }

    let lower = raw.to_lowercase();
    let is_video = lower.contains("video") || raw.starts_with("📹");
    let is_incoming = lower.contains("gọi đến") || (!lower.contains("gọi đi") && !msg.is_outgoing);

    // Parse optional duration `(Ns)`
    let secs_opt: Option<u64> = raw.find('(').and_then(|open| {
        let rest = &raw[open + 1..];
        let close = rest.find("s)")?;
        rest[..close].trim().parse::<u64>().ok()
    });

    // Parse optional failure/rejection reason after `—` or `-`
    let raw_reason = raw
        .split_once(" — ")
        .or_else(|| raw.split_once(" - "))
        .map(|(_, r)| r.trim().to_string())
        .filter(|r| !r.is_empty());

    let is_missed_or_failed = match secs_opt {
        Some(0) => true,
        Some(_) => raw_reason.is_some(),
        None => raw_reason.is_some() || (!msg.is_outgoing && raw == "Cuộc gọi"),
    };

    let title = if is_video {
        if is_incoming {
            if is_missed_or_failed {
                "Cuộc gọi video nhỡ".to_string()
            } else {
                "Cuộc gọi video đến".to_string()
            }
        } else {
            "Cuộc gọi video đi".to_string()
        }
    } else if is_incoming {
        if is_missed_or_failed {
            "Cuộc gọi thoại nhỡ".to_string()
        } else {
            "Cuộc gọi thoại đến".to_string()
        }
    } else {
        "Cuộc gọi thoại đi".to_string()
    };

    let arrow = if is_incoming { "↙" } else { "↗" };
    let status_badge = match (is_missed_or_failed, secs_opt) {
        // The title already names the missed/failed state; show the reason instead of repeating it.
        (true, _) => {
            let reason_txt = raw_reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| {
                    if is_incoming {
                        "Bạn đã bỏ lỡ cuộc gọi này".to_string()
                    } else {
                        "Người nhận không nghe máy".to_string()
                    }
                });
            format!("{arrow} {reason_txt}")
        }
        (false, Some(s)) if s >= 60 => {
            format!("{arrow} Thời lượng: {} phút {:02} giây", s / 60, s % 60)
        }
        (false, Some(s)) => format!("{arrow} Thời lượng: {s} giây"),
        (false, None) => format!("{arrow} Đã kết thúc"),
    };

    // When the call is missed/failed the badge already carries the reason, so only surface a
    // separate reason line for calls that otherwise look successful.
    let reason = if is_missed_or_failed {
        None
    } else {
        raw_reason.map(|r| {
            if r.contains("gián đoạn") || r.contains("từ chối") {
                "Người nhận từ chối hoặc kết nối gián đoạn".to_string()
            } else if r.contains("đang bận") {
                "Người nhận đang bận hoặc từ chối".to_string()
            } else {
                r
            }
        })
    };

    Some(CallCardView {
        title,
        status_badge,
        reason,
        is_video,
        is_missed_or_failed,
    })
}

pub(super) fn render_call_card_widget(
    ui: &mut egui::Ui,
    card: &CallCardView,
    pal: &ThemePalette,
    card_width: f32,
) -> Option<BubbleAction> {
    let mut action = None;
    let accent = if card.is_missed_or_failed {
        Color32::from_rgb(239, 68, 68)
    } else {
        Color32::from_rgb(34, 197, 94)
    };

    ui.set_width(card_width);
    let text_col_w = (card_width - 52.0).max(110.0);
    ui.vertical(|ui| {
        ui.set_width(card_width);
        ui.horizontal(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(40.0, 40.0), Sense::hover());
            ui.painter().circle_filled(
                icon_rect.center(),
                20.0,
                accent.linear_multiply(if pal.is_dark { 0.18 } else { 0.14 }),
            );
            ui.painter().circle_stroke(
                icon_rect.center(),
                20.0,
                Stroke::new(1.2_f32, accent.linear_multiply(0.55)),
            );
            if card.is_video {
                draw_icon_video(ui.painter(), icon_rect.center(), 18.0, accent);
            } else {
                draw_icon_phone(ui.painter(), icon_rect.center(), 18.0, accent);
            }

            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width(text_col_w);
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.title)
                            .size(13.8)
                            .color(pal.text_primary)
                            .strong(),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(&card.status_badge)
                            .size(11.8)
                            .color(accent)
                            .strong(),
                    )
                    .wrap(),
                );
                if let Some(reason) = &card.reason {
                    ui.add_space(3.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(reason).size(11.2).color(pal.text_secondary),
                        )
                        .wrap(),
                    );
                }
            });
        });
        ui.add_space(8.0);
        let (sep_rect, _) = ui.allocate_exact_size(Vec2::new(card_width, 1.0), Sense::hover());
        ui.painter().line_segment(
            [sep_rect.left_center(), sep_rect.right_center()],
            Stroke::new(1.0_f32, pal.border),
        );
        ui.add_space(6.0);

        let btn_label = if card.is_video {
            format!("{}  Gọi video lại", lucide::VIDEO)
        } else {
            format!("{}  Gọi lại", lucide::PHONE)
        };
        let call_btn = egui::Button::new(
            RichText::new(btn_label)
                .size(12.5)
                .color(Color32::WHITE)
                .strong(),
        )
        .fill(pal.primary)
        .stroke(Stroke::new(1.0_f32, pal.primary_hover))
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(card_width, 30.0));

        if ui
            .add(call_btn)
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            action = Some(BubbleAction::CallAgain {
                is_video: card.is_video,
            });
        }
    });
    action
}

/// Open an `http://` or `https://` URL in the user's default system browser.
pub(super) fn open_external_url(url: &str) {
    let _ = crate::platform::open_url(url);
}
/// Pick a folder. Windows uses the real folder picker; other platforms keep the
/// upstream behaviour (pick any file, the caller uses its parent directory).
pub(super) fn pick_native_folder_dialog() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        return rfd::FileDialog::new()
            .set_title("Chọn thư mục lưu file tải về")
            .pick_folder();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let f = pick_native_file_dialog(false)?;
        Some(f.parent().map(|p| p.to_path_buf()).unwrap_or(f))
    }
}
/// Open a native OS file-picker dialog on a worker thread and return the selected path.
pub(super) fn pick_native_file_dialog(images_only: bool) -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        // Native Win32 IFileOpenDialog (via `rfd`): opens instantly, no PowerShell /
        // .NET cold start, and runs natively on ARM64. Called from a worker thread,
        // so the blocking API does not freeze the egui event loop.
        let title = if images_only {
            "Chọn hình ảnh để gửi qua Zalo"
        } else {
            "Chọn video hoặc tập tin đính kèm để gửi qua Zalo"
        };
        let mut dlg = rfd::FileDialog::new().set_title(title);
        dlg = if images_only {
            dlg.add_filter("Hình ảnh", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                .add_filter("Tất cả tập tin", &["*"])
        } else {
            dlg.add_filter("Tất cả tập tin", &["*"])
                .add_filter("Video & âm thanh", &["mp4", "mov", "mkv", "mp3", "wav"])
                .add_filter("Tài liệu", &["pdf", "zip", "docx", "xlsx", "txt"])
        };
        return dlg.pick_file();
    }
    #[cfg(target_os = "macos")]
    {
        let prompt = if images_only {
            "POSIX path of (choose file with prompt \"Chọn hình ảnh để gửi qua Zalo\" of type {\"public.image\"})"
        } else {
            "POSIX path of (choose file with prompt \"Chọn tập tin đính kèm để gửi qua Zalo\")"
        };
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", prompt])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut zcmd = std::process::Command::new("zenity");
        zcmd.arg("--file-selection");
        if images_only {
            zcmd.arg("--file-filter=Images | *.png *.jpg *.jpeg *.webp *.gif *.bmp");
        }
        if let Ok(out) = zcmd.output() {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        if let Ok(out) = std::process::Command::new("kdialog")
            .arg("--getopenfilename")
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !s.is_empty() {
                    return Some(std::path::PathBuf::from(s));
                }
            }
        }
        None
    }
}

/// Detect `Ctrl+V` or `Shift+Insert` reliably on Windows even when `egui-winit` swallows `Key::V`
/// because the system clipboard contains an image instead of UTF-8 text.
pub(super) fn is_clipboard_paste_triggered(ctx: &egui::Context) -> bool {
    let egui_paste = ctx.input(|i| {
        (i.modifiers.ctrl && i.key_pressed(Key::V))
            || (i.modifiers.shift && i.key_pressed(Key::Insert))
            || i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))
    });

    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn GetAsyncKeyState(v_key: i32) -> i16;
        }
        static WAS_PASTE_DOWN: std::sync::atomic::AtomicBool =
            std::sync::atomic::AtomicBool::new(false);

        let is_focused = ctx.input(|i| i.focused);
        if !is_focused {
            WAS_PASTE_DOWN.store(false, std::sync::atomic::Ordering::SeqCst);
            return egui_paste;
        }
        let (win32_down, modifier_held) = unsafe {
            let ctrl = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0; // VK_CONTROL
            let v_key = (GetAsyncKeyState(0x56) as u16 & 0x8000) != 0; // 'V'
            let shift = (GetAsyncKeyState(0x10) as u16 & 0x8000) != 0; // VK_SHIFT
            let ins = (GetAsyncKeyState(0x2D) as u16 & 0x8000) != 0; // VK_INSERT
            ((ctrl && v_key) || (shift && ins), ctrl || shift)
        };
        if modifier_held {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let prev = WAS_PASTE_DOWN.swap(win32_down, std::sync::atomic::Ordering::SeqCst);
        let edge_triggered = win32_down && !prev;
        return egui_paste || edge_triggered;
    }

    #[cfg(not(target_os = "windows"))]
    {
        egui_paste
    }
}

pub(super) fn finalize_clipboard_rgba_image(
    rgba: image::RgbaImage,
) -> Option<(std::path::PathBuf, u32, u32, egui::ColorImage)> {
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    // Absolute path in the data dir's cache (never the process working directory).
    let path = crate::paths::cache_dir().join(format!("clipboard_{now_ms}.png"));
    rgba.save(&path).ok()?;

    let thumb = if width > 240 || height > 240 {
        image::imageops::thumbnail(&rgba, 240, 240)
    } else {
        rgba
    };
    let (tw, th) = thumb.dimensions();
    let col_img =
        egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], thumb.as_raw());
    Some((path, width, height, col_img))
}

/// Read an image from the Windows Clipboard supporting:
/// 1. Registered `"PNG"` / `"image/png"` streams (Snipping Tool, ShareX, Chrome/Edge, Discord)
/// 2. `CF_HDROP` (`15`) copied image files from Windows Explorer or ShareX
/// 3. `CF_DIB` (`8`) / `CF_DIBV5` (`17`) 24-bit & 32-bit bitmaps (`BI_RGB` & `BI_BITFIELDS`)
pub(super) fn read_clipboard_image_to_png(
) -> Option<(std::path::PathBuf, u32, u32, egui::ColorImage)> {
    #[cfg(target_os = "windows")]
    {
        const CF_DIB: u32 = 8;
        const CF_DIBV5: u32 = 17;
        const CF_HDROP: u32 = 15;
        #[link(name = "user32")]
        extern "system" {
            fn IsClipboardFormatAvailable(format: u32) -> i32;
            fn RegisterClipboardFormatA(lpsz_format: *const u8) -> u32;
            fn OpenClipboard(h_wnd_new_owner: *mut std::ffi::c_void) -> i32;
            fn CloseClipboard() -> i32;
            fn GetClipboardData(u_format: u32) -> *mut std::ffi::c_void;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GlobalLock(h_mem: *mut std::ffi::c_void) -> *const u8;
            fn GlobalUnlock(h_mem: *mut std::ffi::c_void) -> i32;
            fn GlobalSize(h_mem: *mut std::ffi::c_void) -> usize;
        }
        #[link(name = "shell32")]
        extern "system" {
            fn DragQueryFileW(
                h_drop: *mut std::ffi::c_void,
                i_file: u32,
                lpsz_file: *mut u16,
                cch: u32,
            ) -> u32;
        }

        // SAFETY: clipboard handles are inspected and released on this thread; locked data is copied before release.
        unsafe {
            let cf_png = RegisterClipboardFormatA(b"PNG\0".as_ptr());
            let cf_mime_png = RegisterClipboardFormatA(b"image/png\0".as_ptr());
            let has_png = cf_png != 0 && IsClipboardFormatAvailable(cf_png) != 0;
            let has_mime_png = cf_mime_png != 0 && IsClipboardFormatAvailable(cf_mime_png) != 0;
            let has_dib = IsClipboardFormatAvailable(CF_DIB) != 0
                || IsClipboardFormatAvailable(CF_DIBV5) != 0;
            let has_hdrop = IsClipboardFormatAvailable(CF_HDROP) != 0;

            if !has_png && !has_mime_png && !has_dib && !has_hdrop {
                return None;
            }

            // Retry OpenClipboard briefly in case egui-winit/arboard or ShareX is releasing the lock
            let mut opened = false;
            for _ in 0..6 {
                if OpenClipboard(std::ptr::null_mut()) != 0 {
                    opened = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(8));
            }
            if !opened {
                return None;
            }

            // 1. Try raw PNG stream first (preserves exact RGBA and avoids DIB header quirks)
            for fmt in [cf_png, cf_mime_png] {
                if fmt != 0 && IsClipboardFormatAvailable(fmt) != 0 {
                    let hmem = GetClipboardData(fmt);
                    if !hmem.is_null() {
                        let ptr = GlobalLock(hmem);
                        let len = GlobalSize(hmem);
                        if !ptr.is_null() && len > 8 {
                            let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                            GlobalUnlock(hmem);
                            if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                                CloseClipboard();
                                return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                            }
                        } else if !ptr.is_null() {
                            GlobalUnlock(hmem);
                        }
                    }
                }
            }

            // 2. Try CF_HDROP (copied image file in Windows Explorer or ShareX)
            if has_hdrop {
                let hdrop = GetClipboardData(CF_HDROP);
                if !hdrop.is_null() {
                    let mut buf = vec![0u16; 1024];
                    let chars =
                        DragQueryFileW(hdrop, 0, buf.as_mut_ptr(), buf.len() as u32) as usize;
                    if chars > 0 && chars < buf.len() {
                        let file_path =
                            std::path::PathBuf::from(String::from_utf16_lossy(&buf[..chars]));
                        if let Ok(dyn_img) = image::open(&file_path) {
                            CloseClipboard();
                            let rgba = dyn_img.to_rgba8();
                            let (width, height) = rgba.dimensions();
                            let thumb = if width > 240 || height > 240 {
                                image::imageops::thumbnail(&rgba, 240, 240)
                            } else {
                                rgba
                            };
                            let (tw, th) = thumb.dimensions();
                            let col_img = egui::ColorImage::from_rgba_unmultiplied(
                                [tw as usize, th as usize],
                                thumb.as_raw(),
                            );
                            return Some((file_path, width, height, col_img));
                        }
                    }
                }
            }

            // Validate while borrowed, then release handles before pixel decoding.
            for format in [CF_DIBV5, CF_DIB] {
                if IsClipboardFormatAvailable(format) == 0 {
                    continue;
                }
                let handle = GetClipboardData(format);
                if handle.is_null() {
                    continue;
                }
                let len = GlobalSize(handle);
                if !(40..=crate::clipboard_dib::MAX_RAW).contains(&len) {
                    continue;
                }
                let ptr = GlobalLock(handle);
                if ptr.is_null() {
                    continue;
                }
                // SAFETY: GlobalSize bounds this locked allocation; no borrowed data escapes.
                let borrowed = std::slice::from_raw_parts(ptr, len);
                let owned = crate::clipboard_dib::validate_dib(borrowed)
                    .ok()
                    .map(|_| borrowed.to_vec());
                GlobalUnlock(handle);
                if let Some(bytes) = owned {
                    CloseClipboard();
                    return crate::clipboard_dib::decode_dib(&bytes)
                        .ok()
                        .and_then(finalize_clipboard_rgba_image);
                }
            }

            CloseClipboard();
            None
        }
    }
    #[cfg(target_os = "macos")]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let tmp = std::env::temp_dir().join(format!("zalo_mac_clip_{now}.png"));
        let tmp_str = tmp.to_str().unwrap_or("/tmp/zalo_mac_clip.png");
        let script = format!(
            "try\nset img to the clipboard as «class PNGf»\nset fp to open for access (POSIX file \"{tmp_str}\") with write permission\nset eof fp to 0\nwrite img to fp\nclose access fp\nreturn \"ok\"\non error\nreturn \"none\"\nend try"
        );
        if let Ok(out) = std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
        {
            let s = String::from_utf8_lossy(&out.stdout);
            if s.trim() == "ok" && tmp.exists() {
                if let Ok(bytes) = std::fs::read(&tmp) {
                    let _ = std::fs::remove_file(&tmp);
                    if let Ok(dyn_img) = image::load_from_memory(&bytes) {
                        return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                    }
                }
                let _ = std::fs::remove_file(&tmp);
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Wayland (`wl-paste --type image/png`) or X11 (`xclip -selection clipboard -t image/png -o`)
        for (prog, args) in [
            ("wl-paste", &["--no-newline", "--type", "image/png"][..]),
            (
                "xclip",
                &["-selection", "clipboard", "-t", "image/png", "-o"][..],
            ),
        ] {
            if let Ok(out) = std::process::Command::new(prog).args(args).output() {
                if out.status.success() && out.stdout.len() > 8 {
                    if let Ok(dyn_img) = image::load_from_memory(&out.stdout) {
                        return finalize_clipboard_rgba_image(dyn_img.to_rgba8());
                    }
                }
            }
        }
        None
    }
}

/// Format a URL for clean UI display by stripping tracking query strings (`?_r=1&utm_...`)
/// and truncating to `max_chars`.
pub(super) fn clean_display_url(url: &str, max_chars: usize) -> String {
    let trimmed = url.trim();
    let no_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let base = no_scheme
        .split('?')
        .next()
        .unwrap_or(no_scheme)
        .trim_end_matches('/');
    truncate_chars(base, max_chars).into_owned()
}

/// Extract `(user_comment, url, domain)` from a message when `msg.link_url` is set or when
/// `msg.text` contains an embedded `http://` / `https://` URL.
///
/// Note: `user_comment` only contains extra text typed by the sender *outside* the link's own
/// `link_title` / `link_desc` (so `link_desc` is rendered inside the Link Preview Card rather than
/// duplicated as a large paragraph above the card).
pub(super) fn extract_clean_domain(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("Liên kết")
        .trim()
        .strip_prefix("www.")
        .unwrap_or_else(|| {
            url.split("://")
                .nth(1)
                .unwrap_or(url)
                .split('/')
                .next()
                .unwrap_or("Liên kết")
                .trim()
        })
        .to_string()
}

pub(super) fn extract_message_link_view(msg: &ChatMessage) -> Option<(String, String, String)> {
    if let Some(url) = msg.link_url.as_deref().filter(|u| !u.is_empty()) {
        if crate::chat::is_zalo_photo_cdn_url(url) {
            return None;
        }
        let desc_trim = msg.link_desc.as_deref().unwrap_or("").trim();
        let title_trim = msg.link_title.as_deref().unwrap_or("").trim();
        let url_base = url.split('?').next().unwrap_or(url);

        let mut kept_lines = Vec::new();
        for raw_line in msg.text.lines() {
            let mut line = raw_line.trim().to_string();
            if let Some(pos) = line.find("http://").or_else(|| line.find("https://")) {
                let after = &line[pos..];
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                let found = &after[..end];
                if found == url || found.starts_with(url_base) {
                    line = format!("{}{}", &line[..pos], &after[end..])
                        .trim()
                        .to_string();
                }
            }
            if line.is_empty()
                || line == url
                || line == title_trim
                || line == desc_trim
                || line == "[Chia sẻ]"
                || line == "[Liên kết]"
                || (!desc_trim.is_empty() && desc_trim.contains(&line))
                || (!title_trim.is_empty() && title_trim.contains(&line))
            {
                continue;
            }
            kept_lines.push(line);
        }
        let user_comment = kept_lines.join("\n").trim().to_string();
        let domain = extract_clean_domain(url);
        return Some((user_comment, url.to_string(), domain));
    }
    let text = msg.text.trim();
    let pos = text.find("https://").or_else(|| text.find("http://"))?;
    let after = &text[pos..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let url = after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';' | '>'));
    if url.len() < 10 || crate::chat::is_zalo_photo_cdn_url(url) {
        return None;
    }
    let caption = format!("{}{}", &text[..pos], &after[end..])
        .trim()
        .to_string();
    let domain = extract_clean_domain(url);
    Some((caption, url.to_string(), domain))
}

/// Normalize a millisecond or second timestamp to milliseconds (`> 0`).
pub(super) fn normalize_ts_ms(ts: i64) -> i64 {
    if ts <= 0 {
        0
    } else if ts < 10_000_000_000 {
        ts * 1000
    } else {
        ts
    }
}

/// Return `true` when a date/time separator pill should be inserted before `curr`.
pub(super) fn should_show_time_divider(prev: Option<&ChatMessage>, curr: &ChatMessage) -> bool {
    let Some(prev) = prev else {
        return true;
    };
    let prev_ms = normalize_ts_ms(prev.ts);
    let curr_ms = normalize_ts_ms(curr.ts);
    if prev_ms > 0 && curr_ms > 0 {
        // Local calendar days (OS time zone) — no hard-coded UTC offset.
        if crate::timefmt::local_day_index(prev_ms) != crate::timefmt::local_day_index(curr_ms) {
            return true;
        }
        return (curr_ms - prev_ms).abs() >= 30 * 60 * 1000;
    }
    false
}

/// Format the label for a centered date/time separator chip.
///
/// A new calendar day shows the day (`Hôm nay`, `Hôm qua`, `Thứ Hai, 27/10`, `27/10/2025`);
/// a long pause within the same day shows just the time (`14:05`).
pub(super) fn format_time_divider_label(prev: Option<&ChatMessage>, msg: &ChatMessage) -> String {
    let ts_ms = normalize_ts_ms(msg.ts);
    if ts_ms <= 0 {
        return if msg.time.is_empty() {
            "Cuộc trò chuyện".to_string()
        } else {
            msg.time.clone()
        };
    }
    let msg_day = crate::timefmt::local_day_index(ts_ms);
    let same_day_as_prev = prev
        .map(|p| normalize_ts_ms(p.ts))
        .filter(|p| *p > 0)
        .map(|p| crate::timefmt::local_day_index(p) == msg_day)
        .unwrap_or(false);
    if same_day_as_prev {
        return crate::timefmt::hm_from_ms(ts_ms);
    }
    let today = crate::timefmt::local_day_index(crate::timefmt::now_ms());
    let (y, m, d) = crate::timefmt::local_ymd(ts_ms);
    match today - msg_day {
        0 => "Hôm nay".to_string(),
        1 => "Hôm qua".to_string(),
        2..=6 => format!("{}, {d:02}/{m:02}", weekday_long(crate::timefmt::local_weekday(ts_ms))),
        _ => format!("{d:02}/{m:02}/{y}"),
    }
}

/// Render a centered compact date chip (white pill with a hairline border).
pub(super) fn render_date_divider(
    ui: &mut egui::Ui,
    prev: Option<&ChatMessage>,
    msg: &ChatMessage,
    pal: &ThemePalette,
) {
    let label = format_time_divider_label(prev, msg);
    let avail_w = ui.available_width();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(
                label.clone(),
                FontId::proportional(11.5),
                pal.text_secondary,
            )
            .size()
            .x
        });
        let pill_w = text_w + 28.0;
        let left_space = ((avail_w - pill_w) * 0.5).max(0.0);
        if left_space > 0.0 {
            ui.add_space(left_space);
        }
        egui::Frame::none()
            .fill(pal.chip_bg)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(11.0))
            .inner_margin(Margin::symmetric(13.0, 4.0))
            .shadow(egui::epaint::Shadow {
                offset: Vec2::new(0.0, 1.0),
                blur: 6.0,
                spread: 0.0,
                color: pal.shadow,
            })
            .show(ui, |ui| {
                ui.label(RichText::new(label).size(12.0).color(pal.text_secondary));
            });
    });
}

/// Detect whether `msg` is a system/event notification rather than a user speech bubble.
pub(super) fn is_system_notice_message(msg: &ChatMessage) -> bool {
    if msg.id.starts_with("sys_") {
        return true;
    }
    let t = msg.text.trim();
    t.starts_with("[Hệ thống]")
        || t.starts_with("[Thông báo]")
        || (t.starts_with("Bạn và ") && t.contains("đã trở thành bạn bè"))
        || t.contains("đã thêm bạn vào nhóm")
        || t.contains("đã tham gia nhóm")
        || t.contains("đã rời khỏi nhóm")
        || t.contains("đã đổi tên nhóm")
}

/// Render a centered system event pill (e.g., friend connected, group event).
pub(super) fn render_system_notice_pill(ui: &mut egui::Ui, msg: &ChatMessage, pal: &ThemePalette) {
    let avail_w = ui.available_width();
    let text = msg.text.trim();
    ui.horizontal(|ui| {
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(
                text.to_string(),
                FontId::proportional(12.0),
                pal.text_secondary,
            )
            .size()
            .x
        });
        let pill_w = (text_w + 28.0).min(avail_w - 40.0);
        let left_pad = ((avail_w - pill_w) * 0.5).max(0.0);
        if left_pad > 0.0 {
            ui.add_space(left_pad);
        }
        egui::Frame::none()
            .fill(pal.bg_elevated)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} {text}", lucide::SPARKLES))
                        .size(12.0)
                        .color(pal.text_secondary),
                );
            });
    });
}

/// Return `true` when two adjacent messages belong to the same visual bubble group.
/// Render chat message text with full Markdown support:
/// - Fenced code blocks (` ```lang ... ``` `)
/// - Headings (`#`, `##`, `###`)
/// - Bullet lists (`- `, `* `) and blockquotes (`> `)
/// - Inline `**bold**`, `*italic*`, `~~strikethrough~~`, `` `inline code` ``, and `[link](url)`
fn strip_markdown_syntax_for_width(s: &str) -> String {
    s.replace("```", "")
        .replace("**", "")
        .replace("__", "")
        .replace("~~", "")
        .replace('`', "")
}

fn render_inline_job(ui: &mut egui::Ui, job: egui::text::LayoutJob, link: Option<String>) {
    if job.text.is_empty() {
        return;
    }
    let label = egui::Label::new(job).wrap();
    if let Some(url) = link {
        let resp = ui.add(label.sense(Sense::click()));
        if resp
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(&url)
            .clicked()
        {
            open_external_url(&url);
        }
    } else {
        ui.add(label);
    }
}

fn message_mention_tokens(msg: &ChatMessage) -> Vec<String> {
    let raw_mentions = msg
        .wire_ref
        .fields
        .get("mentions")
        .and_then(serde_json::Value::as_array)
        .filter(|mentions| !mentions.is_empty())
        .or_else(|| {
            msg.history_source
                .as_ref()
                .and_then(|source| source.pointer("/meta/mentionsList"))
                .and_then(serde_json::Value::as_array)
        });
    let byte_at_utf16 = |offset: usize| {
        let mut units = 0;
        for (byte, ch) in msg.text.char_indices() {
            if units == offset {
                return Some(byte);
            }
            units += ch.len_utf16();
            if units > offset {
                return None;
            }
        }
        (units == offset).then_some(msg.text.len())
    };
    let mut tokens = Vec::new();
    if let Some(mentions) = raw_mentions {
        for mention in mentions {
            let Some(pos) = mention.get("pos").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let Some(len) = mention.get("len").and_then(serde_json::Value::as_u64) else {
                continue;
            };
            let (Ok(pos), Ok(len)) = (usize::try_from(pos), usize::try_from(len)) else {
                continue;
            };
            let Some(end_unit) = pos.checked_add(len) else {
                continue;
            };
            let Some(start) = byte_at_utf16(pos) else {
                continue;
            };
            let Some(end) = byte_at_utf16(end_unit) else {
                continue;
            };
            let start = if msg.text[start..end].starts_with('@') {
                start
            } else if start > 0 && msg.text[..start].ends_with('@') {
                start - 1
            } else {
                continue;
            };
            if let Some(token) = msg.text.get(start..end) {
                tokens.push(token.to_owned());
            }
        }
    }
    tokens.extend(msg.resolved_mention_uids.keys().map(|name| {
        let name = name.trim();
        if name.starts_with('@') {
            name.to_owned()
        } else {
            format!("@{name}")
        }
    }));
    tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
    tokens.dedup();
    tokens
}

fn render_markdown_inline_rich(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
    mention_tokens: &[String],
) -> Option<String> {
    let mut clicked_mention = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        clicked_mention =
            render_markdown_inline_wrapped(ui, text, base_size, base_color, pal, mention_tokens);
    });
    clicked_mention
}

fn render_markdown_inline_wrapped(
    ui: &mut egui::Ui,
    text: &str,
    base_size: f32,
    base_color: Color32,
    pal: &ThemePalette,
    mention_tokens: &[String],
) -> Option<String> {
    let mut job = egui::text::LayoutJob::default();
    let mut first_link: Option<String> = None;
    let mut rest = text;

    while !rest.is_empty() {
        let next_bold = rest.find("**");
        let next_under = rest.find("__");
        let next_strike = rest.find("~~");
        let next_code = rest.find('`');
        let next_mention = rest.find('@');
        let next_italic = rest.find('*').filter(|&i| rest.get(i..i + 2) != Some("**"));
        let next_link = rest.find('[').and_then(|lb| {
            let after_lb = &rest[lb + 1..];
            let rb = after_lb.find("](")?;
            let after_rb = &after_lb[rb + 2..];
            let rp = after_rb.find(')')?;
            Some((lb, lb + 1 + rb, lb + 1 + rb + 2 + rp))
        });

        let min_pos = [
            next_bold,
            next_under,
            next_strike,
            next_code,
            next_mention,
            next_italic,
            next_link.map(|(lb, _, _)| lb),
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(pos) = min_pos else {
            job.append(
                rest,
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: base_color,
                    ..Default::default()
                },
            );
            break;
        };

        if pos > 0 {
            job.append(
                &rest[..pos],
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: base_color,
                    ..Default::default()
                },
            );
            rest = &rest[pos..];
            continue;
        }

        if let Some((0, rb_idx, rp_idx)) = next_link {
            let label = &rest[1..rb_idx];
            let url = &rest[rb_idx + 2..rp_idx];
            if first_link.is_none() && (url.starts_with("http://") || url.starts_with("https://")) {
                first_link = Some(url.to_string());
            }
            job.append(
                label.trim_matches('*').trim_matches('`'),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: pal.primary_hover,
                    underline: Stroke::new(1.0_f32, pal.primary_hover),
                    ..Default::default()
                },
            );
            rest = &rest[rp_idx + 1..];
            continue;
        }

        // Render @All and @Name mentions as styled chips/links in Zalo Blue
        let mention_boundary = job
            .text
            .chars()
            .last()
            .map(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | ':' | ';' | ','))
            .unwrap_or(true);
        if rest.starts_with('@') && mention_boundary {
            let after = &rest[1..];
            let is_all = after.starts_with("All")
                || after.starts_with("all")
                || after.starts_with("ALL")
                || after.starts_with("Tất cả")
                || after.starts_with("tất cả")
                || after.starts_with("Tat ca")
                || after.starts_with("tat ca");
            if is_all {
                let all_len = if after.starts_with("Tất cả") || after.starts_with("tất cả")
                {
                    "Tất cả".len()
                } else if after.starts_with("Tat ca") || after.starts_with("tat ca") {
                    6
                } else {
                    3
                };
                let next_char = after[all_len..].chars().next();
                if next_char.is_none()
                    || next_char.unwrap().is_whitespace()
                    || matches!(next_char.unwrap(), '.' | ',' | '!' | '?' | ':' | ';')
                {
                    let mention_text = &rest[..1 + all_len];
                    job.append(
                        mention_text,
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::proportional(base_size),
                            color: if pal.is_dark {
                                Color32::from_rgb(97, 175, 239)
                            } else {
                                Color32::from_rgb(0, 104, 255)
                            },
                            ..Default::default()
                        },
                    );
                    rest = &after[all_len..];
                    continue;
                }
            }

            if let Some(mention_token) = mention_tokens.iter().find(|token| {
                rest.starts_with(token.as_str())
                    && rest[token.len()..].chars().next().map_or(true, |ch| {
                        ch.is_whitespace() || matches!(ch, ',' | '.' | '!' | '?' | ':' | ';')
                    })
            }) {
                let candidate_name = mention_token.trim_start_matches('@');
                let mention_len = mention_token.len();
                let mention_str = &rest[..mention_len];
                render_inline_job(ui, std::mem::take(&mut job), first_link.take());
                let mention_color = if pal.is_dark {
                    Color32::from_rgb(97, 175, 239)
                } else {
                    Color32::from_rgb(0, 104, 255)
                };
                let mention_response = ui.add(
                    egui::Label::new(
                        RichText::new(mention_str)
                            .size(base_size)
                            .color(mention_color),
                    )
                    .sense(Sense::click()),
                );
                if mention_response
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(format!("Xem thông tin {}", candidate_name))
                    .clicked()
                {
                    return Some(candidate_name.to_owned());
                }
                rest = &rest[mention_len..];
                continue;
            }
        }

        if rest.starts_with("**") {
            let after = &rest[2..];
            if let Some(end_b) = after.find("**") {
                job.append(
                    &after[..end_b],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size + 0.4),
                        color: if pal.is_dark {
                            Color32::WHITE
                        } else {
                            pal.text_primary
                        },
                        ..Default::default()
                    },
                );
                rest = &after[end_b + 2..];
                continue;
            }
        }

        if rest.starts_with("__") {
            let after = &rest[2..];
            if let Some(end_u) = after.find("__") {
                job.append(
                    &after[..end_u],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size),
                        color: base_color,
                        underline: Stroke::new(1.0_f32, base_color),
                        ..Default::default()
                    },
                );
                rest = &after[end_u + 2..];
                continue;
            }
        }

        if rest.starts_with("~~") {
            let after = &rest[2..];
            if let Some(end_s) = after.find("~~") {
                job.append(
                    &after[..end_s],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::proportional(base_size),
                        color: base_color.linear_multiply(0.75),
                        strikethrough: Stroke::new(1.0_f32, base_color),
                        ..Default::default()
                    },
                );
                rest = &after[end_s + 2..];
                continue;
            }
        }
        if rest.starts_with('*') && !rest.starts_with("**") {
            let after = &rest[1..];
            if let Some(end_i) = after.find('*') {
                if end_i > 0 {
                    job.append(
                        &after[..end_i],
                        0.0,
                        egui::TextFormat {
                            font_id: FontId::proportional(base_size),
                            color: base_color,
                            italics: true,
                            ..Default::default()
                        },
                    );
                    rest = &after[end_i + 1..];
                    continue;
                }
            }
        }

        if rest.starts_with('`') {
            let after = &rest[1..];
            if let Some(end_c) = after.find('`') {
                job.append(
                    &after[..end_c],
                    0.0,
                    egui::TextFormat {
                        font_id: FontId::monospace((base_size - 0.8).max(11.0)),
                        color: pal.primary_hover,
                        background: pal.bg_input,
                        ..Default::default()
                    },
                );
                rest = &after[end_c + 1..];
                continue;
            }
        }

        let mut chars = rest.chars();
        if let Some(ch) = chars.next() {
            let mut buf = [0u8; 4];
            job.append(
                ch.encode_utf8(&mut buf),
                0.0,
                egui::TextFormat {
                    font_id: FontId::proportional(base_size),
                    color: base_color,
                    ..Default::default()
                },
            );
            rest = chars.as_str();
        } else {
            break;
        }
    }

    render_inline_job(ui, job, first_link);
    None
}

pub(super) fn render_chat_markdown_block(
    ui: &mut egui::Ui,
    raw: &str,
    text_col: Color32,
    pal: &ThemePalette,
    mention_tokens: &[String],
) -> Option<String> {
    let mut in_code_block = false;
    let mut code_lines: Vec<&str> = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            if in_code_block {
                let code_text = code_lines.join("\n");
                code_lines.clear();
                in_code_block = false;
                egui::Frame::none()
                    .fill(pal.bg_input)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(6.0))
                    .inner_margin(Margin::symmetric(10.0, 7.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(code_text)
                                    .font(FontId::monospace(12.6))
                                    .color(pal.primary_hover),
                            )
                            .wrap(),
                        );
                    });
            } else {
                in_code_block = true;
            }
            continue;
        }
        if in_code_block {
            code_lines.push(line);
            continue;
        }
        let t_start = trimmed.trim_start();
        if let Some(h3) = t_start.strip_prefix("### ") {
            if let Some(name) =
                render_markdown_inline_rich(ui, h3, 15.0, text_col, pal, mention_tokens)
            {
                return Some(name);
            }
        } else if let Some(h2) = t_start.strip_prefix("## ") {
            if let Some(name) =
                render_markdown_inline_rich(ui, h2, 16.0, text_col, pal, mention_tokens)
            {
                return Some(name);
            }
        } else if let Some(h1) = t_start.strip_prefix("# ") {
            if let Some(name) =
                render_markdown_inline_rich(ui, h1, 17.0, text_col, pal, mention_tokens)
            {
                return Some(name);
            }
        } else if let Some(quote) = t_start.strip_prefix("> ") {
            let mut clicked_mention = None;
            ui.horizontal(|ui| {
                let (bar_r, _) = ui.allocate_exact_size(Vec2::new(3.0, 16.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar_r, Rounding::same(1.5), pal.primary_hover);
                clicked_mention = render_markdown_inline_rich(
                    ui,
                    quote,
                    13.8,
                    text_col.linear_multiply(0.88),
                    pal,
                    mention_tokens,
                );
            });
            if clicked_mention.is_some() {
                return clicked_mention;
            }
        } else if let Some(bullet) = t_start
            .strip_prefix("- ")
            .or_else(|| t_start.strip_prefix("* "))
        {
            let bullet_line = format!("• {bullet}");
            if let Some(name) = render_markdown_inline_rich(
                ui,
                &bullet_line,
                14.0,
                text_col,
                pal,
                mention_tokens,
            ) {
                return Some(name);
            }
        } else {
            if let Some(name) =
                render_markdown_inline_rich(ui, trimmed, 14.0, text_col, pal, mention_tokens)
            {
                return Some(name);
            }
        }
    }
    if !code_lines.is_empty() {
        let code_text = code_lines.join("\n");
        egui::Frame::none()
            .fill(pal.bg_input)
            .stroke(Stroke::new(1.0_f32, pal.border))
            .rounding(Rounding::same(6.0))
            .inner_margin(Margin::symmetric(10.0, 7.0))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(code_text)
                            .font(FontId::monospace(12.6))
                            .color(pal.primary_hover),
                    )
                    .wrap(),
                );
            });
    }
    None
}

pub(super) fn same_message_cluster(a: &ChatMessage, b: &ChatMessage) -> bool {
    if is_system_notice_message(a) || is_system_notice_message(b) {
        return false;
    }
    if a.is_outgoing != b.is_outgoing || a.sender != b.sender {
        return false;
    }
    if should_show_time_divider(Some(a), b) {
        return false;
    }
    let a_ms = normalize_ts_ms(a.ts);
    let b_ms = normalize_ts_ms(b.ts);
    if a_ms > 0 && b_ms > 0 && (b_ms - a_ms).abs() > 10 * 60 * 1000 {
        return false;
    }
    true
}

fn render_group_poll_widget(
    ui: &mut egui::Ui,
    poll: &crate::chat::GroupPoll,
    pal: &ThemePalette,
    max_width: f32,
) -> Option<BubbleAction> {
    let selection_id = Id::new(("group_poll_selection", poll.poll_id));
    let mut selected = ui
        .ctx()
        .data_mut(|data| data.get_temp::<std::collections::HashSet<u64>>(selection_id))
        .unwrap_or_else(|| {
            poll.options
                .iter()
                .filter(|option| option.voted)
                .map(|option| option.option_id)
                .collect()
        });
    let mut selection_changed = false;
    let mut submit = false;
    egui::Frame::none()
        .fill(pal.bg_elevated.linear_multiply(0.9))
        .stroke(Stroke::new(1.0_f32, pal.border))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.set_min_width(max_width.clamp(220.0, 360.0));
            ui.label(RichText::new("Bình chọn").size(11.0).color(pal.primary).strong());
            ui.label(RichText::new(&poll.question).size(14.0).strong().color(pal.text_primary));
            ui.add_space(4.0);
            let show_results = !poll.is_hide_vote_preview || poll.joined;
            let has_voted = poll.joined || poll.options.iter().any(|option| option.voted);
            let total_votes = poll.num_vote.max(poll.options.iter().map(|option| option.votes).sum());
            ui.add_enabled_ui(!poll.closed, |ui| {
                for option in &poll.options {
                    let is_selected = selected.contains(&option.option_id);
                    if ui.selectable_label(is_selected, &option.content).clicked() {
                        if is_selected {
                            selected.remove(&option.option_id);
                        } else {
                            if !poll.allow_multi_choices {
                                selected.clear();
                            }
                            selected.insert(option.option_id);
                        }
                        selection_changed = true;
                    }
                    if show_results {
                        let fraction = if total_votes == 0 {
                            0.0
                        } else {
                            option.votes as f32 / total_votes as f32
                        };
                        ui.add(
                            egui::ProgressBar::new(fraction.clamp(0.0, 1.0))
                                .text(format!(
                                    "{} · {}%",
                                    option.votes,
                                    (fraction * 100.0).round() as u32
                                )),
                        );
                    }
                }
            });
            if poll.closed {
                ui.label(RichText::new("Bình chọn đã kết thúc").size(11.0).color(pal.text_muted));
            } else {
                ui.horizontal(|ui| {
                    let label = if selected.is_empty() && has_voted {
                        "Bỏ bình chọn"
                    } else {
                        "Gửi bình chọn"
                    };
                    if ui
                        .add_enabled(!selected.is_empty() || has_voted, egui::Button::new(label))
                        .clicked()
                    {
                        submit = true;
                    }
                    let count_label = if show_results {
                        format!("{total_votes} lượt bình chọn")
                    } else {
                        "Kết quả sẽ hiện sau khi bình chọn".into()
                    };
                    ui.label(RichText::new(count_label).size(10.5).color(pal.text_muted));
                });
            }
        });
    if selection_changed {
        ui.ctx()
            .data_mut(|data| data.insert_temp(selection_id, selected.clone()));
    }
    submit.then(|| BubbleAction::VoteGroupPoll {
        poll_id: poll.poll_id,
        option_ids: selected.into_iter().collect(),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_message_body(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    thread_name: &str,
    pal: &ThemePalette,
    outgoing: bool,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    downloads: &HashMap<String, DownloadState>,
) -> Option<BubbleAction> {
    let mut body_action = None;
    let dl_target = attachment_download_target(msg);
    let dl_state = dl_target.as_ref().and_then(|(u, _)| downloads.get(u));
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };

    let trimmed_text = msg.text.trim();
    let mention_tokens = message_mention_tokens(msg);
    if trimmed_text == "[Tin nhắn đã thu hồi]" {
        ui.label(
            RichText::new(format!("{} Tin nhắn đã được thu hồi", lucide::BAN))
                .color(meta_col)
                .italics()
                .size(13.8),
        );
        return None;
    }

    if trimmed_text.starts_with("[Đã thu hồi]") {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} Đã thu hồi", lucide::BAN))
                    .color(Color32::from_rgb(239, 68, 68))
                    .italics()
                    .strong()
                    .size(12.0),
            );
        });
        ui.add_space(2.0);
    }

    let sender_label = if msg.is_outgoing {
        "Bạn".to_string()
    } else if !msg.sender.is_empty() && msg.sender != "0" {
        msg.sender.clone()
    } else {
        thread_name.to_string()
    };

    if let Some(poll) = msg.poll.as_ref() {
        return render_group_poll_widget(ui, poll, pal, max_text_width);
    }

    // Dedicated inline Photo Card when the message is an uploaded/received Zalo photo (#72)
    let is_zalo_photo = is_zalo_photo_message(msg);
    if is_zalo_photo {
        if let Some(thumb_url) = photo_primary_url(msg) {
            let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
            if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
                if let Some(tx) = WORKER_TX.get() {
                    let _ = tx.send(WorkerCmd::FetchStickerImage {
                        key: key.clone(),
                        url: thumb_url.to_string(),
                    });
                }
            }
            let open_target = msg
                .link_url
                .as_deref()
                .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
                .unwrap_or(thumb_url)
                .to_string();
            let title_label = msg
                .link_title
                .as_deref()
                .map(str::trim)
                .filter(|t| {
                    !t.is_empty()
                        && !t.starts_with("http://")
                        && !t.starts_with("https://")
                        && !t.ends_with(".zdn.vn")
                        && !t.ends_with(".zadn.vn")
                        && !t.ends_with(".dlfl.vn")
                        && !t.contains("zpcloud")
                        && !t.contains("zpg-r")
                        && !t.contains("photo-stal")
                        && !t.contains("photo-zmp")
                })
                .unwrap_or("Hình ảnh")
                .to_string();

            if let Some(tex) = sticker_textures.get(&key) {
                let [tw, th] = tex.size();
                if tw <= 2 || th <= 2 {
                    // Expired or empty tombstone image on Zalo CDN (e.g. 1x1 jpeg)
                    let ph_w = max_text_width.clamp(180.0, 260.0);
                    let ph_frame = egui::Frame::none()
                        .fill(text_col.linear_multiply(0.05))
                        .stroke(Stroke::new(1.0_f32, pal.border))
                        .rounding(Rounding::same(12.0))
                        .inner_margin(Margin::symmetric(16.0, 14.0))
                        .show(ui, |ui| {
                            ui.set_min_width(ph_w);
                            ui.vertical_centered(|ui| {
                                let (ir, _) =
                                    ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                                draw_icon_image(
                                    ui.painter(),
                                    ir.center(),
                                    22.0,
                                    text_col.linear_multiply(0.4),
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Ảnh đã hết hạn trên Zalo")
                                        .size(12.5)
                                        .color(text_col.linear_multiply(0.6)),
                                );
                            });
                        });
                    if ui
                        .interact(
                            ph_frame.response.rect,
                            Id::new(("photo_exp_click", &msg.id, msg.ts)),
                            Sense::click(),
                        )
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Bấm để thử mở liên kết ảnh gốc")
                        .clicked()
                    {
                        open_external_url(&open_target);
                    }
                } else {
                    let tw_f = tw as f32;
                    let th_f = th as f32;
                    let max_w = max_text_width.clamp(220.0, 360.0);
                    let max_h = 340.0_f32;
                    let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                    let disp_w = (tw_f * scale).clamp(160.0, max_w);
                    let disp_h = (th_f * scale).clamp(120.0, max_h);
                    let img = egui::Image::new(tex)
                        .fit_to_exact_size(Vec2::new(disp_w, disp_h))
                        .rounding(Rounding::same(12.0))
                        .sense(Sense::click());
                    let resp = ui
                        .add(img)
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .on_hover_text("Bấm để xem ảnh phóng to trong ứng dụng");
                    if resp.hovered() {
                        ui.painter().rect_filled(
                            resp.rect,
                            Rounding::same(12.0),
                            Color32::from_rgba_unmultiplied(6, 10, 20, 88),
                        );
                    }
                    ui.painter().rect_stroke(
                        resp.rect,
                        Rounding::same(12.0),
                        Stroke::new(
                            if resp.hovered() { 1.6_f32 } else { 1.0_f32 },
                            if resp.hovered() {
                                pal.primary
                            } else {
                                pal.border
                            },
                        ),
                    );
                    let pill_r = Rect::from_center_size(resp.rect.center(), Vec2::new(108.0, 26.0));
                    let zoom_resp = ui.interact(
                        pill_r,
                        Id::new(("photo_zoom", &msg.id, msg.ts)),
                        Sense::click(),
                    ).on_hover_cursor(CursorIcon::PointingHand);
                    ui.painter().rect_filled(
                        pill_r,
                        Rounding::same(13.0),
                        Color32::from_rgba_unmultiplied(15, 23, 42, 225),
                    );
                    ui.painter().text(
                        pill_r.center(),
                        egui::Align2::CENTER_CENTER,
                        "Phóng to ảnh",
                        FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                // "Tải về" chip in the top-right corner: on hover, or while a download exists.
                // Registered after the image so it sits on top for clicks.
                if let Some(target) = dl_target.as_ref() {
                    if dl_state.is_some() || ui.rect_contains_pointer(resp.rect) {
                        let btn = Rect::from_center_size(
                            Pos2::new(resp.rect.right() - 20.0, resp.rect.top() + 20.0),
                            Vec2::splat(28.0),
                        );
                        let c = DlColors {
                            muted: Color32::from_gray(200),
                            track: Color32::from_rgba_unmultiplied(255, 255, 255, 60),
                            accent: Color32::WHITE,
                            ok: Color32::from_rgb(134, 239, 172),
                            err: Color32::from_rgb(252, 165, 165),
                            warn: Color32::from_rgb(253, 230, 138),
                            btn_bg: Color32::from_rgba_unmultiplied(15, 23, 42, 190),
                            btn_hover: Color32::from_rgba_unmultiplied(15, 23, 42, 235),
                        };
                        let id = Id::new(("photo_dl", &msg.id, msg.ts));
                        if let Some(a) =
                            attachment_action_button(ui, btn, id, dl_state, target, &c)
                        {
                            body_action = Some(a);
                        }
                    }
                }
                if body_action.is_none() && (zoom_resp.clicked() || resp.clicked()) {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        message_id: msg.id.clone(),
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
                }
            } else {
                // Loading / Placeholder frame with visual image icon and loading feedback
                let disp_w = max_text_width.clamp(200.0, 280.0);
                let disp_h = 150.0_f32;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(disp_w, disp_h), Sense::click());
                let is_hover = resp.hovered();
                let bg_col = if is_hover {
                    text_col.linear_multiply(0.09)
                } else {
                    text_col.linear_multiply(0.05)
                };
                let stroke_col = if is_hover { pal.primary } else { pal.border };
                ui.painter().rect_filled(rect, Rounding::same(12.0), bg_col);
                ui.painter().rect_stroke(
                    rect,
                    Rounding::same(12.0),
                    Stroke::new(1.0_f32, stroke_col),
                );

                let icon_center = rect.center() - Vec2::new(0.0, 14.0);
                draw_icon_image(
                    ui.painter(),
                    icon_center,
                    26.0,
                    text_col.linear_multiply(0.45),
                );
                let label_text = if title_label != "Hình ảnh" {
                    title_label.clone()
                } else {
                    "Đang tải hình ảnh...".to_string()
                };
                ui.painter().text(
                    rect.center() + Vec2::new(0.0, 20.0),
                    egui::Align2::CENTER_CENTER,
                    label_text,
                    FontId::proportional(12.5),
                    text_col.linear_multiply(0.7),
                );

                if resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Đang tải ảnh · Bấm để xem ảnh phóng to")
                    .clicked()
                {
                    body_action = Some(BubbleAction::OpenImagePopup {
                        message_id: msg.id.clone(),
                        tex_key: key,
                        full_url: open_target,
                        title: title_label,
                        sender: sender_label,
                        time: msg.time.clone(),
                    });
                }
            }
            if let (Some(target), Some(state)) = (dl_target.as_ref(), dl_state) {
                if let Some(action) = render_photo_download_pill(ui, state, target, pal) {
                    body_action = Some(action);
                }
            }
            if let Some(cap) = extract_photo_user_caption(msg) {
                ui.add_space(5.0);
                if let Some(name) =
                    render_chat_markdown_block(ui, cap, text_col, pal, &mention_tokens)
                {
                    body_action = Some(BubbleAction::OpenMentionProfile(name));
                }
            }
            return body_action;
        }
    }
    // Video with a thumbnail: photo-style card with a play button. Click → QTzalo's own player.
    if let Some(thumb_url) = video_thumb_url(msg).filter(|_| !is_zalo_photo) {
        return render_video_thumb_card(
            ui,
            msg,
            thumb_url,
            pal,
            max_text_width,
            sticker_textures,
            loading_stickers,
            dl_target.as_ref(),
            dl_state,
        );
    }
    // Dedicated File / Video / Voice Attachment Card
    if is_file_or_video_message(msg) {
        let is_vid = is_video_message(msg);
        let is_voice = trimmed_text.starts_with("[Tin nhắn thoại]") && !is_vid;
        let (title_owned, sub_owned) = file_card_texts(msg);
        let title_str = title_owned.as_str();
        let sub_str = sub_owned.as_str();
        // File card: rounded icon tile + bold name + size line (never the URL or its domain).
        let (tile_bg, tile_fg) = if outgoing {
            (Color32::from_rgba_unmultiplied(255, 255, 255, 52), Color32::WHITE)
        } else {
            (pal.primary, Color32::WHITE)
        };
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
            ui.painter().rect_filled(tile, Rounding::same(10.0), tile_bg);
            draw_lucide_icon(
                ui.painter(),
                tile.center(),
                if is_vid {
                    lucide::FILM
                } else if is_voice {
                    lucide::MIC
                } else {
                    lucide::FILE
                },
                19.0,
                tile_fg,
            );
            let btn_w = if dl_target.is_some() { 32.0 + 10.0 } else { 0.0 };
            let text_w = (ui.available_width() - btn_w).max(40.0);
            ui.allocate_ui_with_layout(
                Vec2::new(text_w, 40.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(text_w);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add_space(1.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(title_str).size(14.0).color(text_col).strong(),
                        )
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(sub_str).size(12.0).color(meta_col))
                            .truncate(),
                    );
                },
            );
            if dl_target.is_some() {
                ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover()).0
            } else {
                Rect::NOTHING
            }
        });
        let dl_c = dl_colors(pal, outgoing);
        let mut click_rect = row.response.rect;
        if let Some(target) = dl_target.as_ref() {
            // Keep the card's own click off the download button.
            click_rect.max.x = row.inner.left() - 4.0;
            let id = Id::new(("file_dl", &msg.id, msg.ts));
            if let Some(a) = attachment_action_button(ui, row.inner, id, dl_state, target, &dl_c) {
                body_action = Some(a);
            }
        }
        let resp = ui.interact(
            click_rect,
            Id::new(("file_card", &msg.id, msg.ts)),
            Sense::click(),
        );
        // Click: a video plays in QTzalo's own window; a file / voice note is downloaded into
        // "Thư mục lưu file" and opened with its default app (never the web browser).
        let card_action = if is_vid {
            video_play_url(msg).map(|url| BubbleAction::OpenVideo {
                url: url.to_string(),
                title: video_title(msg),
            })
        } else {
            dl_target.as_ref().and_then(|target| match dl_state {
                Some(DownloadState::InProgress { .. }) => None,
                Some(DownloadState::Done(path)) => Some(BubbleAction::OpenDownloaded(path.clone())),
                // A cancelled / failed download is retried from scratch and opened when done.
                None
                | Some(DownloadState::Failed(_))
                | Some(DownloadState::Cancelled) => Some(BubbleAction::DownloadAndOpen {
                    url: target.0.clone(),
                    suggested_name: target.1.clone(),
                }),
            })
        };
        if card_action.is_some() {
            let tip = match (is_vid, dl_state) {
                (true, _) => "Bấm để xem video",
                (false, Some(DownloadState::Done(_))) => "Bấm để mở tệp đã tải",
                (false, _) => "Bấm để tải về và mở tệp",
            };
            if resp
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(tip)
                .clicked()
                && body_action.is_none()
            {
                body_action = card_action;
            }
        }
        if let (Some(target), Some(state)) = (dl_target.as_ref(), dl_state) {
            let w = ui.available_width();
            let dl_c = dl_colors(pal, outgoing);
            if let Some(a) = render_download_status(ui, state, target, w, &dl_c) {
                body_action = Some(a);
            }
        }
        return body_action;
    }

    let Some((caption, url, domain)) = extract_message_link_view(msg) else {
        let display_text = if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
            stripped
        } else {
            &msg.text
        };
        return render_chat_markdown_block(ui, display_text, text_col, pal, &mention_tokens)
            .map(BubbleAction::OpenMentionProfile);
    };

    let link_col = if outgoing {
        // Solid blue outgoing bubbles: links stay white (underlined via the domain line).
        pal.bubble_out_text
    } else if pal.is_dark {
        Color32::from_rgb(97, 175, 239) // #61AFEF Zalo/Atom clean link blue
    } else {
        pal.primary
    };

    // Auto-enrich link metadata if title or thumbnail is missing/expired
    let needs_meta_enrich = msg.link_title.is_none()
        || msg.link_thumb.is_none()
        || msg
            .link_thumb
            .as_deref()
            .map(|th| {
                th.contains("zpcloud.zdn.vn")
                    && !sticker_textures
                        .contains_key(&format!("thumb_{}", crate::crypto::md5_hex(th)))
            })
            .unwrap_or(false);
    if needs_meta_enrich {
        let meta_key = format!("linkmeta_{}", crate::crypto::md5_hex(&url));
        if loading_stickers.insert(meta_key) {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::ParseLinkPreview { url: url.clone() });
            }
        }
    }

    // 1. Optional user comment above the link
    if !caption.is_empty() && caption != url {
        body_action = render_chat_markdown_block(ui, &caption, text_col, pal, &mention_tokens)
            .map(BubbleAction::OpenMentionProfile);
        ui.add_space(4.0);
    }

    // 2. Optional compact thumbnail directly inside the bubble (no nested card box)
    if let Some(thumb_url) = msg.link_thumb.as_deref().filter(|u| !u.is_empty()) {
        let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
        if !sticker_textures.contains_key(&key)
            && (thumb_url.starts_with("http://") || thumb_url.starts_with("https://"))
            && loading_stickers.insert(key.clone())
        {
            if let Some(tx) = WORKER_TX.get() {
                let _ = tx.send(WorkerCmd::FetchStickerImage {
                    key: key.clone(),
                    url: thumb_url.to_string(),
                });
            }
        }
        if let Some(tex) = sticker_textures.get(&key) {
            let [tw, th] = tex.size();
            let aspect = if tw > 0 && th > 0 {
                (th as f32 / tw as f32).clamp(0.45, 0.85)
            } else {
                0.56
            };
            let thumb_w = max_text_width.min(260.0);
            let img_h = (thumb_w * aspect).min(132.0);
            let img = egui::Image::new(tex)
                .fit_to_exact_size(Vec2::new(thumb_w, img_h))
                .rounding(Rounding::same(6.0))
                .sense(Sense::click());
            if ui
                .add(img)
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text(&url)
                .clicked()
            {
                open_external_url(&url);
            }
            ui.add_space(5.0);
        }
    }

    // 3. Bold Title (exact Zalo PC style: no globe icon, no vertical accent bar, no nested border)
    let header_title = msg
        .link_title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != domain)
        .unwrap_or_else(|| {
            url.strip_prefix("https://")
                .or_else(|| url.strip_prefix("http://"))
                .unwrap_or(&url)
        });
    let title_resp = ui
        .add(
            egui::Label::new(
                RichText::new(truncate_chars(header_title, 95))
                    .size(13.8)
                    .color(text_col)
                    .strong(),
            )
            .wrap()
            .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(&url);
    if title_resp.clicked() {
        open_external_url(&url);
    }

    // 4. Optional short description (only if concise and distinct)
    let desc_opt = msg
        .link_desc
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && *d != header_title && *d != url && caption.is_empty());
    if let Some(desc) = desc_opt {
        ui.add_space(2.0);
        let d_resp = ui
            .add(
                egui::Label::new(
                    RichText::new(truncate_chars(desc, 110))
                        .size(12.3)
                        .color(meta_col),
                )
                .wrap()
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(&url);
        if d_resp.clicked() {
            open_external_url(&url);
        }
    }

    // 5. Clean Domain line (`docs.google.com`, `drive.google.com`, `tiktok.com`)
    ui.add_space(2.5);
    let dom_resp = ui
        .add(
            egui::Label::new(RichText::new(&domain).size(12.6).color(link_col))
                .sense(Sense::click()),
        )
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(&url);
    if dom_resp.clicked() {
        open_external_url(&url);
    }

    body_action
}

pub(super) fn is_zalo_file_or_video_message(msg: &ChatMessage) -> bool {
    let trimmed = msg.text.trim();
    matches!(
        msg.link_src.as_deref(),
        Some("Tập tin Zalo") | Some("Video Zalo")
    ) || trimmed.starts_with("[Tập tin]")
        || trimmed.starts_with("[Video]")
        || trimmed.starts_with("[Tin nhắn thoại]")
        || msg
            .link_url
            .as_deref()
            .is_some_and(|u| u.to_ascii_lowercase().contains("/renewmedia"))
}

pub(super) fn photo_primary_url(msg: &ChatMessage) -> Option<&str> {
    if let Some(th) = msg
        .link_thumb
        .as_deref()
        .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
    {
        return Some(th);
    }
    if let Some(lu) = msg
        .link_url
        .as_deref()
        .filter(|u| !u.is_empty() && crate::chat::is_zalo_photo_cdn_url(u))
    {
        return Some(lu);
    }
    let t = msg.text.trim();
    if crate::chat::is_zalo_photo_cdn_url(t) {
        return Some(t);
    }
    if let Some(pos) = t.find("https://").or_else(|| t.find("http://")) {
        let after = &t[pos..];
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        let candidate =
            after[..end].trim_end_matches(|c: char| matches!(c, ')' | ']' | ',' | ';' | '>'));
        if crate::chat::is_zalo_photo_cdn_url(candidate) {
            return Some(candidate);
        }
    }
    None
}

pub(super) fn is_zalo_photo_message(msg: &ChatMessage) -> bool {
    if is_sticker_message(msg).is_some() || is_zalo_file_or_video_message(msg) {
        return false;
    }
    // If link_url points to a non-image webpage (e.g. YouTube, Google Docs, News), it is a rich link card, not a photo message.
    if let Some(ref lu) = msg.link_url {
        if !lu.is_empty() && !crate::chat::is_zalo_photo_cdn_url(lu) {
            return false;
        }
    }
    let trimmed = msg.text.trim();
    msg.link_src.as_deref() == Some("Hình ảnh Zalo")
        || trimmed == "[Hình ảnh]"
        || trimmed.starts_with("[Hình ảnh] ")
        || msg
            .link_url
            .as_deref()
            .is_some_and(crate::chat::is_zalo_photo_cdn_url)
        || msg
            .link_thumb
            .as_deref()
            .is_some_and(crate::chat::is_zalo_photo_cdn_url)
        || crate::chat::is_zalo_photo_cdn_url(trimmed)
        || photo_primary_url(msg).is_some()
}

pub(super) fn extract_photo_user_caption(msg: &ChatMessage) -> Option<&str> {
    let is_valid_caption = |s: &str| -> bool {
        let t = s.trim();
        if t.is_empty() || t == "[Hình ảnh]" {
            return false;
        }
        let lower = t.to_ascii_lowercase();
        if [
            ".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".jfif", ".tiff",
        ]
        .iter()
        .any(|ext| lower.ends_with(ext))
        {
            return false;
        }
        !t.starts_with("http://")
            && !t.starts_with("https://")
            && !t.ends_with(".zdn.vn")
            && !t.ends_with(".zadn.vn")
            && !t.ends_with(".dlfl.vn")
            && !t.contains("photo-stal")
            && !t.contains("zpcloud")
            && !t.contains("zpg-r")
            && Some(t) != msg.link_src.as_deref()
    };
    if let Some(desc) = msg.link_desc.as_deref().map(str::trim) {
        if is_valid_caption(desc) {
            return Some(desc);
        }
    }
    let mut trimmed = msg.text.trim();
    if let Some(s) = trimmed.strip_prefix("[Đã thu hồi]") {
        trimmed = s.trim();
    }
    let stripped = trimmed
        .strip_prefix("[Hình ảnh]")
        .map(str::trim)
        .unwrap_or(trimmed);
    if is_valid_caption(stripped) {
        Some(stripped)
    } else {
        None
    }
}

pub(super) fn render_chat_bubble(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    prev_same_sender: bool,
    next_same_sender: bool,
    is_latest_outgoing: bool,
    is_group: bool,
    sender_profile_uid: Option<&str>,
    thread_name: &str,
    sender_avatar: Option<&str>,
    pal: &ThemePalette,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    emoji_textures: &HashMap<String, TextureHandle>,
    downloads: &HashMap<String, DownloadState>,
    idx: usize,
) -> Option<BubbleAction> {
    // Rows stored before media parsing (text = Zalo CDN URL) render as the photo / video they are.
    let legacy_view = legacy_media_view(msg);
    let msg = legacy_view.as_ref().unwrap_or(msg);
    let avail_w = ui.available_width();
    let max_bubble_width = (avail_w * 0.72).clamp(140.0, 520.0);
    let max_text_width = (max_bubble_width - 24.0).max(90.0);
    let has_sticker = is_sticker_message(msg).is_some();
    let call_card_opt = extract_call_card_view(msg);
    let is_call_card = call_card_opt.is_some();
    let is_photo_card = !has_sticker && is_photo_message(msg);
    // A video with a thumbnail is drawn like a photo (with a play button); without one it is a
    // file-style row.
    let is_video_thumb_card = !has_sticker && !is_photo_card && video_thumb_url(msg).is_some();
    let is_file_card = !has_sticker && !is_photo_card && !is_video_thumb_card && is_file_or_video_message(msg);
    let has_link_card = !has_sticker && !is_photo_card && !is_video_thumb_card && !is_file_card && extract_message_link_view(msg).is_some();
    let has_interactive_card = msg.poll.is_some()
        || is_call_card
        || has_link_card
        || is_photo_card
        || is_video_thumb_card
        || is_file_card;

    let show_timestamp = !msg.time.is_empty()
        && (!next_same_sender
            || is_latest_outgoing
            || has_interactive_card
            || !msg.reactions.is_empty()
            || msg.is_pinned
            || msg.is_starred);

    let photo_caption = if is_photo_card {
        extract_photo_user_caption(msg)
    } else {
        None
    };
    let has_photo_caption = photo_caption.is_some();
    let is_frameless_photo = ((is_photo_card && !has_photo_caption) || is_video_thumb_card)
        && msg.reply_to_text.is_none()
        && (msg.is_outgoing || !is_group);

    let sender_is_name = !msg.sender.is_empty()
        && msg.sender != "0"
        && !msg.sender.chars().all(|c| c.is_ascii_digit());
    let show_group_sender = !msg.is_outgoing && !prev_same_sender && is_group && sender_is_name;

    // Pre-measure exact content width for both outgoing and incoming bubbles so every bubble,
    // photo, and call card hugs its content tightly without horizontal overflow or inner/outer frame mismatch.
    let content_width = if msg.poll.is_some() {
        max_text_width.clamp(220.0, 360.0)
    } else if has_sticker {
        116.0_f32
    } else if is_call_card {
        max_text_width.min(256.0)
    } else if is_photo_card {
        let thumb_u = photo_primary_url(msg).unwrap_or("");
        let tkey = format!("thumb_{}", crate::crypto::md5_hex(thumb_u));
        let photo_w = if let Some(tex) = sticker_textures.get(&tkey) {
            let [tw, th] = tex.size();
            if tw <= 2 || th <= 2 {
                max_text_width.clamp(180.0, 260.0)
            } else {
                let tw_f = tw as f32;
                let th_f = th as f32;
                let max_w = max_text_width.clamp(220.0, 360.0);
                let max_h = 340.0_f32;
                let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
                (tw_f * scale).clamp(160.0, max_w).ceil()
            }
        } else {
            max_text_width.clamp(200.0, 280.0)
        };
        if let Some(cap_str) = photo_caption {
            let cap_w = ui.fonts(|f| {
                f.layout(
                    cap_str.to_string(),
                    FontId::proportional(14.2),
                    Color32::WHITE,
                    max_text_width,
                )
                .size()
                .x
            });
            photo_w.max(cap_w + 4.0).min(max_text_width).ceil()
        } else {
            photo_w
        }
    } else if is_video_thumb_card {
        let thumb = video_thumb_url(msg).unwrap_or("");
        let tex = sticker_textures.get(&format!("thumb_{}", crate::crypto::md5_hex(thumb)));
        visual_card_size(tex, max_text_width).x.ceil()
    } else if is_file_card {
        let (title, sub) = file_card_texts(msg);
        let measure = |s: &str, sz: f32| -> f32 {
            ui.fonts(|f| {
                f.layout_no_wrap(truncate_chars(s, 80).into_owned(), FontId::proportional(sz), Color32::WHITE)
                    .size()
                    .x
            })
        };
        // 40px icon tile + gap in front; downloadable cards also carry a 32px button + gap.
        let dl_btn_w = if attachment_download_target(msg).is_some() { 42.0 } else { 0.0 };
        let w = measure(&title, 14.0).max(measure(&sub, 12.0)) + 8.0 + 54.0 + dl_btn_w;
        let cap = max_text_width.min(340.0);
        let w = w.clamp(135.0_f32.min(cap), cap);
        // Room for the progress / "Đã tải · Mở · Mở thư mục" line under a download.
        if dl_btn_w > 0.0 {
            w.max(cap.min(230.0)).ceil()
        } else {
            w.ceil()
        }
    } else if has_link_card {
        let has_loaded_thumb = msg
            .link_thumb
            .as_deref()
            .filter(|u| !u.is_empty())
            .map(|u| sticker_textures.contains_key(&format!("thumb_{}", crate::crypto::md5_hex(u))))
            .unwrap_or(false);
        if has_loaded_thumb {
            max_text_width.min(260.0)
        } else {
            let (cap, url, dom) = extract_message_link_view(msg).unwrap_or_default();
            let title = msg
                .link_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    if !url.is_empty() {
                        &url
                    } else {
                        msg.text.trim()
                    }
                });
            let desc = msg.link_desc.as_deref().unwrap_or("");
            let measure = |s: &str, sz: f32| -> f32 {
                if s.is_empty() {
                    0.0
                } else {
                    ui.fonts(|f| {
                        f.layout(
                            truncate_chars(s, 110).into_owned(),
                            FontId::proportional(sz),
                            Color32::WHITE,
                            max_text_width.min(340.0),
                        )
                        .size()
                        .x
                    })
                }
            };
            let w = measure(title, 13.8)
                .max(measure(&cap, 13.8))
                .max(measure(desc, 12.3))
                .max(measure(&dom, 12.6))
                + 8.0;
            { let cap = max_text_width.min(340.0); w.clamp(135.0_f32.min(cap), cap).ceil() }
        }
    } else {
        let display_str = if msg.text.trim() == "[Tin nhắn đã thu hồi]" {
            "  Tin nhắn đã được thu hồi".to_string()
        } else if let Some(stripped) = msg.text.strip_prefix("[Đã thu hồi] ") {
            strip_markdown_syntax_for_width(stripped)
        } else {
            strip_markdown_syntax_for_width(&msg.text)
        };
        let text_w = ui.fonts(|f| {
            f.layout(
                display_str,
                FontId::proportional(14.2),
                Color32::WHITE,
                max_text_width,
            )
            .size()
            .x
        });
        let quote_w = match msg.reply_to_text.as_ref() {
            Some(qt) => {
                let q_body_w = ui.fonts(|f| {
                    f.layout(
                        truncate_chars(qt, 80).into_owned(),
                        FontId::proportional(12.0),
                        Color32::WHITE,
                        (max_text_width - 28.0).max(80.0),
                    )
                    .size()
                    .x
                });
                let who = msg
                    .reply_to_sender
                    .as_deref()
                    .filter(|sn| !sn.is_empty() && !sn.chars().all(|c| c.is_ascii_digit()))
                    .unwrap_or("Tin nhắn trích dẫn");
                let q_who_w = ui.fonts(|f| {
                    f.layout_no_wrap(who.to_string(), FontId::proportional(11.5), Color32::WHITE)
                        .size()
                        .x
                });
                q_body_w.max(q_who_w) + 30.0
            }
            None => 0.0_f32,
        };
        let sender_w = if show_group_sender {
            ui.fonts(|f| {
                f.layout_no_wrap(
                    truncate_chars(&msg.sender, 22).into_owned(),
                    FontId::proportional(12.0),
                    Color32::WHITE,
                )
                .size()
                .x
            }) + 8.0
        } else {
            0.0_f32
        };
        let marks_w = if !msg.reactions.is_empty() || msg.is_pinned || msg.is_starred {
            78.0_f32
        } else {
            0.0_f32
        };
        let ts_min_w = if show_timestamp && is_latest_outgoing && msg.is_outgoing {
            96.0_f32
        } else {
            44.0_f32
        };
        text_w
            .max(quote_w)
            .max(sender_w)
            .max(marks_w)
            .max(ts_min_w)
            .min(max_text_width)
            .ceil()
            + 4.0
    };

    let h_margin = if has_sticker || is_frameless_photo {
        2.0_f32
    } else {
        12.0_f32
    };
    let v_margin = if has_sticker || is_frameless_photo {
        2.0_f32
    } else {
        8.5_f32
    };
    let total_bubble_w = content_width + h_margin * 2.0;

    if msg.is_outgoing {
        let out_resp = ui.vertical(|ui| {
            let row_resp = ui.horizontal(|ui| {
                let left_pad = (ui.available_width() - total_bubble_w - 6.0).max(0.0);
                if left_pad > 0.0 {
                    ui.add_space(left_pad);
                }

                let fill = if has_sticker || is_frameless_photo {
                    Color32::TRANSPARENT
                } else if let Some(card) = &call_card_opt {
                    if card.is_missed_or_failed {
                        if pal.is_dark {
                            Color32::from_rgb(38, 23, 30)
                        } else {
                            Color32::from_rgb(254, 242, 242)
                        }
                    } else {
                        pal.bg_elevated
                    }
                } else {
                    pal.bubble_out_bg
                };
                let stroke = if has_sticker || is_frameless_photo {
                    Stroke::NONE
                } else if let Some(card) = &call_card_opt {
                    let border_col = if card.is_missed_or_failed {
                        Color32::from_rgb(239, 68, 68).linear_multiply(0.55)
                    } else {
                        Color32::from_rgb(34, 197, 94).linear_multiply(0.55)
                    };
                    Stroke::new(1.2_f32, border_col)
                } else if !pal.is_dark {
                    Stroke::new(1.0_f32, Color32::from_rgb(186, 214, 255))
                } else {
                    Stroke::new(1.0_f32, Color32::from_rgb(53, 76, 104))
                };
                let rounding = Rounding::same(8.0);

                let mut card_action = None;
                let mut quote_action = None;
                let mut reaction_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            quote_action = render_quote_block(ui, msg, pal, true, content_width);
                            if let Some(card) = &call_card_opt {
                                card_action = render_call_card_widget(ui, card, pal, content_width);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                            ) {
                                card_action = render_message_body(
                                    ui,
                                    msg,
                                    thread_name,
                                    pal,
                                    true,
                                    content_width,
                                    sticker_textures,
                                    loading_stickers,
                                    downloads,
                                );
                            }
                            if let Some(act) = render_reactions(ui, msg, pal, emoji_textures) {
                                reaction_action = Some(act);
                            }
                            if show_timestamp && !has_sticker && !is_frameless_photo {
                                ui.add_space(3.5);
                                let (status_str, status_col) = if is_latest_outgoing {
                                    match msg.delivery {
                                        crate::message_model::DeliveryState::Read => {
                                            (format!("{} · Đã xem", msg.time), pal.primary)
                                        }
                                        crate::message_model::DeliveryState::Pending => {
                                            (format!("{} · Đang gửi…", msg.time), pal.text_muted)
                                        }
                                        crate::message_model::DeliveryState::Failed => (
                                            format!("{} · Lỗi gửi", msg.time),
                                            Color32::from_rgb(239, 68, 68),
                                        ),
                                        _ => {
                                            (format!("{} · Đã nhận", msg.time), pal.bubble_out_meta)
                                        }
                                    }
                                } else {
                                    (msg.time.clone(), pal.bubble_out_meta)
                                };
                                ui.label(RichText::new(status_str).size(11.0).color(status_col));
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                if let Some(act) = render_hover_reaction_bar(
                    ui.ctx(),
                    fr.response.rect,
                    msg,
                    idx,
                    true,
                    pal,
                    emoji_textures,
                ) {
                    return Some(act);
                }
                reaction_action
                    .or(quote_action)
                    .or(card_action)
                    .or_else(|| {
                        bubble_context_menu(
                            ui,
                            &fr.response,
                            msg,
                            idx,
                            pal,
                            emoji_textures,
                            has_interactive_card,
                            "Bạn",
                        )
                    })
            });

            row_resp.inner
        });
        out_resp.inner
    } else {
        ui.horizontal_top(|ui| {
            let display_sender = thread_name;
            let sender_accent = avatar_color_for(display_sender, false);
            let mut sender_avatar_clicked = false;

            // Sender Avatar (only on first message of a consecutive group)
            if !prev_same_sender {
                let (av_rect, avatar_response) =
                    ui.allocate_exact_size(Vec2::new(30.0, 30.0), Sense::click());
                paint_user_avatar(
                    ui.painter(),
                    av_rect.center(),
                    15.0,
                    display_sender,
                    false,
                    sender_avatar,
                    sticker_textures,
                    loading_stickers,
                );
                sender_avatar_clicked = avatar_response
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text("Xem thông tin tài khoản")
                    .clicked();
            } else {
                ui.add_space(30.0);
            }

            let fill = if has_sticker || is_frameless_photo {
                Color32::TRANSPARENT
            } else if let Some(card) = &call_card_opt {
                if card.is_missed_or_failed {
                    if pal.is_dark {
                        Color32::from_rgb(38, 23, 30)
                    } else {
                        Color32::from_rgb(254, 242, 242)
                    }
                } else {
                    pal.bg_elevated
                }
            } else {
                pal.bubble_in_bg
            };
            let stroke = if has_sticker || is_frameless_photo {
                Stroke::NONE
            } else if let Some(card) = &call_card_opt {
                let border_col = if card.is_missed_or_failed {
                    Color32::from_rgb(239, 68, 68).linear_multiply(0.55)
                } else {
                    Color32::from_rgb(34, 197, 94).linear_multiply(0.55)
                };
                Stroke::new(1.2_f32, border_col)
            } else {
                Stroke::new(1.0_f32, pal.bubble_in_border)
            };
            let rounding = Rounding::same(8.0);

            ui.vertical(|ui| {
                ui.set_max_width(total_bubble_w);
                let mut card_action = None;
                let mut quote_action = None;
                let mut reaction_action = None;
                let fr = egui::Frame::none()
                    .fill(fill)
                    .stroke(stroke)
                    .rounding(rounding)
                    .inner_margin(Margin::symmetric(h_margin, v_margin))
                    .show(ui, |ui| {
                        ui.set_min_width(content_width);
                        ui.set_max_width(content_width);
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            if show_group_sender {
                                ui.label(
                                    RichText::new(truncate_chars(display_sender, 22))
                                        .color(sender_accent)
                                        .size(12.0)
                                        .strong(),
                                );
                                ui.add_space(2.0);
                            }
                            quote_action = render_quote_block(ui, msg, pal, false, content_width);
                            if let Some(card) = &call_card_opt {
                                card_action = render_call_card_widget(ui, card, pal, content_width);
                            } else if !render_sticker_widget(
                                ui,
                                msg,
                                sticker_textures,
                                loading_stickers,
                            ) {
                                card_action = render_message_body(
                                    ui,
                                    msg,
                                    thread_name,
                                    pal,
                                    false,
                                    content_width,
                                    sticker_textures,
                                    loading_stickers,
                                    downloads,
                                );
                            }
                            if let Some(act) = render_reactions(ui, msg, pal, emoji_textures) {
                                reaction_action = Some(act);
                            }
                            if show_timestamp && !has_sticker && !is_frameless_photo {
                                ui.add_space(3.5);
                                ui.label(
                                    RichText::new(&msg.time)
                                        .color(pal.bubble_in_meta)
                                        .size(11.0),
                                );
                            }
                        });
                    });
                if fr.response.double_clicked() {
                    return Some(BubbleAction::Reply);
                }
                if let Some(act) = render_hover_reaction_bar(
                    ui.ctx(),
                    fr.response.rect,
                    msg,
                    idx,
                    false,
                    pal,
                    emoji_textures,
                ) {
                    return Some(act);
                }
                reaction_action
                    .or(quote_action)
                    .or(card_action)
                    .or_else(|| {
                        bubble_context_menu(
                            ui,
                            &fr.response,
                            msg,
                            idx,
                            pal,
                            emoji_textures,
                            has_interactive_card,
                            display_sender,
                        )
                    })
            })
            .inner
            .or_else(|| {
                sender_avatar_clicked.then(|| {
                    BubbleAction::OpenSenderProfile(sender_profile_uid.map(str::to_owned))
                })
            })
        })
        .inner
    }
}

#[derive(Clone, PartialEq)]
pub(super) enum BubbleAction {
    Reply,
    Mention(String),
    OpenMentionProfile(String),
    OpenSenderProfile(Option<String>),
    JumpToQuoted(String),
    Forward,
    /// Open the "who reacted" dialog for this message.
    ShowReactions,
    React(&'static str),
    SelectMultiple,
    ViewDetails,
    TogglePin,
    ToggleStar,
    VoteGroupPoll {
        poll_id: u64,
        option_ids: Vec<u64>,
    },
    ScheduleReminder,
    DeleteForMe,
    RecallForEveryone,
    CallAgain {
        is_video: bool,
    },
    OpenImagePopup {
        message_id: String,
        tex_key: String,
        full_url: String,
        title: String,
        sender: String,
        time: String,
    },
    DownloadFile {
        url: String,
        file_name: String,
    },
    Download { url: String, suggested_name: String },
    /// Click on a file / voice card: download it, then open it with its default app.
    DownloadAndOpen { url: String, suggested_name: String },
    /// Click on a video card: play it in QTzalo's own window (`media_view.rs`).
    OpenVideo { url: String, title: String },
    /// "Mở" on a finished download.
    OpenDownloaded(std::path::PathBuf),
    RevealDownloaded(std::path::PathBuf),
}

/// Right-click menu on a chat bubble: reactions, reply, forward, copy, delete-for-me.
pub(super) const ICON_DOWNLOAD: &str = "\u{e0b2}";
/// Lucide `alert-circle`.
const ICON_ALERT: &str = "\u{e077}";
/// Lucide `rotate-cw`.
pub(super) const ICON_RETRY: &str = "\u{e149}";

fn is_photo_message(msg: &ChatMessage) -> bool {
    let t = msg.text.trim();
    msg.link_src.as_deref() == Some("Hình ảnh Zalo") || t == "[Hình ảnh]" || t.starts_with("[Hình ảnh] ")
}

fn is_file_or_video_message(msg: &ChatMessage) -> bool {
    let t = msg.text.trim();
    matches!(msg.link_src.as_deref(), Some("Tập tin Zalo") | Some("Video Zalo"))
        || t.starts_with("[Tập tin]")
        || t.starts_with("[Video]")
        || t.starts_with("[Tin nhắn thoại]")
}

fn is_video_message(msg: &ChatMessage) -> bool {
    msg.link_src.as_deref() == Some("Video Zalo") || msg.text.trim().starts_with("[Video]")
}

/// `u` when it is an `http(s)://` URL.
fn web_url_str(u: Option<&str>) -> Option<&str> {
    u.map(str::trim)
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

fn looks_like_url(s: &str) -> bool {
    let l = s.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("www.")
}

/// Thumbnail of a video message, when it has one.
pub(super) fn video_thumb_url(msg: &ChatMessage) -> Option<&str> {
    if !is_video_message(msg) || is_photo_message(msg) {
        return None;
    }
    web_url_str(msg.link_thumb.as_deref())
}

/// The video file of a video message.
pub(super) fn video_play_url(msg: &ChatMessage) -> Option<&str> {
    if !is_video_message(msg) {
        return None;
    }
    web_url_str(msg.link_url.as_deref())
}

/// Player window title for a video: its file name when known, else `Video`.
pub(super) fn video_title(msg: &ChatMessage) -> String {
    msg.link_title
        .as_deref()
        .map(clean_attachment_title)
        .filter(|t| !t.is_empty() && !looks_like_url(t))
        .or_else(|| {
            let t = clean_attachment_title(&msg.text);
            (!t.is_empty() && !looks_like_url(t)).then_some(t)
        })
        .unwrap_or("Video")
        .to_string()
}

/// `(title, subtitle)` of a file / video / voice card. Never a URL or a domain: the subtitle is
/// the size / duration line (`link_desc`) or a generic label.
pub(super) fn file_card_texts(msg: &ChatMessage) -> (String, String) {
    let is_vid = is_video_message(msg);
    let is_voice = !is_vid && msg.text.trim().starts_with("[Tin nhắn thoại]");
    let title = msg
        .link_title
        .as_deref()
        .map(clean_attachment_title)
        .filter(|t| !t.is_empty() && !looks_like_url(t))
        .or_else(|| {
            let t = clean_attachment_title(&msg.text);
            (!t.is_empty() && !looks_like_url(t)).then_some(t)
        })
        .unwrap_or(if is_vid {
            "Video"
        } else if is_voice {
            "Tin nhắn thoại"
        } else {
            "Tập tin"
        });
    let sub = msg
        .link_desc
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && !looks_like_url(d))
        .unwrap_or(if is_vid {
            "Video đính kèm"
        } else if is_voice {
            "Âm thanh đính kèm"
        } else {
            "Tập tin đính kèm"
        });
    (title.to_string(), sub.to_string())
}

/// Rows stored before media messages were parsed (`text` = a Zalo CDN URL or a file name, no
/// marker): a view of the row as the photo / video / file / voice card it is, instead of a link
/// card that opens the browser. `None` for every other message (see
/// [`crate::chat::legacy_media_kind`]).
pub(super) fn legacy_media_view(msg: &ChatMessage) -> Option<ChatMessage> {
    let kind = crate::chat::legacy_media_kind(msg)?;
    let mut m = msg.clone();
    let old_title = m.link_title.take();
    m.link_desc = None;
    match kind {
        crate::chat::MediaKind::Photo => {
            m.text = "[Hình ảnh]".into();
            m.link_src = Some("Hình ảnh Zalo".into());
            if web_url_str(m.link_thumb.as_deref()).is_none() {
                m.link_thumb = m.link_url.clone();
            }
        }
        crate::chat::MediaKind::Video => {
            m.text = "[Video]".into();
            m.link_src = Some("Video Zalo".into());
        }
        crate::chat::MediaKind::File => {
            // The file name: the old text when it is not the URL itself, else the old title.
            let text = msg.text.trim();
            let name = if !text.is_empty() && Some(text) != msg.link_url.as_deref().map(str::trim) {
                text.to_string()
            } else {
                old_title.unwrap_or_default()
            };
            m.text = if name.is_empty() { "[Tập tin]".into() } else { format!("[Tập tin] {name}") };
            m.link_title = (!name.is_empty()).then_some(name);
            m.link_src = Some("Tập tin Zalo".into());
        }
        crate::chat::MediaKind::Voice => {
            m.text = "[Tin nhắn thoại]".into();
            m.link_title = Some("Tin nhắn thoại".into());
            m.link_src = None;
        }
    }
    Some(m)
}

/// On-screen size of a photo / video thumbnail card (`tex` = the loaded thumbnail).
fn visual_card_size(tex: Option<&TextureHandle>, max_text_width: f32) -> Vec2 {
    let max_w = max_text_width.clamp(160.0, 270.0);
    let max_h = 260.0_f32;
    match tex {
        Some(tex) => {
            let [tw, th] = tex.size();
            let tw_f = (tw.max(1)) as f32;
            let th_f = (th.max(1)) as f32;
            let scale = (max_w / tw_f).min(max_h / th_f).min(1.0);
            Vec2::new((tw_f * scale).clamp(135.0, max_w), (th_f * scale).clamp(95.0, max_h))
        }
        None => Vec2::new(max_w.min(240.0), 135.0),
    }
}

/// Lucide `play`.
pub(super) const ICON_PLAY: &str = "\u{e13c}";

/// Video bubble with a thumbnail: the frame, a play button and the duration / size pill.
/// Click → [`BubbleAction::OpenVideo`] (QTzalo's own player window; never the browser).
#[allow(clippy::too_many_arguments)]
fn render_video_thumb_card(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    thumb_url: &str,
    pal: &ThemePalette,
    max_text_width: f32,
    sticker_textures: &HashMap<String, TextureHandle>,
    loading_stickers: &mut HashSet<String>,
    dl_target: Option<&(String, String)>,
    dl_state: Option<&DownloadState>,
) -> Option<BubbleAction> {
    let mut action = None;
    let key = format!("thumb_{}", crate::crypto::md5_hex(thumb_url));
    if !sticker_textures.contains_key(&key) && loading_stickers.insert(key.clone()) {
        if let Some(tx) = WORKER_TX.get() {
            let _ = tx.send(WorkerCmd::FetchStickerImage {
                key: key.clone(),
                url: thumb_url.to_string(),
            });
        }
    }
    let tex = sticker_textures.get(&key);
    let (rect, resp) = ui.allocate_exact_size(visual_card_size(tex, max_text_width), Sense::click());
    let resp = resp
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Bấm để xem video");
    let hovered = resp.hovered();
    let rounding = Rounding::same(12.0);
    let painter = ui.painter().clone();
    match tex {
        Some(t) => egui::Image::new(t).rounding(rounding).paint_at(ui, rect),
        None => {
            painter.rect_filled(rect, rounding, Color32::from_rgb(30, 35, 46));
        }
    }
    painter.rect_filled(
        rect,
        rounding,
        Color32::from_rgba_premultiplied(0, 0, 0, if hovered { 72 } else { 30 }),
    );
    // Play button.
    let c = rect.center();
    let r = 23.0;
    painter.circle_filled(
        c,
        r,
        if hovered {
            pal.primary
        } else {
            Color32::from_rgba_unmultiplied(15, 23, 42, 200)
        },
    );
    painter.circle_stroke(c, r, Stroke::new(1.5_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 200)));
    draw_lucide_icon(&painter, c + Vec2::new(1.5, 0.0), ICON_PLAY, 21.0, Color32::WHITE);
    // Duration / size pill (bottom-left), e.g. `0:42 · 8,6 MB`.
    let meta = msg
        .link_desc
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty() && !looks_like_url(d))
        .unwrap_or("Video");
    let galley = painter.layout_no_wrap(
        truncate_chars(meta, 28).into_owned(),
        FontId::proportional(11.0),
        Color32::WHITE,
    );
    let pill = Rect::from_min_size(
        Pos2::new(rect.left() + 8.0, rect.bottom() - 8.0 - 19.0),
        Vec2::new(galley.size().x + 26.0, 19.0),
    );
    painter.rect_filled(pill, Rounding::same(9.5), Color32::from_rgba_unmultiplied(0, 0, 0, 150));
    draw_lucide_icon(&painter, Pos2::new(pill.left() + 10.0, pill.center().y), lucide::VIDEO, 11.0, Color32::WHITE);
    painter.galley(
        Pos2::new(pill.left() + 19.0, pill.center().y - galley.size().y / 2.0),
        galley,
        Color32::WHITE,
    );
    painter.rect_stroke(
        rect,
        rounding,
        Stroke::new(
            if hovered { 1.6_f32 } else { 1.0_f32 },
            if hovered { pal.primary } else { pal.border },
        ),
    );
    // "Tải về" chip in the top-right corner, on hover or while a download exists.
    if let Some(target) = dl_target {
        if dl_state.is_some() || ui.rect_contains_pointer(rect) {
            let btn = Rect::from_center_size(
                Pos2::new(rect.right() - 20.0, rect.top() + 20.0),
                Vec2::splat(28.0),
            );
            let colors = DlColors {
                muted: Color32::from_gray(200),
                track: Color32::from_rgba_unmultiplied(255, 255, 255, 60),
                accent: Color32::WHITE,
                ok: Color32::from_rgb(134, 239, 172),
                err: Color32::from_rgb(252, 165, 165),
                warn: Color32::from_rgb(253, 230, 138),
                btn_bg: Color32::from_rgba_unmultiplied(15, 23, 42, 190),
                btn_hover: Color32::from_rgba_unmultiplied(15, 23, 42, 235),
            };
            let id = Id::new(("video_dl", &msg.id, msg.ts));
            action = attachment_action_button(ui, btn, id, dl_state, target, &colors);
        }
    }
    if action.is_none() && resp.clicked() {
        action = video_play_url(msg).map(|url| BubbleAction::OpenVideo {
            url: url.to_string(),
            title: video_title(msg),
        });
    }
    if let (Some(target), Some(state)) = (dl_target, dl_state) {
        if let Some(a) = render_photo_download_pill(ui, state, target, pal) {
            action = Some(a);
        }
    }
    action
}

/// Strip the `[Tập tin]` / `[Video]` / `[Hình ảnh]` placeholders from an attachment title.
fn clean_attachment_title(raw: &str) -> &str {
    let mut t = raw.trim();
    for p in ["[Tập tin]", "[Video]", "[Hình ảnh]", "[Tin nhắn thoại]"] {
        t = t.trim_start_matches(p).trim();
    }
    t
}

/// `(url, suggested_name)` to download for a file / video / photo message: the original
/// (`link_url`, else the photo thumbnail). The name is untrusted and may be empty; the worker
/// sanitises it and falls back to the URL / a timestamp name.
pub(super) fn attachment_download_target(msg: &ChatMessage) -> Option<(String, String)> {
    let photo = is_photo_message(msg);
    if !photo && !is_file_or_video_message(msg) {
        return None;
    }
    let url = msg
        .link_url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .or(if photo { msg.link_thumb.as_deref() } else { None })
        .map(str::trim)
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))?
        .to_string();
    let name = msg
        .link_title
        .as_deref()
        .map(clean_attachment_title)
        .filter(|t| !t.is_empty())
        .or_else(|| {
            let t = clean_attachment_title(&msg.text);
            (!photo && !t.is_empty()).then_some(t)
        })
        .unwrap_or("")
        .to_string();
    Some((url, name))
}

/// Progress text: `45% · 10,8 MB / 24,1 MB` (`compact`: `Đang tải 45%`) or `Đang tải… 3,20 MB`.
pub(super) fn download_progress_label(state: &DownloadState, compact: bool) -> String {
    let DownloadState::InProgress { received, total } = state else {
        return String::new();
    };
    match (state.fraction(), total) {
        (Some(f), Some(_)) if compact => format!("Đang tải {:.0}%", f * 100.0),
        (Some(f), Some(t)) => format!(
            "{:.0}% · {} / {}",
            f * 100.0,
            crate::download::format_bytes(*received),
            crate::download::format_bytes(*t)
        ),
        _ if *received == 0 => "Đang tải…".to_string(),
        _ => format!("Đang tải… {}", crate::download::format_bytes(*received)),
    }
}

/// Ring progress (or a spinning arc when the size is unknown) around `center`.
fn paint_progress_ring(
    ui: &egui::Ui,
    center: Pos2,
    radius: f32,
    fraction: Option<f32>,
    track: Color32,
    fill: Color32,
) {
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new(2.2_f32, track));
    let (start, sweep) = match fraction {
        Some(f) => (-std::f32::consts::FRAC_PI_2, f.clamp(0.0, 1.0) * std::f32::consts::TAU),
        None => {
            let t = ui.input(|i| i.time) as f32;
            (t * 5.0, std::f32::consts::FRAC_PI_2 * 1.2)
        }
    };
    if sweep <= 0.01 {
        return;
    }
    let steps = ((sweep / std::f32::consts::TAU) * 48.0).ceil().max(4.0) as usize;
    let pts: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = start + sweep * (i as f32 / steps as f32);
            center + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(pts, Stroke::new(2.2_f32, fill)));
}

/// Colours for download widgets inside a bubble (white on the blue outgoing bubble).
struct DlColors {
    muted: Color32,
    track: Color32,
    accent: Color32,
    ok: Color32,
    err: Color32,
    /// Softer warning tint for the "Đã huỷ" state (never as loud as `err`).
    warn: Color32,
    btn_bg: Color32,
    btn_hover: Color32,
}

fn dl_colors(pal: &ThemePalette, outgoing: bool) -> DlColors {
    if outgoing {
        DlColors {
            muted: pal.bubble_out_meta,
            track: Color32::from_rgba_unmultiplied(0, 20, 80, 70),
            accent: Color32::WHITE,
            ok: Color32::WHITE,
            err: Color32::WHITE,
            // Readable on the solid blue bubble without competing with the white text.
            warn: Color32::from_rgb(255, 224, 153),
            btn_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 40),
            btn_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 72),
        }
    } else {
        DlColors {
            muted: pal.bubble_in_meta,
            accent: if pal.is_dark { pal.primary_hover } else { pal.primary },
            track: pal.border,
            ok: pal.accent_online,
            err: pal.danger,
            warn: pal.warning,
            btn_bg: pal.bg_hover,
            btn_hover: pal.bg_selected,
        }
    }
}

/// Ask the worker to stop an in-flight download of `url` (`WorkerCmd::CancelDownload`).
///
/// The worker keeps one [`crate::download::CancelFlag`] per URL; it stops at the next chunk,
/// deletes the `.part` file and answers `GuiEvent::DownloadCancelled` (or `VideoCancelled` when
/// the URL is a video being prepared for playback). The GUI then shows `DownloadState::Cancelled`
/// with a "Tải lại" button. Without a worker (`--demo`) there is nothing to stop.
fn cancel_download(url: &str) {
    if let Some(tx) = WORKER_TX.get() {
        let _ = tx.send(WorkerCmd::CancelDownload {
            url: url.to_string(),
        });
    }
}

/// `true` when a "Huỷ" click can reach a background worker (offline demo has none).
fn download_cancel_available() -> bool {
    !is_demo_mode() && WORKER_TX.get().is_some()
}

/// Round action button for an attachment card: download / progress (+ cancel) / open / retry.
///
/// Returns the action for a click. While a download runs the same control shows the progress
/// ring and a ✕ in the middle; a click sends `WorkerCmd::CancelDownload` and returns `None`
/// (the card turns into "Đã huỷ · Tải lại" when `GuiEvent::DownloadCancelled` arrives).
fn attachment_action_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    state: Option<&DownloadState>,
    target: &(String, String),
    c: &DlColors,
) -> Option<BubbleAction> {
    let resp = ui.interact(rect, id, Sense::click());
    let center = rect.center();
    let r = rect.width() * 0.5;
    let can_cancel = download_cancel_available();
    let bg = if resp.hovered() { c.btn_hover } else { c.btn_bg };
    ui.painter().circle_filled(center, r, bg);
    let icon_sz = (r * 1.05).max(12.0);
    let tip = match state {
        None => {
            draw_lucide_icon(ui.painter(), center, ICON_DOWNLOAD, icon_sz, c.accent);
            "Tải về thư mục lưu file".to_string()
        }
        Some(s @ DownloadState::InProgress { .. }) => {
            paint_progress_ring(ui, center, r - 5.0, s.fraction(), c.track, c.accent);
            // One control both shows the progress and cancels: the ring stays, the ✕ is the
            // click target (a second, separate button would not fit the 28–32 px chip).
            if can_cancel {
                draw_lucide_icon(
                    ui.painter(),
                    center,
                    lucide::X,
                    (icon_sz * 0.62).max(9.0),
                    c.warn,
                );
            }
            ui.ctx().request_repaint_after(Duration::from_millis(60));
            let label = download_progress_label(s, false);
            if can_cancel {
                format!("{label}\nBấm để huỷ tải")
            } else {
                format!("{label}\nChế độ demo: không huỷ được tải giả")
            }
        }
        Some(DownloadState::Done(path)) => {
            draw_lucide_icon(ui.painter(), center, lucide::CHECK, icon_sz, c.ok);
            format!("Đã tải · Bấm để mở\n{}", path.display())
        }
        Some(DownloadState::Failed(err)) => {
            draw_lucide_icon(ui.painter(), center, ICON_RETRY, icon_sz * 0.95, c.err);
            format!("Tải về thất bại: {err}\nBấm để thử lại")
        }
        Some(DownloadState::Cancelled) => {
            draw_lucide_icon(ui.painter(), center, ICON_RETRY, icon_sz * 0.95, c.warn);
            "Đã huỷ tải · Bấm để tải lại".to_string()
        }
    };
    let resp = resp
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tip);
    if !resp.clicked() {
        return None;
    }
    match state {
        Some(DownloadState::InProgress { .. }) => {
            if can_cancel {
                cancel_download(&target.0);
            }
            None
        }
        Some(DownloadState::Done(path)) => Some(BubbleAction::OpenDownloaded(path.clone())),
        None | Some(DownloadState::Failed(_)) | Some(DownloadState::Cancelled) => {
            Some(BubbleAction::Download {
                url: target.0.clone(),
                suggested_name: target.1.clone(),
            })
        }
    }
}

/// Small clickable text link used in download status lines.
fn dl_link(ui: &mut egui::Ui, text: &str, col: Color32, tip: &str) -> bool {
    ui.add(
        egui::Label::new(RichText::new(text).size(12.0).color(col).strong()).sense(Sense::click()),
    )
    .on_hover_cursor(CursorIcon::PointingHand)
    .on_hover_text(tip)
    .clicked()
}

/// Status under a file / video card: progress bar, "Đã tải · Mở · Mở thư mục" or the error with
/// "Thử lại".
fn render_download_status(
    ui: &mut egui::Ui,
    state: &DownloadState,
    target: &(String, String),
    width: f32,
    c: &DlColors,
) -> Option<BubbleAction> {
    let mut action = None;
    ui.add_space(7.0);
    match state {
        DownloadState::InProgress { .. } => {
            let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 4.0), Sense::hover());
            ui.painter()
                .rect_filled(bar, Rounding::same(2.0), c.track);
            let fill_rect = match state.fraction() {
                Some(f) => Rect::from_min_size(bar.min, Vec2::new(bar.width() * f, bar.height())),
                None => {
                    // Indeterminate: a 30% segment sliding across the track.
                    let t = (ui.input(|i| i.time) as f32 * 0.8).fract();
                    let seg = bar.width() * 0.3;
                    let x0 = bar.left() - seg + (bar.width() + seg) * t;
                    Rect::from_min_max(
                        Pos2::new(x0.max(bar.left()), bar.top()),
                        Pos2::new((x0 + seg).min(bar.right()), bar.bottom()),
                    )
                }
            };
            if fill_rect.width() > 0.5 {
                ui.painter().rect_filled(fill_rect, Rounding::same(2.0), c.accent);
            }
            ui.ctx().request_repaint_after(Duration::from_millis(60));
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new(download_progress_label(state, false))
                        .size(11.5)
                        .color(c.muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if download_cancel_available()
                        && dl_link(ui, "Huỷ", c.warn, "Huỷ tải tệp này")
                    {
                        cancel_download(&target.0);
                    }
                });
            });
        }
        DownloadState::Done(path) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(
                    RichText::new(format!("{} Đã tải", lucide::CHECK))
                        .size(12.0)
                        .color(c.ok)
                        .strong(),
                );
                if dl_link(ui, "Mở", c.accent, "Mở tệp đã tải") {
                    action = Some(BubbleAction::OpenDownloaded(path.clone()));
                }
                if dl_link(ui, "Mở thư mục", c.accent, &path.display().to_string()) {
                    action = Some(BubbleAction::RevealDownloaded(path.clone()));
                }
            });
        }
        DownloadState::Failed(err) => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let retry_w = 56.0;
                ui.add_sized(
                    Vec2::new((width - retry_w).max(60.0), 16.0),
                    egui::Label::new(
                        RichText::new(format!("{ICON_ALERT} {err}"))
                            .size(11.8)
                            .color(c.err),
                    )
                    .truncate(),
                )
                .on_hover_text(err);
                if dl_link(ui, "Thử lại", c.accent, "Tải lại tệp này") {
                    action = Some(BubbleAction::Download {
                        url: target.0.clone(),
                        suggested_name: target.1.clone(),
                    });
                }
            });
        }
        DownloadState::Cancelled => {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.label(
                    RichText::new(format!("{} Đã huỷ tải", lucide::X))
                        .size(12.0)
                        .color(c.warn)
                        .strong(),
                );
                if dl_link(ui, "Tải lại", c.accent, "Tải lại tệp này") {
                    action = Some(BubbleAction::Download {
                        url: target.0.clone(),
                        suggested_name: target.1.clone(),
                    });
                }
            });
        }
    }
    action
}

/// Compact status pill under a photo (readable on both bubbles and the bare canvas).
fn render_photo_download_pill(
    ui: &mut egui::Ui,
    state: &DownloadState,
    target: &(String, String),
    pal: &ThemePalette,
) -> Option<BubbleAction> {
    let mut action = None;
    ui.add_space(5.0);
    egui::Frame::none()
        .fill(pal.bg_elevated)
        .stroke(Stroke::new(1.0_f32, pal.border))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(9.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let accent = if pal.is_dark { pal.primary_hover } else { pal.primary };
                match state {
                    DownloadState::InProgress { .. } => {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        paint_progress_ring(ui, r.center(), 5.5, state.fraction(), pal.border, accent);
                        ui.ctx().request_repaint_after(Duration::from_millis(60));
                        ui.label(
                            RichText::new(download_progress_label(state, true))
                                .size(11.5)
                                .color(pal.text_secondary),
                        )
                        .on_hover_text(download_progress_label(state, false));
                        if download_cancel_available()
                            && dl_link(ui, "Huỷ", pal.warning, "Huỷ tải ảnh này")
                        {
                            cancel_download(&target.0);
                        }
                    }
                    DownloadState::Done(path) => {
                        ui.label(
                            RichText::new(format!("{} Đã tải", lucide::CHECK))
                                .size(11.5)
                                .color(pal.accent_online)
                                .strong(),
                        );
                        if dl_link(ui, "Mở", accent, "Mở ảnh đã tải") {
                            action = Some(BubbleAction::OpenDownloaded(path.clone()));
                        }
                        if dl_link(ui, "Thư mục", accent, &path.display().to_string()) {
                            action = Some(BubbleAction::RevealDownloaded(path.clone()));
                        }
                    }
                    DownloadState::Failed(err) => {
                        ui.label(
                            RichText::new(format!("{ICON_ALERT} Lỗi tải"))
                                .size(11.5)
                                .color(pal.danger),
                        )
                        .on_hover_text(err);
                        if dl_link(ui, "Thử lại", accent, err) {
                            action = Some(BubbleAction::Download {
                                url: target.0.clone(),
                                suggested_name: target.1.clone(),
                            });
                        }
                    }
                    DownloadState::Cancelled => {
                        ui.label(
                            RichText::new(format!("{} Đã huỷ tải", lucide::X))
                                .size(11.5)
                                .color(pal.warning)
                                .strong(),
                        );
                        if dl_link(ui, "Tải lại", accent, "Tải lại ảnh này") {
                            action = Some(BubbleAction::Download {
                                url: target.0.clone(),
                                suggested_name: target.1.clone(),
                            });
                        }
                    }
                }
            });
        });
    action
}

impl ZaloGuiApp {
    /// Start downloading an attachment into "Thư mục lưu file" (`settings.download_dir`).
    /// Ignored while the same URL is already downloading; the offline demo only shows a toast.
    pub(super) fn start_attachment_download(
        &mut self,
        url: String,
        suggested_name: String,
        thread_id: String,
    ) {
        if matches!(self.downloads.get(&url), Some(DownloadState::InProgress { .. })) {
            return;
        }
        // The map is keyed by URL for the whole session. `Done` entries must stay (their path is
        // what "Mở"/"Mở thư mục" hands to the OS) and can only be evicted with a timestamp /
        // visibility reference owned by `gui.rs`; the terminal "Tải lại" hints are safe to drop
        // once there are a lot of them. Sorting keeps the eviction deterministic.
        const MAX_STALE_ENTRIES: usize = 64;
        let mut stale: Vec<String> = self
            .downloads
            .iter()
            .filter(|(u, s)| {
                u.as_str() != url
                    && matches!(s, DownloadState::Cancelled | DownloadState::Failed(_))
            })
            .map(|(u, _)| u.clone())
            .collect();
        if stale.len() > MAX_STALE_ENTRIES {
            stale.sort();
            for old in stale.iter().take(stale.len() - MAX_STALE_ENTRIES) {
                self.downloads.remove(old);
            }
        }
        let tx = if is_demo_mode() { None } else { WORKER_TX.get() };
        let Some(tx) = tx else {
            self.show_toast("Chế độ demo: không tải tệp thật.");
            return;
        };
        let cmd = WorkerCmd::DownloadAttachment {
            url: url.clone(),
            suggested_name,
            thread_id,
            dest_dir: self.settings.download_dir.clone(),
        };
        if tx.send(cmd).is_ok() {
            self.downloads.insert(
                url,
                DownloadState::InProgress {
                    received: 0,
                    total: None,
                },
            );
            self.status_msg = "Đang tải về…".into();
        } else {
            self.show_toast("Không thể bắt đầu tải về (tiến trình nền đã dừng).");
        }
    }

    /// "Mở" on a finished download: executables / scripts are only revealed in their folder.
    pub(super) fn open_downloaded_path(&mut self, path: &std::path::Path) {
        if is_demo_mode() {
            self.show_toast("Chế độ demo: không mở tệp thật.");
            return;
        }
        if !path.exists() {
            self.show_toast("Không tìm thấy tệp — có thể đã bị di chuyển hoặc xoá.");
            return;
        }
        if crate::download::open_downloaded(path) == crate::download::OpenOutcome::RevealedOnly {
            self.show_toast("Tệp thực thi — chỉ mở thư mục chứa tệp để bạn tự kiểm tra.");
        }
    }

    /// "Mở thư mục" on a finished download.
    pub(super) fn reveal_downloaded_path(&mut self, path: &std::path::Path) {
        if is_demo_mode() {
            self.show_toast("Chế độ demo: không mở thư mục thật.");
            return;
        }
        crate::download::reveal_in_folder(path);
    }
}


fn menu_item(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    text_color: Color32,
) -> egui::Response {
    let text = RichText::new(format!("{icon}   {label}"))
        .size(13.0)
        .color(text_color);
    let button = egui::Button::new(text)
        .fill(Color32::TRANSPARENT)
        .min_size(Vec2::new(175.0, 26.0));
    ui.add(button)
}

/// Context menu for message actions, opened by right-click or the hover-bar ellipsis.
pub(super) fn bubble_context_menu(
    ui: &mut egui::Ui,
    bubble: &egui::Response,
    msg: &ChatMessage,
    idx: usize,
    pal: &ThemePalette,
    _emoji_textures: &HashMap<String, TextureHandle>,
    _has_interactive_card: bool,
    display_sender: &str,
) -> Option<BubbleAction> {
    let ctx_menu_key = Id::new("active_bubble_context_menu");
    let open_time_key = Id::new("bubble_context_menu_open_time");
    let this_key = format!("{}_{}", msg.id, msg.ts);

    let pointer_pos = ui.ctx().input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let row_rect = Rect::from_min_max(
        Pos2::new(ui.clip_rect().left().max(0.0), bubble.rect.min.y - 2.0),
        Pos2::new(ui.clip_rect().right(), bubble.rect.max.y + 2.0),
    );
    let in_row = pointer_pos.map_or(false, |p| row_rect.contains(p));

    let secondary_clicked = ui.ctx().input(|i| {
        i.pointer.button_clicked(egui::PointerButton::Secondary)
            || (i.pointer.button_clicked(egui::PointerButton::Primary)
                && (i.modifiers.ctrl || i.modifiers.command))
    });

    if in_row && secondary_clicked {
        let click_pos = pointer_pos.unwrap_or(bubble.rect.left_top());
        let current_time = ui.input(|i| i.time);
        ui.ctx().data_mut(|d| {
            d.insert_temp(ctx_menu_key, (this_key.clone(), click_pos, idx));
            d.insert_temp(open_time_key, current_time);
            d.remove::<String>(Id::new("active_hover_reaction_bubble"));
        });
        ui.ctx().request_repaint();
    }

    let active_menu: Option<(String, Pos2, usize)> = ui.ctx().data(|d| d.get_temp(ctx_menu_key));
    let is_this_active = active_menu
        .as_ref()
        .map_or(false, |(k, _, _)| k == &this_key);

    if !is_this_active {
        return None;
    }

    let (_, menu_pos, _) = active_menu.unwrap();
    let mut action = None;
    let mut close_menu = false;

    let screen = ui.ctx().screen_rect();
    let menu_w = 190.0_f32;
    let menu_h = 310.0_f32;
    let pos = Pos2::new(
        menu_pos.x.clamp(
            screen.left() + 8.0,
            (screen.right() - menu_w - 8.0).max(screen.left() + 8.0),
        ),
        menu_pos.y.clamp(
            screen.top() + 8.0,
            (screen.bottom() - menu_h - 8.0).max(screen.top() + 8.0),
        ),
    );

    egui::Area::new(Id::new(("active_ctx_menu_area", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::none()
                .fill(if pal.is_dark {
                    Color32::from_rgb(33, 37, 43)
                } else {
                    Color32::WHITE
                })
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(8.0))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 4.0),
                    blur: 16.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(if pal.is_dark { 130 } else { 45 }),
                })
                .inner_margin(Margin::symmetric(6.0, 6.0))
                .show(ui, |ui| {
                    ui.set_min_width(menu_w);
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);

                    if menu_item(ui, lucide::COPY, "Copy tin nhắn", pal.text_primary).clicked() {
                        ui.ctx().copy_text(msg.text.clone());
                        close_menu = true;
                    }

                    let pin_label = if msg.is_pinned {
                        "Bỏ ghim tin nhắn"
                    } else {
                        "Ghim tin nhắn"
                    };
                    if menu_item(ui, lucide::PIN, pin_label, pal.text_primary).clicked() {
                        action = Some(BubbleAction::TogglePin);
                        close_menu = true;
                    }

                    let star_label = if msg.is_starred {
                        "Bỏ đánh dấu tin nhắn"
                    } else {
                        "Đánh dấu tin nhắn"
                    };
                    if menu_item(ui, lucide::STAR, star_label, pal.text_primary).clicked() {
                        action = Some(BubbleAction::ToggleStar);
                        close_menu = true;
                    }

                    if menu_item(ui, lucide::CHECK_CHECK, "Chọn nhiều tin nhắn", pal.text_primary).clicked() {
                        action = Some(BubbleAction::SelectMultiple);
                        close_menu = true;
                    }
                    if menu_item(ui, lucide::INFO, "Xem chi tiết", pal.text_primary).clicked() {
                        action = Some(BubbleAction::ViewDetails);
                        close_menu = true;
                    }

                    ui.menu_button("Tùy chọn khác", |ui| {
                        if menu_item(ui, lucide::QUOTE, "Trả lời", pal.text_primary).clicked() {
                            action = Some(BubbleAction::Reply);
                            close_menu = true;
                        }
                        if menu_item(ui, lucide::FORWARD, "Chia sẻ", pal.text_primary).clicked() {
                            action = Some(BubbleAction::Forward);
                            close_menu = true;
                        }
                        let mention_target = display_sender.trim();
                        if !msg.is_outgoing && !mention_target.is_empty() && mention_target != "Bạn" {
                            if menu_item(ui, "@", &format!("Nhắc đến @{mention_target}"), pal.text_primary).clicked() {
                                action = Some(BubbleAction::Mention(mention_target.to_string()));
                                close_menu = true;
                            }
                        }
                        if menu_item(ui, lucide::BELL, "Hẹn giờ / Nhắc hẹn", pal.text_primary).clicked() {
                            action = Some(BubbleAction::ScheduleReminder);
                            close_menu = true;
                        }
                        if let Some(url) = video_play_url(msg) {
                            if menu_item(ui, ICON_PLAY, "Xem video", pal.text_primary).clicked() {
                                action = Some(BubbleAction::OpenVideo { url: url.to_string(), title: video_title(msg) });
                                close_menu = true;
                            }
                        }
                        if let Some((url, suggested_name)) = attachment_download_target(msg) {
                            if menu_item(ui, lucide::PAPERCLIP, "Tải về và theo dõi", pal.text_primary).clicked() {
                                action = Some(BubbleAction::Download { url, suggested_name });
                                close_menu = true;
                            }
                        }
                        if is_zalo_photo_message(msg) {
                            if let Some(thumb_url) = photo_primary_url(msg) {
                                if menu_item(ui, lucide::SEARCH, "Xem ảnh phóng to", pal.text_primary).clicked() {
                                    action = Some(BubbleAction::OpenImagePopup {
                                        message_id: msg.id.clone(),
                                        tex_key: format!("thumb_{}", crate::crypto::md5_hex(thumb_url)),
                                        full_url: msg.link_url.clone().filter(|url| !url.is_empty()).unwrap_or_else(|| thumb_url.to_string()),
                                        title: msg.link_title.clone().filter(|title| {
                                            !title.is_empty() && !title.starts_with("http://") && !title.starts_with("https://")
                                        }).unwrap_or_else(|| "Hình ảnh".to_string()),
                                        sender: if msg.is_outgoing { "Bạn".to_string() } else { msg.sender.clone() },
                                        time: msg.time.clone(),
                                    });
                                    close_menu = true;
                                }
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Thêm cảm xúc").size(11.5).color(pal.text_muted));
                        ui.horizontal_wrapped(|ui| {
                            for (code, _, tooltip) in REACTION_LIST {
                                if ui.button(code).on_hover_text(tooltip).clicked() {
                                    action = Some(BubbleAction::React(code));
                                    close_menu = true;
                                }
                            }
                        });
                        if msg.is_outgoing
                            && msg.text.trim() != "[Tin nhắn đã thu hồi]"
                            && !msg.text.starts_with("[Đã thu hồi]")
                            && menu_item(ui, lucide::BAN, "Thu hồi", Color32::from_rgb(249, 115, 22)).clicked()
                        {
                            action = Some(BubbleAction::RecallForEveryone);
                            close_menu = true;
                        }
                    });
                    ui.separator();
                    if menu_item(ui, lucide::TRASH_2, "Xóa chỉ ở phía tôi", Color32::from_rgb(239, 68, 68)).clicked() {
                        action = Some(BubbleAction::DeleteForMe);
                        close_menu = true;
                    }

                    let menu_rect = ui.min_rect();
                    let pointer_pos = ui
                        .ctx()
                        .input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
                    let open_time: f64 =
                        ui.ctx().data(|d| d.get_temp(open_time_key)).unwrap_or(0.0);
                    if ui.input(|i| i.time) > open_time + 0.05 {
                        let pressed = ui.ctx().input(|i| i.pointer.any_pressed());
                        if pressed {
                            if let Some(pos) = pointer_pos {
                                if !menu_rect.expand(4.0).contains(pos) {
                                    close_menu = true;
                                }
                            }
                        }
                    }
                    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                        close_menu = true;
                    }
                });
        });

    if close_menu {
        ui.ctx()
            .data_mut(|d| d.remove::<(String, Pos2, usize)>(ctx_menu_key));
        ui.ctx().request_repaint();
    }

    action
}

/// Quoted-message block shown at the top of a reply bubble.
pub(super) fn render_quote_block(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    outgoing: bool,
    max_w: f32,
) -> Option<BubbleAction> {
    let qt = msg.reply_to_text.as_ref()?;
    let (text_col, meta_col) = if outgoing {
        (pal.bubble_out_text, pal.bubble_out_meta)
    } else {
        (pal.bubble_in_text, pal.bubble_in_meta)
    };
    // Quote on a solid blue outgoing bubble: white bar on a translucent white well.
    let bar_col = if outgoing {
        Color32::from_rgba_unmultiplied(255, 255, 255, 220)
    } else {
        pal.text_muted
    };
    let bg_col = if outgoing {
        Color32::from_rgba_unmultiplied(255, 255, 255, 38)
    } else {
        text_col.linear_multiply(0.05)
    };
    let inner_col_w = (max_w - 28.0).max(72.0);
    let frame = egui::Frame::none()
        .fill(bg_col)
        .rounding(Rounding::same(7.0))
        .inner_margin(Margin::symmetric(8.0, 5.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(3.0, 28.0), Sense::hover());
                ui.painter()
                    .rect_filled(bar_rect, Rounding::same(1.5), bar_col);
                ui.vertical(|ui| {
                    ui.set_width(inner_col_w);
                    let who = msg
                        .reply_to_sender
                        .as_deref()
                        .filter(|sn| !sn.is_empty() && !sn.chars().all(|c| c.is_ascii_digit()))
                        .unwrap_or("Tin nhắn trích dẫn");
                    ui.label(RichText::new(who).color(bar_col).size(11.5).strong());
                    ui.add(
                        egui::Label::new(
                            RichText::new(truncate_chars(qt, 84))
                                .color(meta_col)
                                .size(12.0),
                        )
                        .wrap(),
                    );
                });
            });
        });
    let reference = crate::message_navigation::message_reply_reference(msg);
    let response = ui.interact(
        frame.response.rect,
        frame.response.id.with("quote_jump"),
        Sense::click(),
    );
    let clicked = response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Đi tới tin nhắn gốc đã tải")
        .clicked();
    ui.add_space(5.0);
    clicked.then(|| BubbleAction::JumpToQuoted(reference.unwrap_or_default()))
}

pub(super) fn render_hover_reaction_bar(
    ctx: &egui::Context,
    bubble_rect: Rect,
    msg: &ChatMessage,
    idx: usize,
    outgoing: bool,
    pal: &ThemePalette,
    _emoji_textures: &HashMap<String, TextureHandle>,
) -> Option<BubbleAction> {
    let ctx_menu_open = ctx.data(|d| {
        d.get_temp::<(String, Pos2, usize)>(Id::new("active_bubble_context_menu")).is_some()
    });
    if ctx_menu_open || ctx.memory(|m| m.any_popup_open()) {
        return None;
    }
    let bar_w = 100.0_f32;
    let bar_h = 34.0_f32;
    let reaction_w = 198.0_f32;
    let reaction_h = 36.0_f32;
    let like_size = 26.0_f32;
    let clip = ctx.screen_rect();
    let min_x = clip.left() + 8.0;
    let max_bar_x = (clip.right() - bar_w - 8.0).max(min_x);
    let preferred_x = if outgoing {
        bubble_rect.left() - bar_w - 6.0
    } else {
        bubble_rect.right() + 6.0
    };
    let bar_x = preferred_x.clamp(min_x, max_bar_x);
    let min_y = clip.top() + 8.0;
    let max_bar_y = (clip.bottom() - bar_h - 8.0).max(min_y);
    let bar_y = (bubble_rect.bottom() - bar_h).clamp(min_y, max_bar_y);
    let bar_rect = Rect::from_min_size(Pos2::new(bar_x, bar_y), Vec2::new(bar_w, bar_h));
    let reaction_x = (bubble_rect.left() + 8.0).clamp(
        min_x,
        (clip.right() - reaction_w - 8.0).max(min_x),
    );
    let reaction_y = (bubble_rect.top() - reaction_h - 6.0)
        .clamp(min_y, (clip.bottom() - reaction_h - 8.0).max(min_y));
    let reaction_rect = Rect::from_min_size(
        Pos2::new(reaction_x, reaction_y),
        Vec2::new(reaction_w, reaction_h),
    );
    let like_x = (bubble_rect.left() + 8.0).clamp(
        min_x,
        (clip.right() - like_size - 8.0).max(min_x),
    );
    let like_y = (bubble_rect.bottom() - 9.0).clamp(
        min_y,
        (clip.bottom() - like_size - 8.0).max(min_y),
    );
    let like_rect = Rect::from_min_size(
        Pos2::new(like_x, like_y),
        Vec2::splat(like_size),
    );
    let hover_zone = bubble_rect
        .union(bar_rect)
        .union(reaction_rect)
        .union(like_rect)
        .expand2(Vec2::splat(10.0));
    let hover_id = Id::new("active_hover_reaction_bubble");
    let msg_key = format!("{}_{}", msg.id, msg.ts);

    let pointer_pos = ctx.input(|i| i.pointer.hover_pos().or(i.pointer.interact_pos()));
    let in_hover_zone = pointer_pos.map_or(false, |p| hover_zone.contains(p));
    let in_bubble = pointer_pos.map_or(false, |p| {
        bubble_rect.expand2(Vec2::new(4.0, 4.0)).contains(p)
    });

    // Active hover state: (message key, bar rect, frame it was last shown). The bar floats over the
    // previous message, so without the rect a neighbour's bubble under the bar stole the hover and
    // the bar vanished as soon as the pointer reached it.
    let hover_state_id = hover_id.with("state");
    let frame_now = ctx.cumulative_pass_nr();
    let active_state: Option<(String, Rect, u64)> = ctx.data(|d| d.get_temp(hover_state_id));
    let active_key: Option<String> = ctx.data(|d| d.get_temp(hover_id));

    let pointer_on_other_bar = match (&active_state, pointer_pos) {
        (Some((key, rect, seen)), Some(p)) => {
            key != &msg_key
                && frame_now.saturating_sub(*seen) <= 2
                && rect.expand2(Vec2::new(10.0, 10.0)).contains(p)
        }
        _ => false,
    };

    let should_show = if active_key.as_deref() == Some(&msg_key) {
        if in_hover_zone {
            true
        } else {
            ctx.data_mut(|d| {
                d.remove::<String>(hover_id);
                d.remove::<(String, Rect, u64)>(hover_state_id);
            });
            false
        }
    } else if in_bubble && !pointer_on_other_bar {
        ctx
            .data_mut(|d| d.insert_temp(hover_id, msg_key.clone()));
        true
    } else {
        false
    };
    if !should_show {
        return None;
    }
    ctx
        .data_mut(|d| d.insert_temp(hover_state_id, (msg_key.clone(), bar_rect, frame_now)));

    let mut action = None;

    egui::Area::new(Id::new(("hover_reaction_bar", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(bar_x, bar_y))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let button = |icon: &str| {
                    egui::Button::new(
                        RichText::new(icon)
                            .size(13.5)
                            .color(pal.text_secondary),
                    )
                    .fill(pal.bg_list)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .min_size(Vec2::splat(29.0))
                    .rounding(Rounding::same(15.0))
                };
                if ui
                    .add(button(lucide::QUOTE))
                    .on_hover_text("Trả lời")
                    .clicked()
                {
                    ctx.data_mut(|d| d.remove::<String>(hover_id));
                    action = Some(BubbleAction::Reply);
                }
                if ui
                    .add(button(lucide::FORWARD))
                    .on_hover_text("Chia sẻ")
                    .clicked()
                {
                    ctx.data_mut(|d| d.remove::<String>(hover_id));
                    action = Some(BubbleAction::Forward);
                }
                let more_response = ui
                    .add(button(lucide::ELLIPSIS))
                    .on_hover_text("Thêm tùy chọn");
                if more_response.clicked() {
                    let click_pos = more_response.rect.left_bottom();
                    let current_time = ui.input(|i| i.time);
                    ctx.data_mut(|d| {
                        d.insert_temp(
                            Id::new("active_bubble_context_menu"),
                            (msg_key.clone(), click_pos, idx),
                        );
                        d.insert_temp(Id::new("bubble_context_menu_open_time"), current_time);
                        d.remove::<String>(hover_id);
                    });
                    ctx.request_repaint();
                }
            });
        });

    egui::Area::new(Id::new(("quick_reaction_picker", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(reaction_rect.min)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(pal.bg_list)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .rounding(Rounding::same(18.0))
                .inner_margin(Margin::symmetric(6.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for &(emoji, _, label) in &crate::gui::REACTION_LIST {
                            let response = ui
                                .add(
                                    egui::Button::new(RichText::new(emoji).size(17.0))
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(Stroke::NONE)
                                        .min_size(Vec2::splat(28.0))
                                        .rounding(Rounding::same(14.0)),
                                )
                                .on_hover_text(label);
                            if response.clicked() {
                                ctx.data_mut(|d| d.remove::<String>(hover_id));
                                action = Some(BubbleAction::React(emoji));
                            }
                        }
                    });
                });
        });

    egui::Area::new(Id::new(("hover_reaction_like", idx, msg.ts, &msg.id)))
        .order(egui::Order::Foreground)
        .fixed_pos(like_rect.min)
        .show(ctx, |ui| {
            let like_button = egui::Button::new(RichText::new("👍").size(14.0))
                .fill(pal.bg_list)
                .stroke(Stroke::new(1.0_f32, pal.border))
                .min_size(Vec2::splat(like_size))
                .rounding(Rounding::same(like_size / 2.0));
            if ui.add(like_button).on_hover_text("Thích").clicked() {
                ctx.data_mut(|d| d.remove::<String>(hover_id));
                action = Some(BubbleAction::React("👍"));
            }
        });

    if action.is_some() {
        ctx.data_mut(|d| d.remove::<String>(hover_id));
        ctx.request_repaint();
    }
    action
}

pub(super) fn render_reactions(
    ui: &mut egui::Ui,
    msg: &ChatMessage,
    pal: &ThemePalette,
    emoji_textures: &HashMap<String, TextureHandle>,
) -> Option<BubbleAction> {
    let has_pin = msg.is_pinned;
    let has_star = msg.is_starred;
    let has_react = !msg.reactions.is_empty() || !msg.reaction_summaries.is_empty();

    if !has_pin && !has_star && !has_react {
        return None;
    }

    // Clicking the summary pill opens the "who reacted" dialog.
    let mut action: Option<BubbleAction> = None;
    ui.add_space(3.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 2.0);

        if has_pin {
            egui::Frame::none()
                .fill(pal.primary.linear_multiply(0.18))
                .stroke(Stroke::new(1.0_f32, pal.primary))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã ghim", lucide::PIN))
                            .size(11.0)
                            .color(pal.primary)
                            .strong(),
                    );
                });
        }

        if has_star {
            egui::Frame::none()
                .fill(Color32::from_rgb(234, 179, 8).linear_multiply(0.2))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(234, 179, 8)))
                .rounding(Rounding::same(8.0))
                .inner_margin(Margin::symmetric(5.0, 2.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("{} Đã lưu", lucide::STAR))
                            .size(11.0)
                            .color(Color32::from_rgb(234, 179, 8))
                            .strong(),
                    );
                });
        }

        if has_react {
            let mut items: Vec<(String, u64)> = Vec::new();
            if !msg.reaction_summaries.is_empty() {
                for s in &msg.reaction_summaries {
                    if s.count > 0 {
                        if let Some(pos) = items.iter().position(|(e, _)| e == &s.emoji) {
                            items[pos].1 = items[pos].1.saturating_add(s.count);
                        } else {
                            items.push((s.emoji.clone(), s.count));
                        }
                    }
                }
            } else {
                for r in &msg.reactions {
                    if let Some(pos) = items.iter().position(|(e, _)| e == r) {
                        items[pos].1 = items[pos].1.saturating_add(1);
                    } else {
                        items.push((r.clone(), 1));
                    }
                }
            }
            // Most used first, so the pill shows the dominant icons.
            items.sort_by(|a, b| b.1.cmp(&a.1));
            let total: u64 = items.iter().map(|(_, c)| *c).sum();

            if total > 0 {
                // One compact pill like Zalo: up to 3 icons + the total. Separate pills per icon
                // grew wider than a sticker bubble and were clipped at the window edge.
                let fr_resp = egui::Frame::none()
                    .fill(pal.bg_elevated)
                    .stroke(Stroke::new(1.0_f32, pal.border))
                    .rounding(Rounding::same(11.0))
                    .inner_margin(Margin::symmetric(6.0, 2.5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                            for (emoji, _) in items.iter().take(3) {
                                let key = emoji_to_tex_key(emoji);
                                if let Some(tex) = emoji_textures.get(key) {
                                    ui.add(
                                        egui::Image::new(tex).fit_to_exact_size(Vec2::splat(16.0)),
                                    );
                                } else {
                                    ui.label(RichText::new(emoji).size(13.0));
                                }
                            }
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(total.to_string())
                                    .size(11.5)
                                    .color(pal.text_primary)
                                    .strong(),
                            );
                        });
                    })
                    .response;
                let pill_resp = ui.interact(
                    fr_resp.rect,
                    Id::new(("react_pill", &msg.id, msg.ts)),
                    Sense::click(),
                );
                if pill_resp.clicked() {
                    action = Some(BubbleAction::ShowReactions);
                }
                pill_resp
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_ui(|ui| {
                        // Per-reaction breakdown with the real icons instead of plain text.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 0.0);
                            for (emoji, count) in &items {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                                    match emoji_textures.get(emoji_to_tex_key(emoji)) {
                                        Some(tex) => {
                                            ui.add(
                                                egui::Image::new(tex)
                                                    .fit_to_exact_size(Vec2::splat(18.0)),
                                            );
                                        }
                                        None => {
                                            ui.label(RichText::new(emoji).size(14.0));
                                        }
                                    }
                                    ui.label(RichText::new(count.to_string()).size(12.5).strong());
                                });
                            }
                        });
                    });
            }
        }
    });

    action
}

#[cfg(test)]
mod quote_navigation_tests {
    use super::*;

    #[test]
    fn quote_navigation_bubble_propagates_action_for_both_directions() {
        for is_outgoing in [false, true] {
            let ctx = egui::Context::default();
            let msg = ChatMessage {
                id: "reply-1".into(),
                text: "Reply body".into(),
                sender: "Sender".into(),
                reply_to_id: Some("original-1".into()),
                reply_to_text: Some("Original text".into()),
                is_outgoing,
                ..Default::default()
            };
            let mut position = egui::pos2(30.0, 30.0);
            let mut action = None;
            for frame in 0..4 {
                let mut events = vec![egui::Event::PointerMoved(position)];
                if frame >= 2 {
                    events.push(egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 2,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(600.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            action = render_chat_bubble(
                                ui,
                                &msg,
                                false,
                                false,
                                false,
                                false,
                                None,
                                "Thread",
                                None,
                                &ThemePalette::light(),
                                &HashMap::new(),
                                &mut HashSet::new(),
                                &HashMap::new(),
                                &HashMap::new(),
                                0,
                            );
                        });
                    },
                );
                if frame == 0 {
                    position = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) if text.galley.job.text == "Original text" => {
                                Some(text.pos + Vec2::new(4.0, 4.0))
                            }
                            _ => None,
                        })
                        .expect("Quote must be rendered in the bubble");
                }
            }
            assert!(
                matches!(action, Some(BubbleAction::JumpToQuoted(reference)) if reference == "original-1")
            );
        }
    }

    #[test]
    fn quote_navigation_emits_exact_reference_when_clicked() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            reply_to_id: Some("00123".into()),
            reply_to_text: Some("Original text".into()),
            ..Default::default()
        };
        let mut action = None;
        for pressed in [None, Some(true), Some(false)] {
            let mut events = vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: egui::pos2(30.0, 30.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        action = render_quote_block(ui, &msg, &ThemePalette::light(), false, 300.0);
                    });
                },
            );
        }
        assert!(
            matches!(action, Some(BubbleAction::JumpToQuoted(reference)) if reference == "00123")
        );
    }

    #[test]
    fn quote_navigation_advertises_click_when_reference_exists() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            reply_to_id: Some("original-1".into()),
            reply_to_text: Some("Original text".into()),
            ..Default::default()
        };
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::PointerMoved(egui::pos2(30.0, 30.0))],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        render_quote_block(ui, &msg, &ThemePalette::light(), false, 300.0);
                    });
                },
            );
            if output.platform_output.cursor_icon == egui::CursorIcon::PointingHand {
                return;
            }
        }
        panic!("Quote with an exact reference must advertise a clickable target");
    }

    #[test]
    fn bubble_context_menu_opens_on_secondary_click_and_emits_reply_action() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            id: "msg-ctx-test".into(),
            text: "Hello right click test".into(),
            sender: "Tester".into(),
            is_outgoing: false,
            ts: 123456789,
            ..Default::default()
        };

        let bubble_rect = Rect::from_min_size(Pos2::new(50.0, 50.0), Vec2::new(120.0, 40.0));
        let click_pos = Pos2::new(60.0, 60.0);
        let screen = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));

        // Frame 1: Pointer moved to bubble
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(click_pos)],
                screen_rect: screen,
                time: Some(1.0),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        // Frame 2: Secondary button pressed
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.05),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        // Frame 3: Secondary button released -> Context menu is created in ctx.data
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Secondary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.1),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        // Frame 4: Next frame where Area shapes are painted
        let output = ctx.run(
            egui::RawInput {
                screen_rect: screen,
                time: Some(1.15),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        // Find "Trả lời" menu item position from output shapes
        let mut reply_pos = None;
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text.contains("Trả lời") {
                    reply_pos = Some(text.pos + Vec2::new(10.0, 5.0));
                    break;
                }
            }
        }
        let reply_pos = reply_pos.expect("Context menu should be open with 'Trả lời' text");

        // Frame 5: Move mouse to "Trả lời" and press Primary
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(reply_pos),
                    egui::Event::PointerButton {
                        pos: reply_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                screen_rect: screen,
                time: Some(1.2),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    let _ = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        // Frame 6: Release Primary -> Reply action triggered!
        let mut action = None;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: reply_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                time: Some(1.25),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let bubble_resp =
                        ui.interact(bubble_rect, Id::new("dummy_bubble"), Sense::hover());
                    action = bubble_context_menu(
                        ui,
                        &bubble_resp,
                        &msg,
                        0,
                        &ThemePalette::light(),
                        &HashMap::new(),
                        false,
                        "Tester",
                    );
                });
            },
        );

        assert!(matches!(action, Some(BubbleAction::Reply)));
    }

    #[test]
    fn bubble_hover_reaction_bar_emits_react_action() {
        let ctx = egui::Context::default();
        let msg = ChatMessage {
            id: "msg-hover-test".into(),
            text: "Hello hover react".into(),
            sender: "Tester".into(),
            is_outgoing: false,
            ts: 987654321,
            ..Default::default()
        };

        let bubble_rect = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(150.0, 40.0));
        let screen = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0)));

        // Frame 0: Hover over bubble -> triggers hover bar
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(Pos2::new(120.0, 110.0))],
                screen_rect: screen,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = render_hover_reaction_bar(
                        ui,
                        bubble_rect,
                        &msg,
                        0,
                        false,
                        &ThemePalette::light(),
                        &HashMap::new(),
                    );
                });
            },
        );

        // Frame 1: Second frame so Area is painted
        let output = ctx.run(
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = render_hover_reaction_bar(
                        ui,
                        bubble_rect,
                        &msg,
                        0,
                        false,
                        &ThemePalette::light(),
                        &HashMap::new(),
                    );
                });
            },
        );

        // Find first reaction emoji text from output shapes
        let mut emoji_pos = None;
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text == "👍" {
                    emoji_pos = Some(text.pos + Vec2::new(4.0, 4.0));
                    break;
                }
            }
        }
        let emoji_pos = emoji_pos.expect("Reaction bar should be rendered with 👍 emoji");

        // Frame 1: Move mouse to emoji and press Primary
        let _ = ctx.run(
            egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(emoji_pos),
                    egui::Event::PointerButton {
                        pos: emoji_pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                screen_rect: screen,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let _ = render_hover_reaction_bar(
                        ui,
                        bubble_rect,
                        &msg,
                        0,
                        false,
                        &ThemePalette::light(),
                        &HashMap::new(),
                    );
                });
            },
        );

        // Frame 2: Release Primary -> React action emitted!
        let mut action = None;
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: emoji_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                screen_rect: screen,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = render_hover_reaction_bar(
                        ui,
                        bubble_rect,
                        &msg,
                        0,
                        false,
                        &ThemePalette::light(),
                        &HashMap::new(),
                    );
                });
            },
        );

        assert!(matches!(action, Some(BubbleAction::React("👍"))));
    }

    #[test]
    fn extract_photo_user_caption_filters_raw_image_filenames() {
        let raw_filename_msg = ChatMessage {
            text: "[Hình ảnh] msedge_uuRZ2mxGXV.png".into(),
            ..Default::default()
        };
        assert_eq!(extract_photo_user_caption(&raw_filename_msg), None);

        let clipboard_msg = ChatMessage {
            text: "[Hình ảnh] clipboard_1730932200000.png".into(),
            ..Default::default()
        };
        assert_eq!(extract_photo_user_caption(&clipboard_msg), None);

        let real_caption_msg = ChatMessage {
            text: "[Hình ảnh] Xem ảnh này nè".into(),
            ..Default::default()
        };
        assert_eq!(
            extract_photo_user_caption(&real_caption_msg),
            Some("Xem ảnh này nè")
        );
    }
}

#[cfg(test)]
mod download_target_tests {
    use super::*;

    fn m(text: &str, src: Option<&str>, title: Option<&str>, url: Option<&str>) -> ChatMessage {
        ChatMessage {
            text: text.into(),
            link_src: src.map(Into::into),
            link_title: title.map(Into::into),
            link_url: url.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn file_card_uses_title_or_text_name() {
        let a = m("[Tập tin] a.pdf", None, None, Some("https://f.zdn.vn/x"));
        assert_eq!(
            attachment_download_target(&a),
            Some(("https://f.zdn.vn/x".into(), "a.pdf".into()))
        );
        let b = m("x", Some("Tập tin Zalo"), Some("[Tập tin] b.docx"), Some("https://f.zdn.vn/y"));
        assert_eq!(attachment_download_target(&b).unwrap().1, "b.docx");
    }

    #[test]
    fn photo_falls_back_to_thumb_and_empty_name() {
        let mut p = m("[Hình ảnh]", Some("Hình ảnh Zalo"), Some("[Hình ảnh]"), None);
        p.link_thumb = Some("https://photo.zdn.vn/t.jpg".into());
        assert_eq!(
            attachment_download_target(&p),
            Some(("https://photo.zdn.vn/t.jpg".into(), String::new()))
        );
    }

    #[test]
    fn no_target_without_web_url_or_for_plain_text() {
        assert_eq!(attachment_download_target(&m("[Tập tin] a.pdf", None, None, None)), None);
        assert_eq!(
            attachment_download_target(&m("[Video]", None, None, Some("file:///etc/passwd"))),
            None
        );
        assert_eq!(attachment_download_target(&m("chào", None, None, Some("https://x.vn"))), None);
    }

    #[test]
    fn card_texts_never_show_urls_or_domains() {
        let mut f = m("[Tập tin] a.pdf", Some("dl.zdn.vn"), None, Some("https://dl.zdn.vn/x"));
        assert_eq!(file_card_texts(&f), ("a.pdf".into(), "Tập tin đính kèm".into()));
        f.link_desc = Some("24,1 MB".into());
        assert_eq!(file_card_texts(&f).1, "24,1 MB");
        let v = m("[Video]", Some("Video Zalo"), Some("https://v.zdn.vn/c.mp4"), Some("https://v.zdn.vn/c.mp4"));
        assert_eq!(file_card_texts(&v), ("Video".into(), "Video đính kèm".into()));
        assert_eq!(video_title(&v), "Video");
        let named = m("[Video] demo.mp4", Some("Video Zalo"), None, Some("https://v.zdn.vn/c.mp4"));
        assert_eq!(video_title(&named), "demo.mp4");
        let voice = m("[Tin nhắn thoại]", None, Some("Tin nhắn thoại"), Some("https://a.zdn.vn/v.m4a"));
        assert_eq!(file_card_texts(&voice).0, "Tin nhắn thoại");
    }

    #[test]
    fn video_urls_and_legacy_rows() {
        let mut v = m("[Video]", Some("Video Zalo"), None, Some("https://v.zdn.vn/c.mp4"));
        assert_eq!(video_thumb_url(&v), None);
        v.link_thumb = Some("https://v.zdn.vn/c.jpg".into());
        assert_eq!(video_thumb_url(&v), Some("https://v.zdn.vn/c.jpg"));
        assert_eq!(video_play_url(&v), Some("https://v.zdn.vn/c.mp4"));
        assert_eq!(video_play_url(&m("[Tập tin] a", None, None, Some("https://x.vn/a"))), None);

        // Legacy photo row: text == link_url on Zalo's photo CDN.
        let u = "https://f21-zpc.zdn.vn/1/abc.jpg";
        let legacy = m(u, Some("f21-zpc.zdn.vn"), None, Some(u));
        let view = legacy_media_view(&legacy).expect("legacy photo");
        assert_eq!(view.text, "[Hình ảnh]");
        assert_eq!(view.link_src.as_deref(), Some("Hình ảnh Zalo"));
        assert_eq!(view.link_thumb.as_deref(), Some(u));
        assert!(is_photo_message(&view));
        // Legacy video row keeps its thumbnail.
        let vu = "https://f9-zpc.zdn.vn/v/clip.mp4";
        let mut lv = m(vu, Some("f9-zpc.zdn.vn"), None, Some(vu));
        lv.link_thumb = Some("https://f9-zpc.zdn.vn/v/clip.jpg".into());
        let view = legacy_media_view(&lv).expect("legacy video");
        assert_eq!(video_thumb_url(&view), Some("https://f9-zpc.zdn.vn/v/clip.jpg"));
        // Ordinary links are untouched.
        let web = "https://news.example.com/a.jpg";
        assert!(legacy_media_view(&m(web, Some("news.example.com"), None, Some(web))).is_none());
    }

    #[test]
    fn progress_label_formats() {
        let s = DownloadState::InProgress { received: 512, total: Some(1024) };
        assert_eq!(download_progress_label(&s, true), "Đang tải 50%");
        assert!(download_progress_label(&s, false).starts_with("50% · "));
        let u = DownloadState::InProgress { received: 0, total: None };
        assert_eq!(download_progress_label(&u, false), "Đang tải…");
    }
}
