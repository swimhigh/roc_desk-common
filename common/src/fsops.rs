use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use roc_desk_core::error::AppError;

use crate::encoding::decode_text_detect;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContent {
    pub text: String,
    /// Detected (or caller-forced) encoding label, shown in the editor status bar.
    pub encoding: String,
    /// Unix timestamp (seconds), used for save-time conflict detection.
    pub mtime: i64,
    /// Total file size in bytes (not `text`'s length -- `text` is only a
    /// truncated preview when `truncated` is true).
    pub total_size: u64,
    /// True when the file exceeded the size threshold and `text` is only a
    /// truncated preview; callers should switch the editor to read-only.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WriteOutcome {
    Written {
        mtime: i64,
    },
    Conflict {
        current_mtime: i64,
        current_preview: String,
    },
}

/// Above this size, editor reads only return a truncated preview instead of
/// loading the whole file into memory (ported from the host's >1GB freeze fix).
pub const EDITOR_PREVIEW_THRESHOLD_BYTES: u64 = 10 * 1024 * 1024;
/// How many bytes to read for a truncated editor preview.
pub const EDITOR_PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;
/// Binary preview (image/PDF/Office) size cap -- these formats can't be
/// meaningfully truncated, so oversized files are rejected outright.
pub const BINARY_PREVIEW_MAX_BYTES: u64 = 30 * 1024 * 1024;
/// EXE/DLL/SO/JAR inspection uses a much larger cap than image/document
/// preview -- real-world executables/fat jars are routinely tens to hundreds
/// of MB.
pub const EXECUTABLE_INSPECT_MAX_BYTES: u64 = 200 * 1024 * 1024;

/// Local/remote-agnostic filesystem contract shared by every tool that reads
/// and writes files (ported from `roc_desk` host `fsops::FileOps`).
///
/// `read_file`/`write_file` are default, encoding-agnostic conveniences for
/// callers that don't care about encoding: auto-detect UTF-8/GBK/UTF-16 on
/// read, fixed UTF-8 on write. Implementations only need to provide the raw
/// byte primitives (`read_file_raw`/`write_file_bytes`); everything else has
/// a default built on top of them.
///
/// Remote-only concerns (SFTP sessions, Windows Agent transport) are
/// intentionally NOT part of this trait yet -- they stay in the `roc_desk`
/// host until `roc_desk-ssh` is split out and the shared boundary is decided.
#[async_trait]
pub trait FileOps: Send + Sync {
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, AppError>;

    /// Raw bytes + mtime, no encoding assumptions.
    async fn read_file_raw(&self, path: &str) -> Result<(Vec<u8>, i64), AppError>;

    /// Stat-only file size, without reading content -- used to decide whether
    /// to take the truncated-preview branch before paying the cost of reading.
    async fn file_size(&self, path: &str) -> Result<u64, AppError>;

    /// Like `read_file_raw` but reads at most `max_bytes`.
    async fn read_file_raw_bounded(
        &self,
        path: &str,
        max_bytes: u64,
    ) -> Result<(Vec<u8>, i64), AppError>;

    /// Same semantics as `write_file` (including conflict detection), but
    /// takes arbitrary bytes instead of assuming UTF-8 text.
    async fn write_file_bytes(
        &self,
        path: &str,
        bytes: &[u8],
        expected_mtime: Option<i64>,
    ) -> Result<WriteOutcome, AppError>;

    /// `is_dir` is supplied by the caller (already has a `FileEntry`) to
    /// avoid an extra stat.
    async fn delete(&self, path: &str, is_dir: bool) -> Result<(), AppError>;

    async fn rename(&self, from: &str, to: &str) -> Result<(), AppError>;

    /// Create a directory including intermediate components (`mkdir -p` semantics).
    async fn create_dir(&self, path: &str) -> Result<(), AppError>;

    async fn read_file(&self, path: &str) -> Result<FileContent, AppError> {
        let (bytes, mtime) = self.read_file_raw(path).await?;
        let total_size = bytes.len() as u64;
        let (text, encoding) = decode_text_detect(&bytes);
        Ok(FileContent {
            text,
            encoding: encoding.to_string(),
            mtime,
            total_size,
            truncated: false,
        })
    }

    /// `expected_mtime` of `None` means no conflict check (e.g. creating a
    /// new file); otherwise a mismatch returns `WriteOutcome::Conflict`
    /// instead of silently overwriting. Default implementation always writes UTF-8.
    async fn write_file(
        &self,
        path: &str,
        content: &str,
        expected_mtime: Option<i64>,
    ) -> Result<WriteOutcome, AppError> {
        self.write_file_bytes(path, content.as_bytes(), expected_mtime)
            .await
    }

    /// Size-aware read for the editor: above `EDITOR_PREVIEW_THRESHOLD_BYTES`
    /// only the first `EDITOR_PREVIEW_MAX_BYTES` are read; `truncated` tells
    /// the caller this is a preview that must not be saved back verbatim.
    async fn read_bytes_for_editor(
        &self,
        path: &str,
    ) -> Result<(Vec<u8>, i64, u64, bool), AppError> {
        let total_size = self.file_size(path).await?;
        if total_size > EDITOR_PREVIEW_THRESHOLD_BYTES {
            let (bytes, mtime) = self
                .read_file_raw_bounded(path, EDITOR_PREVIEW_MAX_BYTES)
                .await?;
            Ok((bytes, mtime, total_size, true))
        } else {
            let (bytes, mtime) = self.read_file_raw(path).await?;
            Ok((bytes, mtime, total_size, false))
        }
    }

    /// String version of `read_bytes_for_editor`, producing the `FileContent`
    /// the editor / `local_read_file` command needs directly.
    async fn read_file_for_editor(&self, path: &str) -> Result<FileContent, AppError> {
        let (bytes, mtime, total_size, truncated) = self.read_bytes_for_editor(path).await?;
        let (text, encoding) = decode_text_detect(&bytes);
        Ok(FileContent {
            text,
            encoding: encoding.to_string(),
            mtime,
            total_size,
            truncated,
        })
    }

    /// Image/binary preview: stat first, reject outright if oversized (a
    /// truncated image/PDF can't be decoded, so there's no point in a partial read).
    async fn read_binary_for_preview(
        &self,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, AppError> {
        let total_size = self.file_size(path).await?;
        if total_size > max_bytes {
            return Err(AppError::Internal(format!(
                "File too large ({:.1}MB) to preview",
                total_size as f64 / 1024.0 / 1024.0
            )));
        }
        let (bytes, _mtime) = self.read_file_raw(path).await?;
        Ok(bytes)
    }

    /// Materializes remote content to a real local path (system "open with"
    /// can't understand SSH/SFTP paths). Default reads the whole file into
    /// memory first; remote implementations should override with a streaming
    /// download.
    async fn download_to_local_file(&self, path: &str, local_path: &str) -> Result<(), AppError> {
        let (bytes, _mtime) = self.read_file_raw(path).await?;
        tokio::fs::write(local_path, &bytes)
            .await
            .map_err(AppError::from)
    }

    /// File: read bytes then write bytes. Directory: create the destination
    /// then recurse over children. Default implementation shared by
    /// local/remote backends -- they only need to provide the basic
    /// `list_dir`/`create_dir`/`read_file_raw`/`write_file_bytes` primitives.
    async fn copy(&self, from: &str, to: &str, is_dir: bool) -> Result<(), AppError> {
        if !is_dir {
            let (bytes, _) = self.read_file_raw(from).await?;
            self.write_file_bytes(to, &bytes, None).await?;
            return Ok(());
        }
        self.create_dir(to).await?;
        let entries = self.list_dir(from).await?;
        for entry in entries {
            let child_to = format!("{}/{}", to.trim_end_matches('/'), entry.name);
            self.copy(&entry.path, &child_to, entry.is_dir).await?;
        }
        Ok(())
    }
}

pub mod local;
pub use local::LocalFileOps;
