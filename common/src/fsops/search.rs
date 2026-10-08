//! Full-text/filename search over a `FileOps` tree, ported from the host's
//! `fsops::search_stream` -- shared by the AI coding agent's `search_files`
//! tool and (on the host) the Explorer full-text search command. Not an
//! index: every search re-walks and re-reads the unexcluded files, local
//! walks are fast via `std::fs`, remote (SSH/SFTP) walks pay one round trip
//! per file and can be noticeably slower than a local ripgrep-backed search
//! for large trees.

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use roc_desk_core::error::AppError;

use crate::encoding::decode_text_detect;
use super::FileOps;

/// Noise directories (build output/deps/VCS metadata) skipped during a
/// search tree walk, and common binary file extensions skipped during
/// content search -- without these, scanning a tree with `node_modules`
/// would walk tens of thousands of irrelevant files, and reading an
/// image/archive as text would produce garbage matches.
const SEARCH_EXCLUDED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
    ".cargo",
];
const SEARCH_BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "ico", "webp", "bmp", "mp4", "mp3", "wav", "avi", "mov", "zip",
    "tar", "gz", "7z", "rar", "exe", "dll", "pdb", "so", "dylib", "pdf", "woff", "woff2", "ttf",
    "eot", "db", "sqlite", "class", "jar", "wasm",
];
const SEARCH_MAX_FILES: usize = 500;
const SEARCH_MAX_MATCHES: usize = 3000;
const SEARCH_MAX_MATCHES_PER_FILE: usize = 50;
/// Files larger than this are skipped rather than read into memory for
/// regex matching -- guards against accidentally scanning a large
/// log/data file.
const SEARCH_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchOptions {
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub use_regex: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMatch {
    /// 1-based line number (matches how VS Code's search results display line numbers).
    pub line_number: usize,
    pub line_text: String,
    /// Character index, not byte index -- so the frontend can highlight via
    /// `Array.from(line).slice(start, end)` directly.
    pub match_start: usize,
    pub match_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFileResult {
    pub path: String,
    pub matches: Vec<SearchMatch>,
}

/// Search filenames only (directory walk + string match against the name,
/// no file-content I/O) vs. search file content (read every unexcluded
/// file and run the pattern against it).
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    Content,
    FileName,
}

fn build_matcher(query: &str, options: &SearchOptions) -> Result<Regex, AppError> {
    if query.is_empty() {
        return Err(AppError::Internal("搜索内容不能为空".into()));
    }
    let base = if options.use_regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    let pattern = if options.whole_word {
        format!(r"\b{base}\b")
    } else {
        base
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .build()
        .map_err(|e| AppError::Internal(format!("正则表达式无效：{e}")))
}

fn is_excluded_dir(name: &str) -> bool {
    SEARCH_EXCLUDED_DIRS.contains(&name)
}

fn is_binary_extension(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => SEARCH_BINARY_EXTENSIONS.contains(&ext.to_lowercase().as_str()),
        None => false,
    }
}

/// First 8KB containing a NUL byte is treated as binary -- the same
/// heuristic git/ripgrep use, not meant to be 100% accurate, just good
/// enough.
fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8192).any(|b| *b == 0)
}

/// Walks `root` via `file_ops`, calling `on_file` once per matching file as
/// results are found (streaming, rather than collecting into one big
/// `Vec` -- lets a caller push incremental progress to the frontend for a
/// large tree). `should_cancel` is polled between files/directories so a
/// caller can abort a long-running search. Returns `Ok(true)` if the walk
/// was cut short by hitting `SEARCH_MAX_FILES`/`SEARCH_MAX_MATCHES`.
pub async fn search_stream(
    file_ops: &dyn FileOps,
    root: &str,
    query: &str,
    options: &SearchOptions,
    mode: SearchMode,
    mut on_file: impl FnMut(SearchFileResult),
    mut should_cancel: impl FnMut() -> bool,
) -> Result<bool, AppError> {
    let matcher = build_matcher(query, options)?;

    let mut files_found = 0usize;
    let mut total_matches = 0usize;
    let mut truncated = false;
    let mut stack = vec![root.to_string()];

    'walk: while let Some(dir) = stack.pop() {
        if should_cancel() {
            break;
        }
        let entries = match file_ops.list_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue, // one unreadable subdirectory (permissions etc.) shouldn't block the rest
        };
        for entry in entries {
            if should_cancel() {
                break 'walk;
            }
            if entry.is_dir {
                if !is_excluded_dir(&entry.name) {
                    stack.push(entry.path.clone());
                }
                continue;
            }

            match mode {
                SearchMode::FileName => {
                    if let Some(m) = matcher.find(&entry.name) {
                        let match_start = entry.name[..m.start()].chars().count();
                        let match_end = entry.name[..m.end()].chars().count();
                        files_found += 1;
                        total_matches += 1;
                        on_file(SearchFileResult {
                            path: entry.path.clone(),
                            matches: vec![SearchMatch {
                                line_number: 1,
                                line_text: entry.name.clone(),
                                match_start,
                                match_end,
                            }],
                        });
                    }
                }
                SearchMode::Content => {
                    if is_binary_extension(&entry.name) {
                        continue;
                    }
                    if entry
                        .size
                        .map(|s| s > SEARCH_MAX_FILE_BYTES)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    let Ok((bytes, _)) = file_ops.read_file_raw(&entry.path).await else {
                        continue;
                    };
                    if bytes.len() as u64 > SEARCH_MAX_FILE_BYTES || looks_binary(&bytes) {
                        continue;
                    }
                    let (text, _) = decode_text_detect(&bytes);

                    let mut file_matches = Vec::new();
                    'lines: for (idx, line) in text.lines().enumerate() {
                        for m in matcher.find_iter(line) {
                            let match_start = line[..m.start()].chars().count();
                            let match_end = line[..m.end()].chars().count();
                            file_matches.push(SearchMatch {
                                line_number: idx + 1,
                                line_text: line.to_string(),
                                match_start,
                                match_end,
                            });
                            total_matches += 1;
                            if file_matches.len() >= SEARCH_MAX_MATCHES_PER_FILE
                                || total_matches >= SEARCH_MAX_MATCHES
                            {
                                break 'lines;
                            }
                        }
                    }
                    if !file_matches.is_empty() {
                        files_found += 1;
                        on_file(SearchFileResult {
                            path: entry.path.clone(),
                            matches: file_matches,
                        });
                    }
                }
            }

            if files_found >= SEARCH_MAX_FILES || total_matches >= SEARCH_MAX_MATCHES {
                truncated = true;
                break 'walk;
            }
        }
    }

    Ok(truncated)
}
