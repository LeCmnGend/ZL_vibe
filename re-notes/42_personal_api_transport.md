# Chuẩn hoá request tài khoản cá nhân

Status: code-reviewed; live-session verification pending.

## Phạm vi

- Không thêm API hay giao diện OA, Bot, hoặc Mini App.
- OA OpenAPI dùng access token và quyền của OA; Bot dùng Bot Token riêng; Mini App dùng ZMP SDK và quyền người dùng.
- Các cơ chế xác thực đó không đăng nhập hoặc đọc hộp thư cá nhân hiện có.
- ZL-vibe tiếp tục dùng phiên cá nhân hiện tại, service map, cookie phiên và mã hoá tham số theo từng API đã ghi nhận.

References: [OA authentication](https://docs.zaloplatforms.com/docs/OA/quan-ly/quan-ly-thong-tin-oa/lay-thong-tin-zalo-official-account), [Bot polling and token](https://docs.zaloplatforms.com/docs/BOT/create_bot), [Mini App permissions](https://docs.zaloplatforms.com/docs/MA/intro/request-permission).

## Chuẩn hoá đã làm

- Các GET chỉ đọc đã chuẩn hoá dùng `Session::get_endpoint`; POST chỉ đọc đã chuẩn hoá dùng `Session::post_endpoint_form`.
- Các tuyến đã chuyển gồm hồ sơ, ảnh đại diện, dấu chưa đọc, hội thoại ghim, lịch sử nhóm/Cloud, hồ sơ nhóm, tra bạn, trạng thái quan hệ, nhãn dán và phân tích liên kết phía máy chủ.
- Hội thoại ẩn dùng `get_sensitive_endpoint`, giữ việc che nội dung nhạy cảm khỏi log.
- API thay đổi trạng thái, gửi tin, long-poll, tải tệp, CDN, và đăng nhập giữ luồng chuyên biệt; không bật thử lại tự động cho thao tác có thể tạo tác dụng phụ.
- Mã hoá tham số, cookie, giải mã phản hồi và xác thực HTTPS không đổi.

Manual verification after a user-authorized build:

1. Tải hồ sơ cá nhân, danh sách nhóm, dấu chưa đọc, lịch sử nhóm và tìm bạn.
2. Xác nhận lỗi mạng/404 thử host dự phòng; lỗi nghiệp vụ không chuyển host.
3. Mở hội thoại ẩn và xác nhận payload không xuất hiện trong log.
4. Xác nhận các thao tác ghi không bị gửi lặp khi host lỗi.

No build, test, or live API request was run.
