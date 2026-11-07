# Group creation API note

Status: implementation reference only; not live-captured or runtime-verified.

Reference: `RFS-ADRENO/zca-js/src/apis/createGroup.ts` on 2026-11-08:
https://github.com/RFS-ADRENO/zca-js/blob/main/src/apis/createGroup.ts

The reference uses `POST /api/group/create/v2` on the group service, with AES-encoded query parameters containing `clientId`, `gname`, `gdesc`, `members`, `membersTypes`, `nameChanged`, `createLink`, `clientLang`, `imei`, and `zsource`.

Manual verification when the user authorizes a build:
1. Create a test group with one selected friend and a short name.
2. Confirm the request succeeds and returns `data.groupId`.
3. Refresh conversations and confirm the new group appears.
4. Confirm invalid/empty member selections are rejected before sending.

Do not report the protocol as verified until a live session confirms the request and response.
