//! Workspace symbol index ("go to definition/declaration").
//!
//! Ported from the host's `src-tauri/src/symbols/mod.rs`. Shared by the
//! `roc_desk-editor` and `roc_desk-workspace` tools -- both need "go to
//! definition" over a tree of source files, and neither owns the other, so
//! the pure indexing logic lives here instead of being duplicated.
//!
//! This lives in the `common` package (not `core`) because it depends on the
//! `FileOps` trait, which itself lives in `common::fsops` (local/remote
//! filesystem access ended up there during the explorer migration, not in
//! `core` as the original split plan sketched).
//!
//! Not a real language server: no semantic analysis, no macro expansion, no
//! understanding of overloads or conditional compilation -- just a per-line
//! regex scan for common function/type/macro definition shapes, in the same
//! spirit as ctags. This lets it work identically over local, SSH, and Agent
//! workspaces without needing an external ctags/LSP binary to be present (or
//! shipped) on either end -- it only needs `FileOps::list_dir`/`read_file_raw`.
//!
//! Known precision trade-offs: multi-line function signatures, macro-generated
//! definitions, and complex function-pointer typedefs are missed; name
//! collisions (overloads, same type name in different files) are not
//! deduplicated -- every match is returned and the caller (Monaco's
//! multi-result "peek"/quick-pick UI) lets the user choose.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::fsops::FileOps;
use roc_desk_core::error::AppError;

/// Caps a single index build to this many files -- purely a guard against
/// pathological cases (an extremely large repo, or accidentally opening an
/// entire disk as a workspace), not a limit normal usage will hit.
const MAX_INDEX_FILES: usize = 20_000;
/// Files larger than this are skipped entirely -- generated/bundled source
/// files that large aren't worth regex-scanning and would slow down the
/// overall index build for no benefit.
const MAX_INDEX_FILE_BYTES: u64 = 2 * 1024 * 1024;

const SOURCE_EXTENSIONS: &[&str] = &[
    "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "ino", "rs", "py", "go", "js", "jsx", "ts",
    "tsx", "mjs", "cjs",
];

/// Directories skipped while walking a workspace to build the index --
/// build output / dependency / VCS metadata directories that would otherwise
/// blow up scan time (e.g. `node_modules`) without containing anything worth
/// indexing.
const INDEX_EXCLUDED_DIRS: &[&str] = &[
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

fn is_excluded_dir(name: &str) -> bool {
    INDEX_EXCLUDED_DIRS.contains(&name)
}

/// C/C++ control-flow/statement keywords, used two ways: the function
/// signature regex can misfire on `if (...)`/`while (...)` (excluded when the
/// keyword occupies the "name" capture position); and on `return foo();`/
/// `throw foo();`-style statements being mistaken for a definition of a
/// function named `foo` (excluded when the keyword occupies the whole prefix
/// before the name, see `extract_symbols`).
const CONTROL_KEYWORDS: &[&str] = &[
    "if", "for", "while", "switch", "return", "sizeof", "catch", "else", "do", "defined",
    "static_assert", "__attribute__", "throw", "goto", "delete", "case",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolLocation {
    pub path: String,
    /// 1-based line number, matching Monaco's `Range` convention.
    pub line: u32,
    /// "function"/"type"/"macro"/"typedef" etc, display-only -- the frontend
    /// doesn't currently branch on it.
    pub kind: String,
}

#[derive(Default)]
pub struct SymbolIndex {
    table: HashMap<String, Vec<SymbolLocation>>,
    /// path -> symbol names it contributed, so re-indexing a single file can
    /// remove its stale entries without scanning the whole table.
    by_file: HashMap<String, Vec<String>>,
}

impl SymbolIndex {
    pub fn lookup(&self, name: &str) -> Vec<SymbolLocation> {
        self.table.get(name).cloned().unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.table.values().map(|v| v.len()).sum()
    }

    fn remove_file(&mut self, path: &str) {
        if let Some(names) = self.by_file.remove(path) {
            for name in names {
                if let Some(locs) = self.table.get_mut(&name) {
                    locs.retain(|l| l.path != path);
                    if locs.is_empty() {
                        self.table.remove(&name);
                    }
                }
            }
        }
    }

    /// Incrementally rebuilds the entries for one file -- called after a
    /// save, instead of re-scanning the whole workspace.
    pub fn index_file(&mut self, path: &str, content: &str) {
        self.remove_file(path);
        let symbols = extract_symbols(path, content);
        if symbols.is_empty() {
            return;
        }
        let mut names = Vec::with_capacity(symbols.len());
        for (name, line, kind) in symbols {
            self.table
                .entry(name.clone())
                .or_default()
                .push(SymbolLocation {
                    path: path.to_string(),
                    line,
                    kind: kind.to_string(),
                });
            names.push(name);
        }
        self.by_file.insert(path.to_string(), names);
    }
}

fn is_source_file(name: &str) -> bool {
    name.rsplit_once('.')
        .map(|(_, ext)| SOURCE_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Walks an entire workspace and parses every source file to build a fresh
/// index (called when a workspace is opened / index rebuild is triggered
/// manually). Uses only `FileOps::list_dir`/`read_file_raw` -- local runs
/// through `std::fs` and is fast; remote (SSH/Agent) is one round-trip per
/// file and can be noticeably slower on large workspaces. This is the same
/// known trade-off as the filesystem full-text search feature; making it
/// faster would require running the scan on the remote host itself, which is
/// a follow-up optimization, not in scope here.
pub async fn build_index(file_ops: &dyn FileOps, root: &str) -> Result<SymbolIndex, AppError> {
    let mut index = SymbolIndex::default();
    let mut stack = vec![root.to_string()];
    let mut scanned = 0usize;

    while let Some(dir) = stack.pop() {
        let entries = match file_ops.list_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            if entry.is_dir {
                if !is_excluded_dir(&entry.name) {
                    stack.push(entry.path.clone());
                }
                continue;
            }
            if !is_source_file(&entry.name) {
                continue;
            }
            if entry.size.map(|s| s > MAX_INDEX_FILE_BYTES).unwrap_or(false) {
                continue;
            }
            let Ok((bytes, _mtime)) = file_ops.read_file_raw(&entry.path).await else {
                continue;
            };
            let text = String::from_utf8_lossy(&bytes);
            index.index_file(&entry.path, &text);

            scanned += 1;
            if scanned >= MAX_INDEX_FILES {
                return Ok(index);
            }
        }
    }

    Ok(index)
}

struct LangRules {
    patterns: Vec<(Regex, &'static str)>,
}

macro_rules! lang_rules {
    ($fn_name:ident, $cell:ident, [$(($pat:expr, $kind:expr)),+ $(,)?]) => {
        fn $fn_name() -> &'static LangRules {
            static $cell: OnceLock<LangRules> = OnceLock::new();
            $cell.get_or_init(|| LangRules {
                patterns: vec![$((Regex::new($pat).unwrap(), $kind)),+],
            })
        }
    };
}

lang_rules!(
    c_family_rules,
    C_FAMILY_RULES,
    [
        // Function definition/declaration: `RET_TYPE name(params)`. The
        // brace may be on the same line, or (more commonly, C formatting
        // style) on the next -- the regex doesn't require a trailing `{`.
        // The lazy prefix `[\w:<>,*&\s]+?` eats the return type (which may
        // carry `*`/`&`/namespaces/template args); the name is cut at the
        // last separator (space/`*`/`&`) right before it, so
        // `void *get_uab_lib()` (pointer return type, `*` glued to the name
        // with no space) also matches.
        (
            r"^\s*[\w:<>,*&\s]+?[\s*&]([A-Za-z_]\w*)\s*\(([^;{}]*)\)\s*\{?\s*;?\s*$",
            "function"
        ),
        (r"^\s*(?:typedef\s+)?(?:struct|class|enum|union)\s+([A-Za-z_]\w*)\b", "type"),
        (r"^\s*#\s*define\s+([A-Za-z_]\w*)", "macro"),
        (r"^\s*typedef\b.*[\s*]([A-Za-z_]\w*)\s*;\s*$", "typedef"),
    ]
);

lang_rules!(
    rust_rules,
    RUST_RULES,
    [
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_]\w*)", "function"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Za-z_]\w*)", "type"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+([A-Za-z_]\w*)", "type"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+([A-Za-z_]\w*)", "type"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?type\s+([A-Za-z_]\w*)", "typedef"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Za-z_][A-Za-z0-9_]*)\s*:", "const"),
        (r"^\s*(?:pub(?:\([^)]*\))?\s+)?static\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:", "const"),
    ]
);

lang_rules!(
    python_rules,
    PYTHON_RULES,
    [
        (r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(", "function"),
        (r"^\s*class\s+([A-Za-z_]\w*)\s*[:\(]", "type"),
    ]
);

lang_rules!(
    go_rules,
    GO_RULES,
    [
        (r"^\s*func\s+(?:\([^)]*\)\s*)?([A-Za-z_]\w*)\s*\(", "function"),
        (r"^\s*type\s+([A-Za-z_]\w*)\s+(?:struct|interface)\b", "type"),
    ]
);

lang_rules!(
    js_rules,
    JS_RULES,
    [
        (
            r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\s*\*?\s+([A-Za-z_$][\w$]*)\s*\(",
            "function"
        ),
        (r"^\s*(?:export\s+)?(?:default\s+)?class\s+([A-Za-z_$][\w$]*)", "type"),
        (
            r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?\(",
            "function"
        ),
        (r"^\s*(?:export\s+)?interface\s+([A-Za-z_$][\w$]*)", "type"),
        (r"^\s*(?:export\s+)?type\s+([A-Za-z_$][\w$]*)\s*=", "typedef"),
    ]
);

fn rules_for_extension(ext: &str) -> Option<&'static LangRules> {
    match ext {
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ino" => Some(c_family_rules()),
        "rs" => Some(rust_rules()),
        "py" => Some(python_rules()),
        "go" => Some(go_rules()),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => Some(js_rules()),
        _ => None,
    }
}

/// Runs the language-appropriate rule table over a file's content, line by
/// line -- a line only counts once against the first rule it matches, so the
/// same line can't be double-counted by two overlapping rules (e.g. C's
/// function rule and typedef rule could in theory both "look like" a match
/// at the start of the same line).
fn extract_symbols(path: &str, content: &str) -> Vec<(String, u32, &'static str)> {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    let Some(rules) = rules_for_extension(&ext) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        for (re, kind) in &rules.patterns {
            let Some(caps) = re.captures(line) else {
                continue;
            };
            let Some(whole) = caps.get(0) else { continue };
            let Some(m) = caps.get(1) else { continue };
            let name = m.as_str();
            if name.is_empty() || CONTROL_KEYWORDS.contains(&name) {
                continue;
            }
            // The C function rule's prefix now allows `*`/`&` glued directly
            // to the name (see the comment above), at the cost of statements
            // like `return foo();`/`throw foo();` also matching the
            // "prefix + separator + name + (...)" shape -- excluded by
            // checking whether the whole prefix before the name is itself a
            // control-flow/statement keyword (not a real type).
            let prefix = line[whole.start()..m.start()].trim();
            if CONTROL_KEYWORDS.contains(&prefix) {
                continue;
            }
            out.push((name.to_string(), (idx + 1) as u32, *kind));
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_c_function_with_brace_on_next_line() {
        let content = "static int kgbp_do_signin_via_kuab(KUABCLIHANDLE hHandle)\n{\n    return 0;\n}\n";
        let symbols = extract_symbols("kgmscli_kuabcli.cpp", content);
        assert!(symbols
            .iter()
            .any(|(name, line, kind)| name == "kgbp_do_signin_via_kuab" && *line == 1 && *kind == "function"));
    }

    #[test]
    fn does_not_mistake_control_flow_for_function() {
        let content = "if (get_kmgscli_flg_gm())\n{\n    return 0;\n}\n";
        let symbols = extract_symbols("foo.c", content);
        assert!(symbols.is_empty());
    }

    #[test]
    fn extracts_c_macro_and_struct() {
        let content = "#define MAX_LEN 128\nstruct Foo {\n    int x;\n};\n";
        let symbols = extract_symbols("foo.h", content);
        assert!(symbols.iter().any(|(name, _, kind)| name == "MAX_LEN" && *kind == "macro"));
        assert!(symbols.iter().any(|(name, _, kind)| name == "Foo" && *kind == "type"));
    }

    #[test]
    fn extracts_rust_fn_and_struct() {
        let content = "pub struct Foo;\n\nasync fn do_thing() -> Result<(), Error> {\n    Ok(())\n}\n";
        let symbols = extract_symbols("lib.rs", content);
        assert!(symbols.iter().any(|(name, _, kind)| name == "Foo" && *kind == "type"));
        assert!(symbols.iter().any(|(name, _, kind)| name == "do_thing" && *kind == "function"));
    }

    #[test]
    fn extracts_pointer_return_function_with_star_glued_to_name() {
        let decl = "extern void *get_uab_lib();\n";
        let symbols = extract_symbols("kgmscli_kuabcli.cpp", decl);
        assert!(symbols
            .iter()
            .any(|(name, line, kind)| name == "get_uab_lib" && *line == 1 && *kind == "function"));

        let def = "void *get_uab_lib()\n{\n    return g_lib_handle;\n}\n";
        let symbols = extract_symbols("kgmscli_kuabcli.cpp", def);
        assert!(symbols
            .iter()
            .any(|(name, line, kind)| name == "get_uab_lib" && *line == 1 && *kind == "function"));
    }

    #[test]
    fn does_not_mistake_return_statement_for_function_definition() {
        let content = "int wrapper(void)\n{\n    return get_uab_lib();\n}\n";
        let symbols = extract_symbols("foo.c", content);
        assert!(symbols.iter().any(|(name, _, _)| name == "wrapper"));
        assert!(!symbols.iter().any(|(name, _, _)| name == "get_uab_lib"));
    }

    #[test]
    fn index_file_reindex_replaces_old_entries() {
        let mut index = SymbolIndex::default();
        index.index_file("a.rs", "fn old_name() {}\n");
        assert_eq!(index.lookup("old_name").len(), 1);
        index.index_file("a.rs", "fn new_name() {}\n");
        assert!(index.lookup("old_name").is_empty());
        assert_eq!(index.lookup("new_name").len(), 1);
    }
}
