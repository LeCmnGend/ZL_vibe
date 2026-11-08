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

## Member administration extension

Status: source-matched; live-session verification pending.

Reference: captured `RFS-ADRENO/zca-js/src/apis` source in Feeding `zalo-100` snapshot:

- Deputy promotion/demotion: `GET /api/group/admins/add` and `/api/group/admins/remove`; AES parameters `grid`, `members`, `imei`.
- Remove member: `POST /api/group/kickout`; form body carries AES `params` with `grid`, `members`, `imei`.
- Transfer owner: `GET /api/group/change-owner`; AES parameters `grid`, `newAdminId`, `imei`, and `language`.
- UI actions are limited to owner/deputy roles reported by group metadata. Removing a member and transferring ownership require confirmation.

Manual verification when the user authorizes a build and live session:

1. As owner, promote and demote a test member; refresh and confirm the role changes.
2. Remove a test member; refresh and confirm the member disappears.
3. Transfer ownership in a disposable test group; confirm the new owner and the former owner's reduced permissions.
4. As a deputy, attempt an owner-only action; confirm Zalo returns a visible permission error.

No build, test, or live API request was run. Revert the group-administration commit to roll back these endpoints and UI actions.

## Group board pins and polls

Status: source-matched; live-session verification pending.

References: local `zca-js` snapshot in Feeding `zalo-100` for `/api/board/list`, poll create/vote, and `BoardType`; archived `Its-VrxxDev/zlapi` implementation for group message pin and unpin payload fields.

- Fetch board items from the runtime `group_board` service-map host, with the source-derived `https://groupboard-wpa.chat.zalo.me` fallback.
- Create and vote polls through the `group` service map. Board list items supply poll questions, options, counts, and voter state.
- Pin through `/api/board/topic/createv2`; unpin through `/api/board/unpinv2`. Pin state and board identifiers are refreshed from the server list.
- Pin payload support currently covers text, photo, voice, sticker, and video message kinds. Other kinds return an explicit error.
- These endpoints are reverse-engineered and unofficial. The pin payload source uses an older ZPW version; live compatibility is not confirmed.

Manual verification when the user authorizes a build and live session:

1. Open a group with existing pins and polls; confirm the server board appears in chat history.
2. Pin and unpin a text and photo message; confirm the top pinned strip updates after each server refresh.
3. Create a poll with single and multiple choices; vote and confirm counts update from the board response.
4. Confirm a failed or unauthorized request shows an error and does not leave local pin state changed.

No build, test, or live API request was run for this change.
