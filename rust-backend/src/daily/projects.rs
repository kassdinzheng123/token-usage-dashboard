use crate::protocol::ProjectBinding;
use chrono::{Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

const PROJECTS_PATH_ENV: &str = "TOKEN_USAGE_PROJECTS_PATH";
const APP_SUPPORT_DIR: &str = "Library/Application Support/Token Usage Dashboard";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ProjectsFile {
    #[serde(default)]
    projects: Vec<ProjectBinding>,
}

/// Where the project bindings live. Overridable for tests.
pub fn projects_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(PROJECTS_PATH_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err("HOME is not set; cannot locate projects file".to_string());
    };
    Ok(home.join(APP_SUPPORT_DIR).join("projects.json"))
}

fn load_file() -> Result<ProjectsFile, String> {
    let path = projects_path()?;
    if !path.is_file() {
        return Ok(ProjectsFile::default());
    }
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn save_file(file: &ProjectsFile) -> Result<PathBuf, String> {
    let path = projects_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(file)
        .map_err(|err| format!("failed to serialize projects: {err}"))?;
    fs::write(&path, text).map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

pub fn list_projects() -> Result<Vec<ProjectBinding>, String> {
    Ok(load_file()?.projects)
}

pub fn find_project(name: &str) -> Result<Option<ProjectBinding>, String> {
    Ok(load_file()?
        .projects
        .into_iter()
        .find(|binding| binding.name == name))
}

/// Adds or updates a binding. The path must exist as a directory; the stored
/// path is made absolute without canonicalizing (macOS resolves `/var` to
/// `/private/var`, which would not match the paths CLIs record). The name
/// must be a safe filesystem token because it becomes the daily report cache
/// directory name. Existing aliases are preserved when re-binding the same
/// name; an alias equal to the new primary path is dropped to avoid a path
/// matching itself twice.
pub fn upsert_project(name: &str, path: &str) -> Result<ProjectBinding, String> {
    let name = name.trim().to_string();
    validate_name(&name)?;
    let resolved = resolve_project_path(path)?;

    let mut file = load_file()?;
    let template = ProjectBinding {
        name: name.clone(),
        path: resolved.clone(),
        aliases: Vec::new(),
        added_at: Local::now().to_rfc3339_opts(SecondsFormat::Secs, false),
    };
    let binding = match file.projects.iter_mut().find(|entry| entry.name == name) {
        Some(entry) => {
            let preserved: Vec<String> = entry
                .aliases
                .iter()
                .filter(|alias| normalize_path(alias) != normalize_path(&resolved))
                .cloned()
                .collect();
            let mut next = template.clone();
            next.aliases = preserved;
            next.added_at = entry.added_at.clone();
            *entry = next.clone();
            next
        }
        None => {
            file.projects.push(template.clone());
            template
        }
    };
    file.projects
        .sort_by(|left, right| left.name.cmp(&right.name));
    save_file(&file)?;
    Ok(binding)
}

/// Adds `alias_path` as an alias of the bound project. The path must be an
/// existing directory distinct from the primary path and existing aliases.
pub fn add_alias(name: &str, alias_path: &str) -> Result<ProjectBinding, String> {
    let name = name.trim().to_string();
    let alias = resolve_project_path(alias_path)?;
    let alias_norm = normalize_path(&alias);

    let mut file = load_file()?;
    let updated = {
        let binding = file
            .projects
            .iter_mut()
            .find(|entry| entry.name == name)
            .ok_or_else(|| format!("no such project: {name}"))?;
        if normalize_path(&binding.path) == alias_norm {
            return Err("alias is already the primary path".to_string());
        }
        if binding
            .aliases
            .iter()
            .any(|existing| normalize_path(existing) == alias_norm)
        {
            return Ok(binding.clone());
        }
        binding.aliases.push(alias);
        binding.clone()
    };
    save_file(&file)?;
    Ok(updated)
}

/// Removes an alias from the bound project. No-op (returns the binding) if
/// the path is not a registered alias; errors if it is the primary path.
pub fn remove_alias(name: &str, alias_path: &str) -> Result<ProjectBinding, String> {
    let name = name.trim().to_string();
    let alias_norm = normalize_path(alias_path.trim());
    if alias_norm.is_empty() {
        return Err("alias path is required".to_string());
    }
    let mut file = load_file()?;
    let updated = {
        let binding = file
            .projects
            .iter_mut()
            .find(|entry| entry.name == name)
            .ok_or_else(|| format!("no such project: {name}"))?;
        if normalize_path(&binding.path) == alias_norm {
            return Err("cannot remove the primary path; remove the project instead".to_string());
        }
        let before = binding.aliases.len();
        binding
            .aliases
            .retain(|existing| normalize_path(existing) != alias_norm);
        if binding.aliases.len() == before {
            return Ok(binding.clone());
        }
        binding.clone()
    };
    save_file(&file)?;
    Ok(updated)
}

/// Folds the `source` binding into `target`: the source's primary path and
/// aliases become aliases of `target`, then the source binding is removed.
/// Paths already covered by `target` (primary or existing aliases) are
/// skipped. Cached daily reports under the source name are reassigned to the
/// target so their history is preserved.
pub fn merge_projects(source: &str, target: &str) -> Result<Vec<ProjectBinding>, String> {
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return Err("source and target names are required".to_string());
    }
    if source == target {
        return Err("source and target must be different projects".to_string());
    }

    let mut file = load_file()?;
    let source_binding = file
        .projects
        .iter()
        .find(|entry| entry.name == source)
        .cloned()
        .ok_or_else(|| format!("no such project: {source}"))?;
    if !file.projects.iter().any(|entry| entry.name == target) {
        return Err(format!("no such project: {target}"));
    }

    // Collect the source's full path set, deduplicated and normalized-collapsed.
    let mut incoming: Vec<String> = Vec::with_capacity(1 + source_binding.aliases.len());
    let push_unique = |path: String, list: &mut Vec<String>| {
        let norm = normalize_path(&path);
        if norm.is_empty() {
            return;
        }
        if !list.iter().any(|existing| normalize_path(existing) == norm) {
            list.push(path);
        }
    };
    push_unique(source_binding.path, &mut incoming);
    for alias in &source_binding.aliases {
        push_unique(alias.clone(), &mut incoming);
    }

    for entry in file.projects.iter_mut() {
        if entry.name != target {
            continue;
        }
        for path in &incoming {
            let norm = normalize_path(path);
            if norm == normalize_path(&entry.path) {
                continue;
            }
            if entry
                .aliases
                .iter()
                .any(|existing| normalize_path(existing) == norm)
            {
                continue;
            }
            entry.aliases.push(path.clone());
        }
        break;
    }

    file.projects.retain(|entry| entry.name != source);
    file.projects
        .sort_by(|left, right| left.name.cmp(&right.name));
    save_file(&file)?;

    // Preserve cached reports: move <source>/<date>.json into <target>/.
    let _ = super::store::reassign_project_reports(source, target);

    Ok(file.projects)
}

pub fn remove_project(name: &str) -> Result<Vec<ProjectBinding>, String> {
    let mut file = load_file()?;
    let before = file.projects.len();
    file.projects.retain(|binding| binding.name != name);
    if file.projects.len() == before {
        return Err(format!("no such project: {name}"));
    }
    save_file(&file)?;
    Ok(file.projects)
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project name is required".to_string());
    }
    if name == "." || name == ".." {
        return Err(format!("invalid project name: {name}"));
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err("project name may only contain letters, digits, '-', '_', '.'".to_string());
    }
    Ok(())
}

/// Resolves a user-supplied path to an absolute, non-canonicalized string
/// and verifies it is an existing directory.
fn resolve_project_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("project path is required".to_string());
    }
    let expanded = expand_home(trimmed);
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|err| format!("failed to resolve current directory: {err}"))?
            .join(expanded)
    };
    if !absolute.is_dir() {
        return Err(format!(
            "project path is not a directory: {}",
            absolute.display()
        ));
    }
    Ok(absolute.to_string_lossy().into_owned())
}

pub(crate) fn normalize_path(path: &str) -> String {
    path.trim().trim_end_matches('/').to_string()
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn temp_paths() -> (PathBuf, PathBuf, PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-projects-{stamp}"));
        let projects_file = dir.join("projects.json");
        let project_dir = dir.join("token-usage");
        let alias_dir = dir.join("token-usage-worktree");
        fs::create_dir_all(&project_dir).unwrap();
        fs::create_dir_all(&alias_dir).unwrap();
        (projects_file, project_dir, alias_dir)
    }

    #[test]
    fn upsert_lists_and_removes_bindings() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (file, dir, _alias) = temp_paths();
        let previous = std::env::var_os(PROJECTS_PATH_ENV);
        std::env::set_var(PROJECTS_PATH_ENV, &file);

        let binding = upsert_project("token-usage", dir.to_str().unwrap()).unwrap();
        assert_eq!(binding.name, "token-usage");
        assert!(binding.path.ends_with("token-usage"));
        assert!(binding.aliases.is_empty());

        // Upserting an existing name updates its path in place.
        let other = dir.parent().unwrap();
        upsert_project("token-usage", other.to_str().unwrap()).unwrap();
        assert_eq!(list_projects().unwrap().len(), 1);
        assert_eq!(
            find_project("token-usage").unwrap().unwrap().path,
            other.to_string_lossy()
        );

        upsert_project("summer", other.to_str().unwrap()).unwrap();
        assert_eq!(list_projects().unwrap().len(), 2);

        let remaining = remove_project("token-usage").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "summer");
        assert!(remove_project("missing").is_err());

        restore_env(PROJECTS_PATH_ENV, previous);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn add_and_remove_aliases() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (file, dir, alias_dir) = temp_paths();
        let previous = std::env::var_os(PROJECTS_PATH_ENV);
        std::env::set_var(PROJECTS_PATH_ENV, &file);

        upsert_project("token-usage", dir.to_str().unwrap()).unwrap();
        let binding = add_alias("token-usage", alias_dir.to_str().unwrap()).unwrap();
        assert_eq!(binding.aliases.len(), 1);
        assert!(binding.aliases[0].ends_with("token-usage-worktree"));

        // Idempotent: adding the same alias again is a no-op.
        let again = add_alias("token-usage", alias_dir.to_str().unwrap()).unwrap();
        assert_eq!(again.aliases.len(), 1);

        // The primary path cannot become an alias.
        assert!(add_alias("token-usage", dir.to_str().unwrap()).is_err());

        let removed = remove_alias("token-usage", alias_dir.to_str().unwrap()).unwrap();
        assert!(removed.aliases.is_empty());
        // Removing the primary path via remove_alias is rejected.
        assert!(remove_alias("token-usage", dir.to_str().unwrap()).is_err());

        restore_env(PROJECTS_PATH_ENV, previous);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn merge_folds_source_paths_into_target_aliases() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (file, dir, alias_dir) = temp_paths();
        let previous = std::env::var_os(PROJECTS_PATH_ENV);
        std::env::set_var(PROJECTS_PATH_ENV, &file);

        upsert_project("alpha", dir.to_str().unwrap()).unwrap();
        upsert_project("beta", alias_dir.to_str().unwrap()).unwrap();

        let remaining = merge_projects("beta", "alpha").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].name, "alpha");
        assert!(remaining[0]
            .aliases
            .iter()
            .any(|alias| alias.ends_with("token-usage-worktree")));
        assert!(find_project("beta").unwrap().is_none());

        // Merging a missing source or self is rejected.
        assert!(merge_projects("beta", "alpha").is_err());
        assert!(merge_projects("alpha", "alpha").is_err());

        restore_env(PROJECTS_PATH_ENV, previous);
        let _ = fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn rejects_unsafe_names_and_missing_paths() {
        assert!(upsert_project("a/b", "/tmp").is_err());
        assert!(upsert_project("..", "/tmp").is_err());
        assert!(upsert_project("", "/tmp").is_err());
        assert!(upsert_project("ok-name_1", "/definitely/not/a/dir-xyz").is_err());
        assert!(upsert_project("ok-name_1", "   ").is_err());
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
