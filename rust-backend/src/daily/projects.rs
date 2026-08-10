use crate::protocol::ProjectBinding;
use chrono::{Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
};

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
    serde_json::from_str(&text)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
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
/// directory name.
pub fn upsert_project(name: &str, path: &str) -> Result<ProjectBinding, String> {
    let name = name.trim().to_string();
    validate_name(&name)?;
    let raw = path.trim();
    if raw.is_empty() {
        return Err("project path is required".to_string());
    }
    let expanded = expand_home(raw);
    let expanded = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|err| format!("failed to resolve current directory: {err}"))?
            .join(expanded)
    };
    if !expanded.is_dir() {
        return Err(format!(
            "project path is not a directory: {}",
            expanded.display()
        ));
    }
    let binding = ProjectBinding {
        name: name.clone(),
        path: expanded.to_string_lossy().into_owned(),
        added_at: Local::now().to_rfc3339_opts(SecondsFormat::Secs, false),
    };
    let mut file = load_file()?;
    match file.projects.iter_mut().find(|entry| entry.name == name) {
        Some(entry) => *entry = binding.clone(),
        None => file.projects.push(binding.clone()),
    }
    file.projects.sort_by(|left, right| left.name.cmp(&right.name));
    save_file(&file)?;
    Ok(binding)
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

    fn temp_paths() -> (PathBuf, PathBuf) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-projects-{stamp}"));
        let projects_file = dir.join("projects.json");
        let project_dir = dir.join("token-usage");
        fs::create_dir_all(&project_dir).unwrap();
        (projects_file, project_dir)
    }

    #[test]
    fn upsert_lists_and_removes_bindings() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let (file, dir) = temp_paths();
        let previous = std::env::var_os(PROJECTS_PATH_ENV);
        std::env::set_var(PROJECTS_PATH_ENV, &file);

        let binding = upsert_project("token-usage", dir.to_str().unwrap()).unwrap();
        assert_eq!(binding.name, "token-usage");
        assert!(binding.path.ends_with("token-usage"));

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
