//! Encrypt local sessions and SQLite data at rest on Windows.

use anyhow::{bail, Context, Result};
use rusqlite::Connection;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroizing;

const ENCRYPTED_MAGIC: &[u8; 8] = b"ZLVENC1\0";
#[cfg(windows)]
const KEY_MAGIC: &[u8; 8] = b"ZLVKEY1\0";
const NONCE_LEN: usize = 12;
static DATA_KEY: OnceLock<Zeroizing<[u8; 32]>> = OnceLock::new();
static MIGRATION_LOCK: Mutex<()> = Mutex::new(());

/// Load or create a data key protected by the current Windows account.
pub fn initialize(data_dir: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let key_path = data_dir.join("vault.key");
        let key_bytes = match std::fs::read(&key_path) {
            Ok(blob) => {
                if !blob.starts_with(KEY_MAGIC) {
                    bail!(
                        "vault.key không đúng định dạng; giữ nguyên dữ liệu để khôi phục thủ công"
                    );
                }
                dpapi_unprotect(&blob[KEY_MAGIC.len()..])
                    .context("Không mở được khóa dữ liệu Windows")?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if has_encrypted_data(data_dir)? {
                    bail!("Thiếu vault.key nhưng dữ liệu đã mã hóa; không tạo khóa mới để tránh mất dữ liệu");
                }
                use aes_gcm::aead::rand_core::RngCore;
                let key = Zeroizing::new({
                    let mut bytes = [0u8; 32];
                    aes_gcm::aead::OsRng.fill_bytes(&mut bytes);
                    bytes
                });
                let mut blob = KEY_MAGIC.to_vec();
                blob.extend_from_slice(&dpapi_protect(&key)?);
                crate::data_dir::write_private_atomic(&key_path, &blob)?;
                key.to_vec()
            }
            Err(error) => return Err(error.into()),
        };
        let key_bytes = Zeroizing::new(key_bytes);
        if key_bytes.len() != 32 {
            bail!("Khóa dữ liệu Windows không hợp lệ");
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(key_bytes.as_slice());
        DATA_KEY
            .set(Zeroizing::new(key))
            .map_err(|_| anyhow::anyhow!("Khóa dữ liệu đã được khởi tạo"))?;
        migrate_existing_data(data_dir)?;
    }
    #[cfg(not(windows))]
    let _ = data_dir;
    Ok(())
}

#[cfg(windows)]
fn has_encrypted_data(dir: &Path) -> Result<bool> {
    for name in [
        "session.json",
        "session.json.bak",
        "threads_cache.json",
        "zalo_history.db",
    ] {
        let path = dir.join(name);
        if starts_with(&path, ENCRYPTED_MAGIC)
            || (name == "zalo_history.db" && looks_like_encrypted_sqlite(&path))
        {
            return Ok(true);
        }
    }
    let accounts = dir.join("accounts");
    if accounts.is_dir() {
        for entry in std::fs::read_dir(accounts)? {
            let entry = entry?;
            let path = entry.path();
            let is_database = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|extension| matches!(extension, "db" | "sqlite" | "sqlite3"));
            if entry.file_type()?.is_file()
                && (starts_with(&path, ENCRYPTED_MAGIC)
                    || (is_database && looks_like_encrypted_sqlite(&path)))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(windows)]
fn starts_with(path: &Path, prefix: &[u8]) -> bool {
    let mut bytes = vec![0; prefix.len()];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok()
        && bytes == prefix
}

#[cfg(windows)]
fn looks_like_encrypted_sqlite(path: &Path) -> bool {
    let mut header = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header != b"SQLite format 3\0"
}

#[cfg(windows)]
fn migrate_existing_data(data_dir: &Path) -> Result<()> {
    for name in ["session.json", "session.json.bak", "threads_cache.json"] {
        migrate_sensitive_file(&data_dir.join(name))?;
    }
    let accounts = data_dir.join("accounts");
    if accounts.is_dir() {
        for entry in std::fs::read_dir(&accounts)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            match path.extension().and_then(std::ffi::OsStr::to_str) {
                Some("json") => migrate_sensitive_file(&path)?,
                Some("db" | "sqlite" | "sqlite3") => {
                    let _ = open_database(&path)?;
                }
                _ => {}
            }
        }
    }
    let history = data_dir.join("zalo_history.db");
    if history.exists() {
        let _ = open_database(history)?;
    }
    Ok(())
}

#[cfg(windows)]
#[cfg(windows)]
fn migrate_sensitive_file(path: &Path) -> Result<()> {
    if path.is_file() {
        let bytes = std::fs::read(path)?;
        if !bytes.starts_with(ENCRYPTED_MAGIC) {
            write_sensitive(path, &bytes)?;
        }
    }
    Ok(())
}

fn key() -> Option<&'static [u8; 32]> {
    DATA_KEY.get().map(|key| &**key)
}

#[cfg(test)]
pub(crate) fn initialize_test_key() {
    DATA_KEY.get_or_init(|| Zeroizing::new([0x5a; 32]));
}

pub fn enabled() -> bool {
    key().is_some()
}

pub fn encrypt_bytes(plain: &[u8]) -> Result<Vec<u8>> {
    let Some(key) = key() else {
        #[cfg(windows)]
        bail!("Khoá dữ liệu chưa được mở; từ chối ghi dữ liệu dạng rõ");
        #[cfg(not(windows))]
        return Ok(plain.to_vec());
    };
    use aes_gcm::aead::rand_core::RngCore;
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let mut nonce = [0u8; NONCE_LEN];
    aes_gcm::aead::OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: ENCRYPTED_MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("Mã hóa dữ liệu cục bộ thất bại"))?;
    let mut out = Vec::with_capacity(ENCRYPTED_MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(ENCRYPTED_MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

pub fn decrypt_bytes(blob: &[u8]) -> Result<Vec<u8>> {
    if !blob.starts_with(ENCRYPTED_MAGIC) {
        return Ok(blob.to_vec());
    }
    let key = key().context("Chưa nạp khóa dữ liệu cục bộ")?;
    if blob.len() < ENCRYPTED_MAGIC.len() + NONCE_LEN + 16 {
        bail!("Dữ liệu mã hóa bị cắt cụt");
    }
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key)?;
    cipher
        .decrypt(
            Nonce::from_slice(&blob[ENCRYPTED_MAGIC.len()..ENCRYPTED_MAGIC.len() + NONCE_LEN]),
            Payload {
                msg: &blob[ENCRYPTED_MAGIC.len() + NONCE_LEN..],
                aad: ENCRYPTED_MAGIC,
            },
        )
        .map_err(|_| anyhow::anyhow!("Không giải mã được dữ liệu cục bộ"))
}

pub fn write_sensitive(path: &Path, plain: &[u8]) -> Result<()> {
    crate::data_dir::write_private_atomic(path, &encrypt_bytes(plain)?)
}

pub fn read_sensitive(path: &Path) -> Result<Vec<u8>> {
    let blob = std::fs::read(path)?;
    let encrypted = blob.starts_with(ENCRYPTED_MAGIC);
    let plain = decrypt_bytes(&blob)?;
    if !encrypted && enabled() {
        write_sensitive(path, &plain)?;
    }
    Ok(plain)
}

pub fn read_sensitive_text(path: &Path) -> Result<String> {
    Ok(String::from_utf8(read_sensitive(path)?)?)
}

/// Open an app-owned database with SQLCipher; external import databases remain plain SQLite.
pub fn open_database(path: impl AsRef<Path>) -> Result<rusqlite::Connection> {
    let path = path.as_ref();
    if path == Path::new(":memory:") {
        return Ok(Connection::open_in_memory()?);
    }
    let Some(path) = managed_path(path) else {
        return Ok(Connection::open(path)?);
    };
    if key().is_none() {
        #[cfg(windows)]
        bail!("Khoá dữ liệu chưa được mở; từ chối mở cơ sở dữ liệu dạng rõ");
        #[cfg(not(windows))]
        return Ok(Connection::open(path)?);
    }
    migrate_plain_database(&path)?;
    open_cipher_connection(&path)
}

fn migrate_plain_database(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if std::fs::metadata(path)?.len() == 0 {
        std::fs::remove_file(path)?;
        return Ok(());
    }
    let mut header = [0u8; 16];
    let read = std::fs::File::open(path)?.read_exact(&mut header);
    if read.is_err() || &header != b"SQLite format 3\0" {
        return Ok(());
    }
    let _guard = MIGRATION_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut header = [0u8; 16];
    if std::fs::File::open(path)?.read_exact(&mut header).is_err()
        || &header != b"SQLite format 3\0"
    {
        return Ok(());
    }
    let temp = path.with_extension("db.vault-tmp");
    let _ = std::fs::remove_file(&temp);
    let source = Connection::open(path)?;
    let (busy, log_pages, checkpointed_pages): (i64, i64, i64) =
        source.query_row("PRAGMA wal_checkpoint(FULL)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 || (log_pages >= 0 && checkpointed_pages < log_pages) {
        bail!("Không thể chốt WAL; giữ nguyên cơ sở dữ liệu chưa mã hóa để thử lại");
    }
    let key = key().context("Chưa nạp khóa dữ liệu cục bộ")?;
    let hex = key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let attach = format!("ATTACH DATABASE ?1 AS encrypted KEY \"x'{hex}'\"");
    source.execute(&attach, [temp.to_string_lossy().as_ref()])?;
    source.execute_batch("SELECT sqlcipher_export('encrypted');")?;
    source.execute_batch("DETACH DATABASE encrypted")?;
    drop(source);
    let verify = open_cipher_connection(&temp)?;
    verify.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })?;
    drop(verify);
    crate::data_dir::replace_file(&temp, path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(sidecar));
    }
    Ok(())
}

fn open_cipher_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    let key = key().context("Chưa nạp khóa dữ liệu cục bộ")?;
    let hex = key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    conn.execute_batch(&format!(
        "PRAGMA key = \"x'{hex}'\"; PRAGMA cipher_memory_security = ON;"
    ))?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })?;
    Ok(conn)
}

fn managed_path(path: &Path) -> Option<PathBuf> {
    let root = crate::paths::data_dir().canonicalize().ok()?;
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let parent = absolute.parent()?.canonicalize().ok()?;
    let resolved = parent.join(absolute.file_name()?);
    let resolved = if resolved.exists() {
        resolved.canonicalize().ok()?
    } else {
        resolved
    };
    resolved.starts_with(root).then_some(resolved)
}

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
    }
    if output.cbData > 0 && output.pbData.is_null() {
        bail!("Windows không trả về khóa dữ liệu đã bảo vệ");
    }
    let bytes = if output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
    };
    unsafe {
        LocalFree(HLOCAL(output.pbData.cast()));
    }
    Ok(bytes)
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB {
        cbData: blob.len() as u32,
        pbData: blob.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )?;
    }
    if output.cbData > 0 && output.pbData.is_null() {
        bail!("Windows không trả về khóa dữ liệu đã giải mã");
    }
    let bytes = if output.cbData == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() }
    };
    unsafe {
        LocalFree(HLOCAL(output.pbData.cast()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aes256_gcm_matches_nist_known_answer() {
        use aes_gcm::aead::{Aead, KeyInit, Payload};
        use aes_gcm::{Aes256Gcm, Nonce};
        let cipher = Aes256Gcm::new_from_slice(&[0u8; 32]).unwrap();
        let actual = cipher
            .encrypt(
                Nonce::from_slice(&[0u8; 12]),
                Payload {
                    msg: &[0u8; 16],
                    aad: &[],
                },
            )
            .unwrap();
        let expected = [
            0xce, 0xa7, 0x40, 0x3d, 0x4d, 0x60, 0x6b, 0x6e, 0x07, 0x4e, 0xc5, 0xd3, 0xba, 0xf3,
            0x9d, 0x18, 0xd0, 0xd1, 0xc8, 0xa7, 0x99, 0x99, 0x6b, 0xf0, 0x26, 0x5b, 0x98, 0xb5,
            0xd4, 0x8a, 0xb9, 0x19,
        ];
        assert_eq!(actual, expected);
    }
}
