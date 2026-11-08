# Phone app exits after sync confirmation

Status: source-supported correction; phone reproduction pending.

Evidence:
- Local extracted Zalo PC bundle `default-login-main-startup-shared-worker-znotification.180b91810a16ba658871.js` implements the desktop backup request through `/api/message/pull_mobile_msg` (`pullMobileMsg`).
- This app previously sent that request and then also sent `/api/transfer-sync-v2/request-sync`, even when the first request returned success.
- Sending two sync protocols can leave the phone processing duplicate jobs. This is the likely cause of the reported exit; no phone logs or live reproduction are available.

Change: treat the Zalo PC `pullMobileMsg` route as primary; send Sync-V2 only when the first request fails or is rejected.

Manual verification when a build and phone session are authorized:
1. Start one manual sync and accept it on the phone; confirm Zalo stays open and completes the upload.
2. Confirm the desktop imports the backup and continues Cloud/group sync.
3. Confirm Sync-V2 is attempted only when the primary request is rejected.

No build, test, or live request was run for this change.
