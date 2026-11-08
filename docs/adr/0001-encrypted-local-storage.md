# ADR-0001: Encrypt local session and chat data

- Status: accepted
- Date: 2026-11-08

## Context

The Windows client stores a Zalo session and local chat history under `%APPDATA%\ZLV`.
The selected upstream vault uses AES-GCM for local data, but its live plaintext directory depends
on Linux tmpfs. The Windows client must not leave its active SQLite database as plaintext on disk.

## Decision

- Encrypt session and thread-cache files with AES-256-GCM.
- Encrypt SQLite databases with SQLCipher so normal SQLite reads and writes stay page-based.
- Protect a random 256-bit data key with Windows DPAPI for the current user account.
- Convert existing plaintext databases with `sqlcipher_export` and verify the encrypted copy before
  atomically replacing the original.
- Encrypt existing plaintext files during startup migration; stop startup on migration or key errors.
- Do not require an app PIN. The app opens without PIN entry; Windows unwraps the key for the
  current account. This key is account-bound and cannot be copied to another Windows account as-is.

## Consequences

- SQLCipher uses the vendored OpenSSL build for reproducible Windows support.
- Existing data is migrated in place. The first database conversion may take longer for a large file.
- Tests must use fake data and include a fixed AES-GCM known-answer vector.
- No build or test result is claimed until the user requests a build.

## Evidence

- `longdoanz/ctdozlite` commit `6c43629` stores local data in an AES-256-GCM vault and uses
  `VACUUM INTO` to snapshot active SQLite databases.
- The local `zalo-100` snapshot of `ctdoteam1110/ctdozlite` still stores `session.json` as JSON
  and opens the history database with ordinary SQLite.
- The local `bqqhuy/ctdozlite` snapshot protects session files with current-user Windows DPAPI,
  but opens history databases with ordinary SQLite. This protects credentials, not chat history.
- The `zalo-100` manifest says these snapshots omit commit IDs and content hashes, so their exact
  revisions cannot be verified from the local capture.
- The current application uses `%APPDATA%\ZLV`, `session.json` and `zalo_history.db`.
