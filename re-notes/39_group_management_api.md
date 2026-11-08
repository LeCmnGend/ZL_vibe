# Group member invite and settings APIs

Status: source-matched; live-session verification pending.

Reference: local `zca-js` bundle, `init_addUserToGroup` and `init_updateGroupSettings` in `dist/index.js`.

- Invite: `POST /api/group/invite/v2`; AES parameters `grid`, `members`, `memberTypes` (`-1` per member), `imei`, `clientLang`.
- Settings read: group `getmg-v2` response, `gridInfoMap.<group-id>.setting`.
- Settings update: `GET /api/group/setting/update`; AES parameters map nine supported setting flags to `0`/`1` and include the reference defaults `bannFeature`, `dirtyMedia`, `banDuration`, and `blocked_members`.
- The UI sources invite candidates from the app's existing friend threads and refreshes group members after a successful invite.
- The app checks non-zero `error_code` with its shared write-response handler.

Manual verification when the user authorizes a build and live session:
1. Open a group, invite one friend, and confirm the response and refreshed member list.
2. Load settings, change one flag, save, reopen the dialog, and confirm the server value.
3. Confirm an account without permission receives a visible server error and can retry loading settings.

No build, test, or live API request was run for this change.
