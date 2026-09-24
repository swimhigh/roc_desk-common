//! Portable (exe-relative) data directory resolution, shared by the host
//! (`roc_desk.exe`) and every standalone tool binary.
//!
//! The host's own convention (see `roc_desk`'s `src-tauri/src/lib.rs`,
//! `resolve_app_data_dir`) puts all mutable state (SQLite files, logs, caches)
//! in a `.rock_desk` directory next to the running executable, not the OS's
//! per-app AppData/Roaming folder — this makes the whole install directory
//! independently movable/backup-able, matches a portable-zip deployment, and
//! (the point that matters here) means several tool executables copied into
//! the *same* directory automatically end up sharing the *same* `.rock_desk`
//! folder, with no extra coordination needed: each tool just uses its own
//! filename inside it (`roc_desk_ssh.db`, `workspace.db`, ...).
//!
//! Before this helper existed, each standalone tool binary rolled its own
//! data-dir logic — several used Tauri's `app.path().app_data_dir()`
//! (OS AppData, keyed by that tool's own bundle id) and one used a bespoke
//! `dirs_next_data_dir()` — so a standalone SSH tool and a standalone SQL
//! tool run side by side would each keep a *different*, OS-specific data
//! directory, and neither would match what the full `roc_desk.exe` host uses
//! for the same underlying data. That's the same "split registry" class of
//! bug this workspace has already hit at the in-process wiring level, just
//! one layer up at the packaging/distribution level (2026-09 user feedback:
//! sub-tool local/remote workspace info must live in the same `.rock_desk`
//! layout as the host, and copying several tool exes into one directory
//! should be enough to make them share it).
pub fn portable_data_dir() -> std::io::Result<std::path::PathBuf> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "无法定位可执行文件所在目录")
        })?
        .to_path_buf();
    let data_dir = exe_dir.join(".rock_desk");
    std::fs::create_dir_all(&data_dir)?;
    Ok(data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_to_current_exe() {
        let dir = portable_data_dir().unwrap();
        assert!(dir.ends_with(".rock_desk"));
        assert!(dir.is_dir());
    }
}
