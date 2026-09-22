use async_trait::async_trait;
use std::io::Read;
use std::time::UNIX_EPOCH;

use super::{FileEntry, FileOps, WriteOutcome};
use crate::encoding::decode_text;
use roc_desk_core::error::AppError;

#[derive(Debug, Default, Clone, Copy)]
pub struct LocalFileOps;

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[async_trait]
impl FileOps for LocalFileOps {
    async fn read_file_raw(&self, path: &str) -> Result<(Vec<u8>, i64), AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = std::fs::metadata(&path)?;
            let bytes = std::fs::read(&path)?;
            Ok((bytes, mtime_secs(&meta)))
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn file_size(&self, path: &str) -> Result<u64, AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || Ok(std::fs::metadata(&path)?.len()))
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn read_file_raw_bounded(
        &self,
        path: &str,
        max_bytes: u64,
    ) -> Result<(Vec<u8>, i64), AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = std::fs::metadata(&path)?;
            let file = std::fs::File::open(&path)?;
            let mut buf = Vec::with_capacity(max_bytes.min(meta.len()) as usize);
            file.take(max_bytes).read_to_end(&mut buf)?;
            Ok((buf, mtime_secs(&meta)))
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn download_to_local_file(&self, path: &str, local_path: &str) -> Result<(), AppError> {
        let path = path.to_string();
        let local_path = local_path.to_string();
        tokio::task::spawn_blocking(move || {
            std::fs::copy(&path, &local_path)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    /// Overrides the default to merge into a single `spawn_blocking` (the
    /// default calls `file_size()` and `read_file_raw()`/`read_file_raw_bounded()`
    /// as separate blocking tasks, which is pure overhead for the local case).
    async fn read_bytes_for_editor(
        &self,
        path: &str,
    ) -> Result<(Vec<u8>, i64, u64, bool), AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = std::fs::metadata(&path)?;
            let total_size = meta.len();
            let truncated = total_size > super::EDITOR_PREVIEW_THRESHOLD_BYTES;
            if truncated {
                let file = std::fs::File::open(&path)?;
                let mut buf =
                    Vec::with_capacity(super::EDITOR_PREVIEW_MAX_BYTES.min(total_size) as usize);
                file.take(super::EDITOR_PREVIEW_MAX_BYTES)
                    .read_to_end(&mut buf)?;
                Ok((buf, mtime_secs(&meta), total_size, true))
            } else {
                let bytes = std::fs::read(&path)?;
                Ok((bytes, mtime_secs(&meta), total_size, false))
            }
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    /// Same rationale as `read_bytes_for_editor`: merge into one `spawn_blocking`.
    async fn read_binary_for_preview(
        &self,
        path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let meta = std::fs::metadata(&path)?;
            if meta.len() > max_bytes {
                return Err(AppError::Internal(format!(
                    "File too large ({:.1}MB) to preview",
                    meta.len() as f64 / 1024.0 / 1024.0
                )));
            }
            let bytes = std::fs::read(&path)?;
            Ok(bytes)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn write_file_bytes(
        &self,
        path: &str,
        bytes: &[u8],
        expected_mtime: Option<i64>,
    ) -> Result<WriteOutcome, AppError> {
        let path = path.to_string();
        let bytes = bytes.to_vec();
        tokio::task::spawn_blocking(move || {
            // Save-time conflict detection: the file may have been changed
            // externally since it was read.
            if let Some(expected) = expected_mtime {
                if let Ok(meta) = std::fs::metadata(&path) {
                    let current = mtime_secs(&meta);
                    if current != expected {
                        let preview = std::fs::read(&path)
                            .map(|b| decode_text(&b))
                            .unwrap_or_default()
                            .lines()
                            .take(5)
                            .collect::<Vec<_>>()
                            .join("\n");
                        return Ok(WriteOutcome::Conflict {
                            current_mtime: current,
                            current_preview: preview,
                        });
                    }
                }
            }

            std::fs::write(&path, &bytes)?;
            let meta = std::fs::metadata(&path)?;
            Ok(WriteOutcome::Written {
                mtime: mtime_secs(&meta),
            })
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&path)? {
                let entry = entry?;
                let meta = entry.metadata()?;
                let full_path = entry.path();
                entries.push(FileEntry {
                    name: entry.file_name().to_string_lossy().to_string(),
                    path: full_path.to_string_lossy().replace('\\', "/"),
                    is_dir: meta.is_dir(),
                    size: if meta.is_dir() {
                        None
                    } else {
                        Some(meta.len())
                    },
                    modified: Some(mtime_secs(&meta)),
                });
            }
            // Directories first, then alphabetical within each group.
            entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            });
            Ok(entries)
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn delete(&self, path: &str, is_dir: bool) -> Result<(), AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            if is_dir {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn rename(&self, from: &str, to: &str) -> Result<(), AppError> {
        let from = from.to_string();
        let to = to.to_string();
        tokio::task::spawn_blocking(move || {
            std::fs::rename(&from, &to)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }

    async fn create_dir(&self, path: &str) -> Result<(), AppError> {
        let path = path.to_string();
        tokio::task::spawn_blocking(move || {
            std::fs::create_dir_all(&path)?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        tokio::runtime::Runtime::new().unwrap().block_on(fut)
    }

    #[test]
    fn read_file_raw_bounded_stops_at_max_bytes() {
        let dir = std::env::temp_dir().join(format!("roc_desk_common_test_{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("large.txt");
        let content = "0123456789".repeat(1000); // 10_000 bytes
        std::fs::write(&path, &content).unwrap();
        let path_str = path.to_string_lossy().replace('\\', "/");

        let ops = LocalFileOps;
        let total = block_on(ops.file_size(&path_str)).unwrap();
        assert_eq!(total, 10_000);

        let (bytes, _mtime) = block_on(ops.read_file_raw_bounded(&path_str, 100)).unwrap();
        assert_eq!(bytes.len(), 100);
        assert_eq!(&bytes, &content.as_bytes()[..100]);

        let (bytes, _mtime) =
            block_on(ops.read_file_raw_bounded(&path_str, 1_000_000)).unwrap();
        assert_eq!(bytes.len(), 10_000);

        std::fs::remove_dir_all(&dir).ok();
    }

    fn uuid_like() -> u128 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }
}
