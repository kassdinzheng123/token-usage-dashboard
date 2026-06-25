use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode {
    DryRun,
    Apply,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupAction {
    pub path: PathBuf,
}

const APP_SUPPORT_DIR: &str = "Library/Application Support/Token Usage Dashboard";
const ALLOWED_DIR_NAMES: &[&str] = &["usage", "cache", "tmp", "backup"];
const ALLOWED_FILE_SUFFIXES: &[&str] = &[".tmp", ".bak", ".backup", ".old"];

pub fn plan_cleanup() -> Vec<CleanupAction> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };

    let root = home.join(APP_SUPPORT_DIR);
    plan_cleanup_in(&root)
}

pub fn run_cleanup(mode: CleanupMode) -> Result<Vec<CleanupAction>, String> {
    let Some(home) = home_dir() else {
        return Ok(Vec::new());
    };

    let root = home.join(APP_SUPPORT_DIR);
    run_cleanup_in(&root, mode)
}

fn run_cleanup_in(root: &Path, mode: CleanupMode) -> Result<Vec<CleanupAction>, String> {
    let actions = plan_cleanup_in(root);
    for action in &actions {
        if !is_app_owned_path(&action.path, &root) {
            return Err(format!(
                "refusing to clean non app-owned path: {}",
                action.path.display()
            ));
        }
    }

    match mode {
        CleanupMode::DryRun => Ok(actions),
        CleanupMode::Apply => {
            for action in &actions {
                fs::remove_file(&action.path)
                    .map_err(|err| format!("failed to remove {}: {err}", action.path.display()))?;
            }
            Ok(actions)
        }
    }
}

fn plan_cleanup_in(root: &Path) -> Vec<CleanupAction> {
    let mut actions = Vec::new();
    if !root.is_dir() {
        return actions;
    }

    collect_candidates(root, false, root, &mut actions);
    actions.sort_by(|left, right| left.path.cmp(&right.path));
    actions.dedup_by(|left, right| left.path == right.path);
    actions
}

fn collect_candidates(
    current: &Path,
    under_allowlisted_dir: bool,
    root: &Path,
    actions: &mut Vec<CleanupAction>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if file_type.is_dir() {
            if is_allowed_dir_name(&name) {
                collect_candidates(&path, true, root, actions);
            }
            continue;
        }

        if file_type.is_file()
            && is_allowed_file_name(&name, under_allowlisted_dir)
            && is_app_owned_path(&path, root)
        {
            actions.push(CleanupAction { path });
        }
    }
}

fn is_allowed_dir_name(name: &str) -> bool {
    ALLOWED_DIR_NAMES.iter().any(|allowed| *allowed == name)
}

fn is_allowed_file_name(name: &str, under_allowlisted_dir: bool) -> bool {
    let lower = name.to_ascii_lowercase();
    let has_safe_suffix = ALLOWED_FILE_SUFFIXES
        .iter()
        .any(|suffix| lower.ends_with(suffix));

    if !has_safe_suffix {
        return false;
    }

    if under_allowlisted_dir {
        return true;
    }

    ALLOWED_DIR_NAMES
        .iter()
        .any(|needle| lower.contains(needle))
}

fn is_app_owned_path(path: &Path, root: &Path) -> bool {
    if !path.starts_with(root) {
        return false;
    }

    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };

    for component in relative.components() {
        let Some(part) = component.as_os_str().to_str() else {
            return false;
        };

        if part.starts_with('.') {
            return false;
        }
    }

    true
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn plan_cleanup_excludes_user_owned_dot_dirs() {
        let home = temp_home();
        let root = home.join(APP_SUPPORT_DIR);

        fs::create_dir_all(root.join("usage")).unwrap();
        fs::create_dir_all(root.join("cache")).unwrap();
        fs::create_dir_all(root.join("tmp")).unwrap();
        fs::create_dir_all(root.join("backup")).unwrap();
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::create_dir_all(root.join(".opencode")).unwrap();
        fs::create_dir_all(root.join(".hermes")).unwrap();
        fs::create_dir_all(root.join(".openclaw")).unwrap();

        write_file(root.join("usage-ledger.sqlite.tmp"));
        write_file(root.join("cache").join("stale.backup"));
        write_file(root.join("tmp").join("session.tmp"));
        write_file(root.join("backup").join("old.bak"));
        write_file(root.join(".claude").join("keep.tmp"));
        write_file(root.join(".codex").join("keep.tmp"));
        write_file(root.join(".opencode").join("keep.tmp"));
        write_file(root.join(".hermes").join("keep.tmp"));
        write_file(root.join(".openclaw").join("keep.tmp"));

        let actions = plan_cleanup_in(&root);
        let paths: Vec<_> = actions.iter().map(|action| action.path.clone()).collect();

        assert!(paths.contains(&root.join("usage-ledger.sqlite.tmp")));
        assert!(paths.contains(&root.join("cache").join("stale.backup")));
        assert!(paths.contains(&root.join("tmp").join("session.tmp")));
        assert!(paths.contains(&root.join("backup").join("old.bak")));

        for blocked in [
            root.join(".claude"),
            root.join(".codex"),
            root.join(".opencode"),
            root.join(".hermes"),
            root.join(".openclaw"),
        ] {
            assert!(
                paths.iter().all(|path| !path.starts_with(&blocked)),
                "unexpected path under {}",
                blocked.display()
            );
        }

        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn apply_cleanup_removes_planned_files() {
        let home = temp_home();
        let root = home.join(APP_SUPPORT_DIR);

        write_file(root.join("cache").join("stale.tmp"));
        let actions = run_cleanup_in(&root, CleanupMode::Apply).unwrap();
        assert_eq!(actions.len(), 1);

        assert!(!root.join("cache").join("stale.tmp").exists());
        let _ = fs::remove_dir_all(&home);
    }

    fn temp_home() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("token-usage-cleanup-{stamp}"))
    }

    fn write_file(path: PathBuf) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"tmp").unwrap();
    }
}
