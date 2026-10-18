//! 剪贴板历史导入/导出：JSON 明文 + AES-GCM 加密二进制
//!
//! 这个模块**只做序列化 / 反序列化 / 加解密 + 返回 ImportSummary**，
//! 实际的数据库写入（合并 / 替换 / 跳过）由 `app::commands::import_cmd`
//! 在 Task 50 单独完成。两个 import_* 函数本身**不持有**任何 DB 句柄，
//! 仅解析文件并返回 `imported` / `skipped` 计数。
//!
//! ## 加密格式
//!
//! 二进制输出布局（无 base64 包装）：
//!
//! ```text
//! ┌────────────┬────────────┬──────────────────────────┐
//! │ nonce (12) │ salt (16)  │ ciphertext || tag (16)   │
//! └────────────┴────────────┴──────────────────────────┘
//! ```
//!
//! - `nonce` — AES-GCM 12 字节随机数（每次加密独立）
//! - `salt` — Argon2id 16 字节随机盐
//! - `ciphertext || tag` — `cipher.encrypt(nonce, plaintext)` 自身
//!   输出的格式（已含 16 字节认证 tag）
//!
//! ## KDF
//!
//! - 算法：Argon2id（`argon2 = "0.5"`，`Algorithm::Argon2id`）
//! - 参数：`m_cost = 19456 KiB`（19 MiB），`t_cost = 2`，`p_cost = 1`
//!   （OWASP 2024 对交互式登录的推荐值）
//! - 输出：32 字节 → AES-256-GCM key
//!
//! ## Zeroize
//!
//! 派生出的 `[u8; 32]` key 离开作用域前用 `zeroize::Zeroize::zeroize()`
//! 清零，防止内存残留。

use crate::domain::models::ClipboardEntry;
use base64::{engine::general_purpose::STANDARD, Engine};
use std::fmt;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 导出版本号。导入时校验。后续破坏性 schema 升级时递增到 `v2`。
pub const EXPORT_VERSION: &str = "dezirclip-export-v1";
const LEGACY_EXPORT_VERSION: &str = "tiez-export-v1";

/// AES-GCM nonce 长度（字节）。标准 96-bit nonce。
const NONCE_LEN: usize = 12;

/// Argon2id 盐长度（字节）。OWASP 推荐 ≥ 16 字节。
const SALT_LEN: usize = 16;

/// 派生 key 长度（字节）。32 字节 → AES-256。
const KEY_LEN: usize = 32;

/// Argon2id 内存成本（KiB）。19456 KiB = 19 MiB。
const ARGON2_M_COST_KIB: u32 = 19_456;
/// Argon2id 时间成本（迭代轮数）。
const ARGON2_T_COST: u32 = 2;
/// Argon2id 并行度。
const ARGON2_P_COST: u32 = 1;

// ---------------------------------------------------------------------------
// Export types — Serialized into JSON / encrypted blob
// ---------------------------------------------------------------------------

/// 单条剪贴板记录的可导出形态。字段集是 `domain::models::ClipboardEntry`
/// 的子集（去掉运行时态字段如 `file_preview_exists`、`is_external`，
/// 把 `timestamp` 拆成 `created_at` / `updated_at` 两个语义更明确的字段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportEntry {
    pub id: i64,
    pub content_type: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_app_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub use_count: i32,
    #[serde(default)]
    pub is_pinned: bool,
    #[serde(default)]
    pub pinned_order: i64,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_text: Option<String>,
    /// 分类标签集合（v13+ 的 `content_kinds` 列）。
    #[serde(default)]
    pub kinds: Vec<String>,
}

/// 完整导出文件结构。包含 schema 版本号 + 导出时间戳 + 条目列表。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportFile {
    pub version: String,
    pub exported_at: i64,
    pub entries: Vec<ExportEntry>,
}

/// 导入策略。`Merge` 模式按 id 覆盖已有行；`Replace` 模式先清空再写入。
/// 序列化大小写与 Tauri IPC 兼容（snake_case 由 `#[serde(rename_all)]`
/// 控制；此处直接用 PascalCase 以保持 Rust 端 enum 命名习惯）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportMode {
    Merge,
    Replace,
}

impl ImportMode {
    /// 序列化为小写字符串，用于填入 [`ImportSummary::mode`]。
    pub fn as_str(self) -> &'static str {
        match self {
            ImportMode::Merge => "merge",
            ImportMode::Replace => "replace",
        }
    }
}

/// 导入结果摘要。`imported` 实际写入条数；`skipped` 跳过条数
/// （按 id 冲突的 Merge 模式保留旧行）。`mode` 是策略字符串，
/// 方便前端无歧义地展示。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
    pub mode: String,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// 所有 backup 模块错误。`From` impl 允许 `?` 直接传播底层错误。
#[derive(Debug)]
pub enum BackupError {
    /// `serde_json` 解析/序列化失败。
    JsonError(serde_json::Error),
    /// `aes-gcm` 加解密失败（认证 tag 不匹配 / 长度错误 / …）。
    CryptoError(aes_gcm::Error),
    /// Argon2id key 派生失败。`hash_password_into` 唯一会返回的错误是
    /// 输出长度非法；这里打包成 `CryptoError` 不太准确，所以单列。
    /// 当前实现下不会触发，仅保留以备 `Params::new` 校验变更。
    KdfError(argon2::Error),
    /// 解密成功但 tag 校验通过、但 passphrase 错误（key 错误 → tag
    /// 校验失败 → 走到 `CryptoError`）。这里显式覆盖为 `WrongPassphrase`，
    /// 提示 UI "密码错误" 而非 "数据损坏"。
    WrongPassphrase,
    /// 导出文件头 `version` 不匹配。
    InvalidFormat(String),
    /// `IoError` 变体保留为对外契约一部分，**当前模块不直接做 I/O**，
    /// 调用方（`import_cmd` / 写入文件那一步）可能用得到。
    IoError(std::io::Error),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackupError::JsonError(e) => write!(f, "json error: {e}"),
            BackupError::CryptoError(e) => write!(f, "crypto error: {e}"),
            BackupError::KdfError(e) => write!(f, "key derivation error: {e}"),
            BackupError::WrongPassphrase => write!(f, "wrong passphrase"),
            BackupError::InvalidFormat(msg) => write!(f, "invalid export format: {msg}"),
            BackupError::IoError(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for BackupError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BackupError::JsonError(e) => Some(e),
            BackupError::IoError(e) => Some(e),
            // `aes_gcm::Error` and `argon2::Error` implement `Display`
            // but not `std::error::Error`, so they can't appear in the
            // `source()` chain. Their messages are still surfaced via
            // our own `Display` impl above.
            BackupError::CryptoError(_)
            | BackupError::KdfError(_)
            | BackupError::WrongPassphrase
            | BackupError::InvalidFormat(_) => None,
        }
    }
}

impl From<serde_json::Error> for BackupError {
    fn from(e: serde_json::Error) -> Self {
        BackupError::JsonError(e)
    }
}

impl From<aes_gcm::Error> for BackupError {
    fn from(e: aes_gcm::Error) -> Self {
        BackupError::CryptoError(e)
    }
}

impl From<argon2::Error> for BackupError {
    fn from(e: argon2::Error) -> Self {
        BackupError::KdfError(e)
    }
}

impl From<std::io::Error> for BackupError {
    fn from(e: std::io::Error) -> Self {
        BackupError::IoError(e)
    }
}

// ---------------------------------------------------------------------------
// Public API — JSON
// ---------------------------------------------------------------------------

/// 把条目列表序列化成 pretty JSON 字符串（`to_string_pretty`）。
/// 顶端包裹一个 [`ExportFile`]，带 `version` + `exported_at`。
///
/// 错误：仅在 `serde_json` 序列化失败时返回（实际上 `String` 字段
/// 不会有序列化失败，所以这条路径基本不会触发，但保留 `Result`
/// 以保持 API 一致性）。
pub fn export_to_json(entries: Vec<ExportEntry>) -> Result<String, BackupError> {
    let file = ExportFile {
        version: EXPORT_VERSION.to_string(),
        exported_at: now_unix_secs(),
        entries,
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

/// 解析导出 JSON，校验 `version` 字段，返回 [`ImportSummary`]。
/// **不写数据库**。调用方（`import_cmd`）拿到 summary 后再决定
/// 如何落库（merge 走 upsert、replace 走 delete-all + insert）。
pub fn import_from_json(json: &str, mode: ImportMode) -> Result<ImportSummary, BackupError> {
    let file: ExportFile = serde_json::from_str(json)?;
    validate_version(&file.version)?;
    Ok(ImportSummary {
        imported: file.entries.len(),
        skipped: 0,
        mode: mode.as_str().to_string(),
    })
}

/// 从 JSON 字符串解析条目并校验版本，避免调用方遗漏版本保护。
/// 用于 `import_cmd` 在拿到 summary 后还需要条目列表落库的场景。
pub fn entries_from_json(json: &str) -> Result<Vec<ExportEntry>, BackupError> {
    let file: ExportFile = serde_json::from_str(json)?;
    validate_version(&file.version)?;
    Ok(file.entries)
}

/// 解密加密 blob，返回解密后的 UTF-8 JSON 字符串。
/// 与 `import_from_encrypted` 的区别：不解析 JSON，只负责解密+转码，
/// 调用方可以额外调用 `entries_from_json` 拿到条目列表。
pub fn decrypt_to_json(data: &[u8], passphrase: &str) -> Result<String, BackupError> {
    if data.len() < NONCE_LEN + SALT_LEN {
        return Err(BackupError::InvalidFormat(format!(
            "blob too short: {} < {}",
            data.len(),
            NONCE_LEN + SALT_LEN
        )));
    }
    let ciphertext = &data[NONCE_LEN + SALT_LEN..];
    if ciphertext.len() < 16 {
        return Err(BackupError::InvalidFormat(format!(
            "blob missing auth tag: ciphertext length {} < 16",
            ciphertext.len()
        )));
    }

    let nonce_bytes = &data[..NONCE_LEN];
    let salt = &data[NONCE_LEN..NONCE_LEN + SALT_LEN];

    let mut key = derive_key(passphrase.as_bytes(), salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = match cipher.decrypt(nonce, ciphertext) {
        Ok(p) => p,
        Err(_) => {
            key.zeroize();
            return Err(BackupError::WrongPassphrase);
        }
    };

    key.zeroize();

    let json = std::str::from_utf8(&plaintext).map_err(|_| {
        BackupError::InvalidFormat("decrypted payload is not valid UTF-8".to_string())
    })?;
    Ok(json.to_string())
}

// ---------------------------------------------------------------------------
// Public API — Encrypted
// ---------------------------------------------------------------------------

/// 用 passphrase 加密导出条目，返回二进制 blob。
/// 格式见模块顶部文档。
pub fn export_to_encrypted(
    entries: Vec<ExportEntry>,
    passphrase: &str,
) -> Result<Vec<u8>, BackupError> {
    if passphrase.is_empty() {
        return Err(BackupError::InvalidFormat(
            "passphrase must not be empty".to_string(),
        ));
    }

    let json = export_to_json(entries)?;

    // 1. 随机 nonce + salt
    let mut nonce_bytes = [0u8; NONCE_LEN];
    let mut salt = [0u8; SALT_LEN];
    fill_random(&mut nonce_bytes)?;
    fill_random(&mut salt)?;

    // 2. 派生 key
    let mut key = derive_key(passphrase.as_bytes(), &salt)?;

    // 3. AES-GCM 加密
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, json.as_bytes())?;

    // 4. 拼接输出：[nonce | salt | ciphertext || tag]
    let mut out = Vec::with_capacity(NONCE_LEN + SALT_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&ciphertext);

    // 5. 离开作用域前清零 key
    key.zeroize();

    Ok(out)
}

/// 解密加密 blob，解析并返回 [`ImportSummary`]。
/// 错误的 passphrase 会产生 AES-GCM 认证 tag 校验失败 →
/// `aes_gcm::Error` → `BackupError::WrongPassphrase`。
/// 其他解析/版本错误走对应变体。
pub fn import_from_encrypted(data: &[u8], passphrase: &str) -> Result<ImportSummary, BackupError> {
    if data.len() < NONCE_LEN + SALT_LEN {
        return Err(BackupError::InvalidFormat(format!(
            "blob too short: {} < {}",
            data.len(),
            NONCE_LEN + SALT_LEN
        )));
    }
    let ciphertext = &data[NONCE_LEN + SALT_LEN..];
    // AES-GCM 的 ciphertext 必须至少包含 16 字节认证 tag。空 ciphertext
    // 走到 decrypt 会得到一个模糊的 crypto 错误，重写为 WrongPassphrase
    // 后会误导用户。先在这里拒绝 → InvalidFormat，语义更清晰。
    if ciphertext.len() < 16 {
        return Err(BackupError::InvalidFormat(format!(
            "blob missing auth tag: ciphertext length {} < 16",
            ciphertext.len()
        )));
    }

    let nonce_bytes = &data[..NONCE_LEN];
    let salt = &data[NONCE_LEN..NONCE_LEN + SALT_LEN];

    let mut key = derive_key(passphrase.as_bytes(), salt)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = match cipher.decrypt(nonce, ciphertext) {
        Ok(p) => p,
        Err(_) => {
            // 认证 tag 不匹配 = 99% 是密码错。这里把通用 crypto
            // 错误重写成 WrongPassphrase，前端就能显示"密码错误"
            // 而不是误导性的"文件损坏"。
            key.zeroize();
            return Err(BackupError::WrongPassphrase);
        }
    };

    key.zeroize();

    let json = std::str::from_utf8(&plaintext).map_err(|_| {
        BackupError::InvalidFormat("decrypted payload is not valid UTF-8".to_string())
    })?;
    // import_from_json 默认 Merge；加密导入当前没有 mode 参数，
    // 因为这是"恢复"语义——调用方拿到 summary 后通常走 replace。
    import_from_json(json, ImportMode::Merge)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Argon2id KDF → 32 字节 key。
/// `Params::new` 是 `const fn` 且参数都已通过编译期检查，
/// 所以 `Result` 路径只会在 `output_len > 2^32` 等极端情况下触发。
fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], BackupError> {
    let params = Params::new(
        ARGON2_M_COST_KIB,
        ARGON2_T_COST,
        ARGON2_P_COST,
        Some(KEY_LEN),
    )
    .map_err(|e| BackupError::InvalidFormat(format!("argon2 params invalid: {e}")))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon2.hash_password_into(passphrase, salt, &mut key)?;
    Ok(key)
}

/// `rand` 0.8 的 OS RNG 填充。
fn fill_random(buf: &mut [u8]) -> Result<(), BackupError> {
    rand::thread_rng().fill_bytes(buf);
    Ok(())
}

/// 校验导出文件头的 `version` 字段。
fn validate_version(version: &str) -> Result<(), BackupError> {
    if version != EXPORT_VERSION && version != LEGACY_EXPORT_VERSION {
        return Err(BackupError::InvalidFormat(format!(
            "expected version {EXPORT_VERSION}, got {version}"
        )));
    }
    Ok(())
}

/// Unix 秒（导出时间戳）。独立函数便于测试中固定。
fn now_unix_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Convert decoded domain data; database callers use `query_cli_for_export` first.
pub fn prepare_export_entry(e: &ClipboardEntry) -> Result<ExportEntry, String> {
    let content = if e.content_type == "image" {
        let bytes = crate::infrastructure::attachments::image_bytes(&e.content)
            .map_err(|error| format!("export: image attachment id={}: {error}", e.id))?;
        let format = image::guess_format(&bytes)
            .map_err(|error| format!("export: invalid image attachment id={}: {error}", e.id))?;
        image::load_from_memory(&bytes)
            .map_err(|error| format!("export: invalid image attachment id={}: {error}", e.id))?;
        if e.content.starts_with("data:image/") {
            e.content.clone()
        } else {
            format!(
                "data:{};base64,{}",
                format.to_mime_type(),
                STANDARD.encode(bytes)
            )
        }
    } else {
        e.content.clone()
    };

    Ok(ExportEntry {
        id: e.id,
        content_type: e.content_type.clone(),
        content,
        preview: Some(e.preview.clone()),
        html_content: e.html_content.clone(),
        source_app: if e.source_app.is_empty() {
            None
        } else {
            Some(e.source_app.clone())
        },
        source_app_path: e.source_app_path.clone(),
        // The domain model exposes one timestamp for both backup fields.
        created_at: e.timestamp,
        updated_at: e.timestamp,
        use_count: e.use_count,
        is_pinned: e.is_pinned,
        pinned_order: e.pinned_order,
        tags: e.tags.clone(),
        ocr_text: e.ocr_text.clone(),
        kinds: e.content_kinds.clone(),
    })
}

pub fn restore_entry(exp: &crate::services::backup::ExportEntry) -> ClipboardEntry {
    ClipboardEntry {
        id: exp.id,
        content_type: exp.content_type.clone(),
        content: exp.content.clone(),
        html_content: exp.html_content.clone(),
        source_app: exp.source_app.clone().unwrap_or_default(),
        source_app_path: exp.source_app_path.clone(),
        timestamp: exp.updated_at,
        // `preview` is `Option<String>` in the export schema but
        // `String` in the domain model — collapse `None` to empty
        // and let the repo re-derive a fresh preview on next read.
        preview: exp.preview.clone().unwrap_or_default(),
        is_pinned: exp.is_pinned,
        tags: exp.tags.clone(),
        use_count: exp.use_count,
        is_external: false,
        pinned_order: exp.pinned_order,
        file_preview_exists: false,
        content_kinds: exp.kinds.clone(),
        ocr_text: exp.ocr_text.clone(),
        ocr_status: exp.ocr_text.as_ref().map(|_| "done".to_string()),
    }
}

pub fn apply_entries(
    repo: &crate::infrastructure::repository::clipboard_repo::SqliteClipboardRepository,
    entries: &[ExportEntry],
    mode: ImportMode,
) -> Result<(), String> {
    let mut restored: Vec<ClipboardEntry> = entries.iter().map(restore_entry).collect();
    // Validate every image before replacing history. Imported bytes stay inside
    // SQLite, so rollback cannot leave partially replaced external attachments.
    for entry in &mut restored {
        if entry.content_type == "image" {
            let bytes = crate::infrastructure::attachments::image_bytes(&entry.content)
                .map_err(|error| format!("import: image id={}: {error}", entry.id))?;
            image::load_from_memory(&bytes)
                .map_err(|error| format!("import: invalid image id={}: {error}", entry.id))?;
            if !entry.content.starts_with("data:image/") {
                let format = image::guess_format(&bytes).map_err(|error| error.to_string())?;
                entry.content = format!(
                    "data:{};base64,{}",
                    format.to_mime_type(),
                    STANDARD.encode(bytes)
                );
            }
            entry.is_external = false;
        }
    }
    repo.import_cli_entries(&restored, mode == ImportMode::Replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个非平凡的 ExportEntry，用作所有 roundtrip 测试的 fixture。
    fn make_entry(i: usize) -> ExportEntry {
        ExportEntry {
            id: i as i64,
            content_type: if i % 2 == 0 {
                "text".into()
            } else {
                "code".into()
            },
            content: format!("entry #{i} content 你好 🎉"),
            preview: Some(format!("entry #{i}")),
            html_content: if i % 3 == 0 {
                Some(format!("<b>entry {i}</b>"))
            } else {
                None
            },
            source_app: Some(format!("App{i}")),
            source_app_path: None,
            created_at: 1_700_000_000 + i as i64,
            updated_at: 1_700_000_001 + i as i64,
            use_count: i as i32,
            is_pinned: i % 7 == 0,
            pinned_order: if i % 7 == 0 { i as i64 } else { 0 },
            tags: (0..(i % 3)).map(|t| format!("tag{t}")).collect(),
            ocr_text: if i % 5 == 0 {
                Some(format!("ocr-{i}"))
            } else {
                None
            },
            kinds: vec!["text".into(), "code".into()],
        }
    }

    fn make_entries(n: usize) -> Vec<ExportEntry> {
        (0..n).map(make_entry).collect()
    }

    #[test]
    fn test_export_version_constant() {
        assert_eq!(EXPORT_VERSION, "dezirclip-export-v1");
        assert_eq!(LEGACY_EXPORT_VERSION, "tiez-export-v1");
    }

    #[test]
    fn test_export_import_json_roundtrip() {
        let entries = make_entries(100);
        let json = export_to_json(entries.clone()).expect("export json");
        // sanity: pretty JSON 至少包含 version 字段
        assert!(json.contains("\"version\""));
        assert!(json.contains(EXPORT_VERSION));

        // 重新解析
        let file: ExportFile = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(file.version, EXPORT_VERSION);
        assert_eq!(file.entries.len(), 100);
        assert_eq!(file.entries[0].id, 0);
        assert_eq!(file.entries[99].id, 99);
        assert_eq!(file.entries, entries);

        // import_from_json 返回的 summary 正确
        let summary = import_from_json(&json, ImportMode::Merge).expect("import json");
        assert_eq!(summary.imported, 100);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.mode, "merge");

        // Replace 模式 summary 也只反映 count（DB 清空是 import_cmd 的活）
        let summary2 = import_from_json(&json, ImportMode::Replace).expect("import json");
        assert_eq!(summary2.imported, 100);
        assert_eq!(summary2.mode, "replace");
    }

    #[test]
    fn test_export_import_encrypted_roundtrip() {
        let entries = make_entries(100);
        let blob = export_to_encrypted(entries.clone(), "correct horse battery staple")
            .expect("export encrypted");

        // blob 必须以 nonce (12) + salt (16) + 至少 16 字节 tag 开头
        assert!(blob.len() >= NONCE_LEN + SALT_LEN + 16);
        // ciphertext 长度至少 = json 长度 + 16 字节 tag
        let json = export_to_json(entries).expect("re-export for size check");
        assert!(blob.len() >= NONCE_LEN + SALT_LEN + json.len() + 16);

        let summary =
            import_from_encrypted(&blob, "correct horse battery staple").expect("import encrypted");
        assert_eq!(summary.imported, 100);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.mode, "merge");
    }

    #[test]
    fn test_wrong_passphrase_errors() {
        let entries = make_entries(5);
        let blob = export_to_encrypted(entries, "right password 123").expect("export");

        let result = import_from_encrypted(&blob, "wrong password 456");
        assert!(
            matches!(result, Err(BackupError::WrongPassphrase)),
            "expected WrongPassphrase, got {result:?}"
        );
    }

    #[test]
    fn test_replace_mode_clears_first() {
        // 模块只返回 summary；"clears first" 语义由 import_cmd 实现。
        // 这里验证 Replace 模式返回的 summary.mode 字段正确标记，
        // 且 imported = 实际条目数（下游据此清空 + 写入）。
        let entries = make_entries(42);
        let json = export_to_json(entries).expect("export");
        let summary = import_from_json(&json, ImportMode::Replace).expect("import");
        assert_eq!(summary.mode, "replace");
        assert_eq!(summary.imported, 42);
        assert_eq!(summary.skipped, 0);
    }

    #[test]
    fn test_merge_mode_upserts_by_id() {
        // 同上，模块本身不落库；验证 summary 字段 + entries 完整性。
        let entries = make_entries(17);
        let json = export_to_json(entries).expect("export");

        // 解析后检查 id 唯一性（这是 merge 模式 upsert 的前提）
        let file: ExportFile = serde_json::from_str(&json).expect("parse");
        let mut ids: Vec<i64> = file.entries.iter().map(|e| e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 17, "ids should be unique within a merge-set");

        let summary = import_from_json(&json, ImportMode::Merge).expect("import");
        assert_eq!(summary.mode, "merge");
        assert_eq!(summary.imported, 17);
    }

    #[test]
    fn test_encrypted_output_not_plaintext() {
        // 加密输出必须是二进制。原始文本里的 UTF-8 多字节序列
        // 出现概率为 0，所以任何明文 JSON 段存在就说明加密失败。
        let entries = make_entries(50);
        let json = export_to_json(entries).expect("export json");
        // 提取一段独一无二的明文 token
        let probe = "entry #42 content 你好 🎉";
        assert!(json.contains(probe), "json should contain probe verbatim");

        let blob = export_to_encrypted(make_entries(50), "pw").expect("encrypt");
        // 多种方式尝试找明文泄漏
        let as_str = String::from_utf8_lossy(&blob);
        assert!(
            !as_str.contains(probe),
            "encrypted blob must not contain plaintext probe (found via lossy utf8)"
        );
        // 直接尝试反序列化：必须失败
        let parsed: Result<serde_json::Value, _> = serde_json::from_slice(&blob);
        assert!(
            parsed.is_err(),
            "encrypted blob must NOT be valid JSON, got {parsed:?}"
        );
        // ExportFile 直接解析也应失败
        let parsed_file: Result<ExportFile, _> = serde_json::from_slice(&blob);
        assert!(
            parsed_file.is_err(),
            "encrypted blob must NOT parse as ExportFile"
        );
    }

    // -- extra cross-cutting tests ----------------------------------------

    #[test]
    fn test_empty_passphrase_rejected() {
        let result = export_to_encrypted(make_entries(1), "");
        assert!(
            matches!(result, Err(BackupError::InvalidFormat(_))),
            "empty passphrase must be rejected, got {result:?}"
        );
    }

    #[test]
    fn test_truncated_blob_rejected() {
        let blob = export_to_encrypted(make_entries(1), "pw").expect("export");
        // 只保留 nonce + salt，不够 AES-GCM 块
        let truncated = &blob[..NONCE_LEN + SALT_LEN];
        let result = import_from_encrypted(truncated, "pw");
        assert!(
            matches!(result, Err(BackupError::InvalidFormat(_))),
            "truncated blob must be InvalidFormat, got {result:?}"
        );
    }

    #[test]
    fn test_wrong_version_rejected() {
        let bad_json = r#"{
            "version": "tiez-export-v999",
            "exported_at": 0,
            "entries": []
        }"#;
        let result = import_from_json(bad_json, ImportMode::Merge);
        assert!(
            matches!(result, Err(BackupError::InvalidFormat(_))),
            "wrong version must be InvalidFormat, got {result:?}"
        );
    }

    #[test]
    fn test_legacy_tiez_export_version_still_imports() {
        let legacy_json = r#"{
            "version": "tiez-export-v1",
            "exported_at": 0,
            "entries": []
        }"#;
        let summary = import_from_json(legacy_json, ImportMode::Merge).expect("legacy import");
        assert_eq!(summary.imported, 0);
    }

    #[test]
    fn test_corrupted_ciphertext_errors() {
        // 加密 → 把 ciphertext 区间一个字节翻转为 0 → 应当 tag 校验失败
        // → 走到 WrongPassphrase（按 design doc 行为）。
        let mut blob = export_to_encrypted(make_entries(3), "pw").expect("export");
        let last = blob.len() - 1;
        blob[last] ^= 0xFF;
        let result = import_from_encrypted(&blob, "pw");
        assert!(
            matches!(result, Err(BackupError::WrongPassphrase)),
            "tampered ciphertext must yield WrongPassphrase, got {result:?}"
        );
    }

    #[test]
    fn test_import_mode_as_str() {
        assert_eq!(ImportMode::Merge.as_str(), "merge");
        assert_eq!(ImportMode::Replace.as_str(), "replace");
    }

    #[test]
    fn test_error_display() {
        let cases: Vec<(BackupError, &str)> =
            vec![(BackupError::WrongPassphrase, "wrong passphrase")];
        for (err, expected_substr) in cases {
            let s = err.to_string();
            assert!(
                s.contains(expected_substr),
                "Display output {s:?} should contain {expected_substr:?}"
            );
        }
    }

    #[test]
    fn test_unicode_in_entries_roundtrips() {
        let entries = vec![ExportEntry {
            id: 1,
            content_type: "text".into(),
            content: "你好世界 🌍 — UTF-8 multi-byte".into(),
            preview: None,
            html_content: None,
            source_app: None,
            source_app_path: None,
            created_at: 1_700_000_000,
            updated_at: 1_700_000_000,
            use_count: 0,
            is_pinned: false,
            pinned_order: 0,
            tags: vec!["emoji".into(), "中文".into()],
            ocr_text: Some("OCR with é and ñ".into()),
            kinds: vec!["text".into()],
        }];
        let json = export_to_json(entries.clone()).expect("json");
        let parsed: ExportFile = serde_json::from_str(&json).expect("re-parse");
        assert_eq!(parsed.entries[0].content, entries[0].content);
        assert_eq!(parsed.entries[0].tags, vec!["emoji", "中文"]);

        let blob = export_to_encrypted(entries, "密码 with spaces").expect("encrypt");
        let s = import_from_encrypted(&blob, "密码 with spaces").expect("decrypt");
        assert_eq!(s.imported, 1);
    }
}

#[cfg(test)]
mod data_integrity_tests {
    use super::*;
    use crate::infrastructure::repository::{
        cli_query::CliQuery,
        clipboard_repo::{ClipboardRepository, SqliteClipboardRepository},
    };
    use std::sync::{Arc, Mutex};

    fn repository() -> (SqliteClipboardRepository, Arc<Mutex<rusqlite::Connection>>) {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::infrastructure::repository::migrations::run_migrations(&mut conn).unwrap();
        let conn = Arc::new(Mutex::new(conn));
        (SqliteClipboardRepository::new(conn.clone()), conn)
    }

    fn entry(id: i64, kind: &str, content: String) -> ClipboardEntry {
        serde_json::from_value(serde_json::json!({
            "id":id,"content_type":kind,"content":content,"source_app":"fixture",
            "timestamp":1700000000123i64,"preview":"summary","is_pinned":true,
            "pinned_order":i64::MAX-100,"tags":["work"],"use_count":23,
            "content_kinds":["text","code"],"ocr_text":"识别结果","ocr_status":"done"
        }))
        .unwrap()
    }

    fn rows(repo: &SqliteClipboardRepository) -> Vec<ClipboardEntry> {
        repo.query_cli(&CliQuery {
            limit: i32::MAX,
            ..Default::default()
        })
        .unwrap()
    }

    fn png() -> Vec<u8> {
        let mut data = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(2, 2)
            .write_to(&mut data, image::ImageFormat::Png)
            .unwrap();
        data.into_inner()
    }

    #[test]
    fn literal_encryption_markers_in_plaintext_fields_export_without_changes() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-marker-export-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let (repo, _) = repository();
        for prefix in ["linux:", "dpapi:"] {
            let mut source = entry(7, "rich_text", format!("{prefix} kernel notes\r\n"));
            source.preview = format!("{prefix} ordinary preview");
            source.html_content = Some(format!("{prefix}<p>ordinary HTML</p>"));
            repo.import_cli_entries(&[source.clone()], true).unwrap();
            let path = directory.join("history.json");
            crate::cli::export::run(
                &crate::cli::export::ExportArgs {
                    path: path.to_string_lossy().into_owned(),
                    encrypted: false,
                    passphrase: None,
                    quiet: true,
                },
                &repo,
            )
            .unwrap();
            let exported = entries_from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
            assert_eq!(exported[0].content, source.content);
            assert_eq!(
                exported[0].preview.as_deref(),
                Some(source.preview.as_str())
            );
            assert_eq!(exported[0].html_content, source.html_content);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn shared_backup_roundtrip_preserves_long_html_images_and_metadata() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-roundtrip-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let source_image = directory.join("原图100%.png");
        let image = png();
        std::fs::write(&source_image, &image).unwrap();
        let (source, _) = repository();
        let mut rich = entry(
            80,
            "rich_text",
            format!("  {}\r\n\n", "中文🙂".repeat(5000)),
        );
        rich.html_content = Some(format!("<p>{}</p>", "<b>保真</b>".repeat(3000)));
        source
            .import_cli_entries(
                &[
                    rich.clone(),
                    entry(5, "image", source_image.to_string_lossy().into_owned()),
                ],
                true,
            )
            .unwrap();
        let backup = directory.join("history.json");
        let exported = rows(&source)
            .iter()
            .map(prepare_export_entry)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        crate::infrastructure::attachments::write_backup(
            &backup,
            export_to_json(exported).unwrap().as_bytes(),
        )
        .unwrap();
        std::fs::remove_file(&source_image).unwrap();
        let parsed = entries_from_json(&std::fs::read_to_string(&backup).unwrap()).unwrap();
        let (restored, _) = repository();
        apply_entries(&restored, &parsed, ImportMode::Replace).unwrap();
        let output = rows(&restored);
        let restored_rich = output.iter().find(|item| item.id == 80).unwrap();
        assert_eq!(restored_rich.content, rich.content);
        assert_eq!(restored_rich.html_content, rich.html_content);
        assert_eq!(restored_rich.pinned_order, rich.pinned_order);
        assert_eq!(restored_rich.use_count, 23);
        assert_eq!(restored_rich.content_kinds, rich.content_kinds);
        assert_eq!(restored_rich.ocr_text, rich.ocr_text);
        assert_eq!(restored_rich.tags, rich.tags);
        assert_eq!(
            crate::infrastructure::attachments::image_bytes(
                &output.iter().find(|item| item.id == 5).unwrap().content
            )
            .unwrap(),
            image
        );
        let encrypted = export_to_encrypted(parsed.clone(), "roundtrip passphrase").unwrap();
        assert_eq!(
            entries_from_json(&decrypt_to_json(&encrypted, "roundtrip passphrase").unwrap())
                .unwrap(),
            parsed
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_restore_rolls_back_rows_tags_and_preserves_existing_image_files() {
        let directory =
            std::env::temp_dir().join(format!("dezirclip-rollback-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let (repo, connection) = repository();
        let mut original = entry(
            0,
            "image",
            format!("data:image/png;base64,{}", STANDARD.encode(png())),
        );
        original.is_pinned = false;
        let id = repo.save(&original, Some(&directory)).unwrap();
        let before = rows(&repo);
        let attachment = std::path::PathBuf::from(&before[0].content);
        let bytes = std::fs::read(&attachment).unwrap();
        connection.lock().unwrap().execute_batch("CREATE TRIGGER reject_import BEFORE UPDATE ON clipboard_history WHEN new.id=42 BEGIN SELECT RAISE(ABORT,'injected import failure'); END;").unwrap();
        let imported = vec![
            prepare_export_entry(&entry(
                10,
                "image",
                format!("data:image/png;base64,{}", STANDARD.encode(png())),
            ))
            .unwrap(),
            prepare_export_entry(&entry(42, "text", "second".to_string())).unwrap(),
        ];
        assert!(apply_entries(&repo, &imported, ImportMode::Replace)
            .unwrap_err()
            .contains("injected import failure"));
        let after = rows(&repo);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, id);
        assert_eq!(after[0].content, before[0].content);
        assert_eq!(after[0].tags, before[0].tags);
        let count: i64 = connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM entry_tags WHERE entry_id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(std::fs::read(&attachment).unwrap(), bytes);
        let mut invalid = imported[0].clone();
        invalid.content = "data:image/png;base64,broken".to_string();
        assert!(apply_entries(&repo, &[invalid], ImportMode::Replace).is_err());
        assert_eq!(rows(&repo)[0].id, id);
        assert_eq!(std::fs::read(attachment).unwrap(), bytes);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_ids_and_future_backup_formats_cannot_replace_existing_history() {
        let (repo, _) = repository();
        repo.import_cli_entries(&[entry(7, "text", "existing".to_string())], true)
            .unwrap();
        let good = prepare_export_entry(&entry(3, "text", "new".to_string())).unwrap();
        for imported in [
            vec![{
                let mut item = good.clone();
                item.id = 0;
                item
            }],
            vec![good.clone(), good.clone()],
        ] {
            assert!(apply_entries(&repo, &imported, ImportMode::Replace).is_err());
            assert_eq!(rows(&repo)[0].content, "existing");
        }
        let json = export_to_json(vec![good])
            .unwrap()
            .replace(EXPORT_VERSION, "dezirclip-export-v99");
        assert!(entries_from_json(&json).is_err());
        assert!(import_from_json(&json, ImportMode::Replace).is_err());
        assert_eq!(rows(&repo)[0].id, 7);
    }

    #[test]
    fn invalid_stored_images_are_not_portable_backups() {
        let mut original = entry(7, "rich_text", "plain text".to_string());
        original.content_type = "image".to_string();
        original.content = "data:image/png;base64,bm90IGFuIGltYWdl".to_string();
        assert!(prepare_export_entry(&original).is_err());
    }

    fn unavailable_ciphertexts() -> Vec<String> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };
        let cipher = Aes256Gcm::new_from_slice(&[173; 32]).unwrap();
        let nonce = [197; 12];
        let encrypted = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                b"foreign-key private payload".as_slice(),
            )
            .unwrap();
        let mut bytes = nonce.to_vec();
        bytes.extend(encrypted);
        let values = vec![format!("linux:{}", STANDARD.encode(bytes))];
        #[cfg(windows)]
        let values = {
            let mut values = values;
            let native =
                crate::infrastructure::encryption::encrypt_value("native private payload").unwrap();
            let mut bytes = STANDARD
                .decode(native.strip_prefix("dpapi:").unwrap())
                .unwrap();
            *bytes.last_mut().unwrap() ^= 1;
            values.push(format!("dpapi:{}", STANDARD.encode(bytes)));
            values
        };
        values
    }

    #[test]
    fn unavailable_encrypted_fields_abort_export_and_preserve_previous_backup() {
        let directory = std::env::temp_dir().join(format!(
            "dezirclip-failed-decrypt-export-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&directory).unwrap();
        let (repo, connection) = repository();
        for cipher in unavailable_ciphertexts() {
            assert!(crate::infrastructure::encryption::decrypt_value(&cipher).is_none());
            for field in ["content", "preview", "html_content"] {
                repo.import_cli_entries(
                    &[
                        entry(7, "rich_text", "safe text".to_string()),
                        entry(8, "text", "visible entry".to_string()),
                    ],
                    true,
                )
                .unwrap();
                connection.lock().unwrap().execute(&format!("UPDATE clipboard_history SET {field}=?1,tags='[\"sensitive\"]' WHERE id=7"),[&cipher]).unwrap();
                for encrypted in [false, true] {
                    let path = directory.join("previous.json");
                    std::fs::write(&path, b"previous usable backup").unwrap();
                    let error = crate::cli::export::run(
                        &crate::cli::export::ExportArgs {
                            path: path.to_string_lossy().into_owned(),
                            encrypted,
                            passphrase: Some("valid backup passphrase".to_string()),
                            quiet: true,
                        },
                        &repo,
                    )
                    .unwrap_err();
                    assert!(error.contains("could not be decrypted"), "{field}: {error}");
                    assert!(!error.contains(&cipher));
                    assert_eq!(std::fs::read(path).unwrap(), b"previous usable backup");
                }
                let filtered = repo
                    .query_cli_for_export(&CliQuery {
                        limit: i32::MAX,
                        exclude_sensitive: true,
                        ..Default::default()
                    })
                    .unwrap();
                assert_eq!(filtered.len(), 1);
                assert_eq!(filtered[0].content, "visible entry");
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn successfully_decrypted_sensitive_marker_fields_remain_plain_export_content() {
        let (repo, connection) = repository();
        let mut source = entry(7, "rich_text", "linux: kernel notes\r\n".to_string());
        source.preview = "dpapi: ordinary preview".to_string();
        source.html_content = Some("linux:<p>HTML</p>".to_string());
        source.tags = vec!["sensitive".to_string()];
        repo.import_cli_entries(&[source.clone()], true).unwrap();
        let encrypted: Vec<_> = [
            source.content.as_str(),
            source.preview.as_str(),
            source.html_content.as_deref().unwrap(),
        ]
        .iter()
        .map(|value| crate::infrastructure::encryption::encrypt_value(value).unwrap())
        .collect();
        connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE clipboard_history SET content=?1,preview=?2,html_content=?3 WHERE id=7",
                rusqlite::params![encrypted[0], encrypted[1], encrypted[2]],
            )
            .unwrap();
        let decoded = repo
            .query_cli_for_export(&CliQuery {
                limit: i32::MAX,
                ..Default::default()
            })
            .unwrap();
        let exported = prepare_export_entry(&decoded[0]).unwrap();
        assert_eq!(exported.content, source.content);
        assert_eq!(exported.preview.as_deref(), Some(source.preview.as_str()));
        assert_eq!(exported.html_content, source.html_content);
        let json = export_to_json(vec![exported]).unwrap();
        let (restored, _) = repository();
        apply_entries(
            &restored,
            &entries_from_json(&json).unwrap(),
            ImportMode::Replace,
        )
        .unwrap();
        let output = restored
            .query_cli_for_export(&CliQuery {
                limit: i32::MAX,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(output[0].content, source.content);
        assert_eq!(output[0].html_content, source.html_content);
    }

    #[test]
    fn restored_sensitive_rich_text_uses_the_repository_encryption_path() {
        let (repo, connection) = repository();
        let mut source = entry(9, "rich_text", "  私人文本\r\n".to_string());
        source.preview = "私人摘要".to_string();
        source.html_content = Some("<p>私人文本</p>".to_string());
        source.tags = vec!["sensitive".to_string()];
        apply_entries(
            &repo,
            &[prepare_export_entry(&source).unwrap()],
            ImportMode::Replace,
        )
        .unwrap();
        let restored = rows(&repo);
        assert_eq!(restored[0].content, source.content);
        assert_eq!(restored[0].html_content, source.html_content);
        if crate::infrastructure::encryption::encrypt_value(&source.content).is_some() {
            let stored: (String, String, String) = connection
                .lock()
                .unwrap()
                .query_row(
                    "SELECT content,preview,html_content FROM clipboard_history WHERE id=9",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            assert!([&stored.0, &stored.1, &stored.2]
                .iter()
                .all(|value| crate::infrastructure::encryption::is_encrypted_value(value)));
        }
        assert!(repo
            .query_cli(&CliQuery {
                limit: 100,
                exclude_sensitive: true,
                ..Default::default()
            })
            .unwrap()
            .is_empty());
    }
}
