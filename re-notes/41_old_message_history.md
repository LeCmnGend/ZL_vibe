# Tải lịch sử tin nhắn cũ

Status: source-matched; live-session verification pending.

Reference: unofficial [RFS-ADRENO/zca-js](https://github.com/RFS-ADRENO/zca-js), `src/apis/listen.ts`:

- Chat riêng gửi WebSocket `cmd 510`, nhóm gửi `cmd 511`; cả hai dùng `subCmd 1`.
- Payload yêu cầu gồm `first: true`, `lastId` là ID máy chủ cũ nhất đang có, và `preIds: []`.
- Phản hồi chat riêng chứa `data.msgs`; phản hồi nhóm chứa `data.groupMsgs`.
- Worker WebSocket hiện có đã nhận hai mã này, nhập tin vào hội thoại đã biết, rồi lưu thay đổi.
- “Cloud của tôi” vẫn dùng API lịch sử Cloud riêng.
- `lastId` phải lấy từ tin máy chủ cũ nhất trong phần lịch sử đang hiển thị. Không dùng ID cục bộ hoặc ID ứng dụng.
- Không suy ra hết lịch sử từ số tin SQLite; chỉ máy chủ mới xác nhận được trang cũ còn hay hết.

Manual verification when the user authorizes a build and live session:

1. In a one-to-one chat, request older messages and confirm they appear before existing messages without duplicates.
2. Repeat in a group chat and confirm the same behavior for group messages.
3. Request another page and confirm pagination continues from the oldest server message ID.
4. Disconnect WebSocket and confirm the UI reports that the request was not sent.

Source code and protocol are unofficial. No build, test, or live API request was run.
