use crate::protocol::TodayBriefResponse;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

const BRIEFS_DIR_ENV: &str = "TOKEN_USAGE_BRIEFS_DIR";
const APP_SUPPORT_DIR: &str = "Library/Application Support/Token Usage Dashboard";
const BRIEFS_SUBDIR: &str = "briefs";

pub fn briefs_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(BRIEFS_DIR_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err("HOME is not set; cannot locate briefs directory".to_string());
    };
    Ok(home.join(APP_SUPPORT_DIR).join(BRIEFS_SUBDIR))
}

pub fn brief_path_for_date(date: &str) -> Result<PathBuf, String> {
    Ok(briefs_dir()?.join(format!("{date}.json")))
}

pub fn load_brief(date: &str) -> Result<Option<TodayBriefResponse>, String> {
    let path = brief_path_for_date(date)?;
    load_brief_from_path(&path)
}

pub fn load_brief_from_path(path: &Path) -> Result<Option<TodayBriefResponse>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let brief: TodayBriefResponse = serde_json::from_str(&text)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    Ok(Some(brief.normalized()))
}

pub fn save_brief(brief: &TodayBriefResponse) -> Result<PathBuf, String> {
    let path = brief_path_for_date(&brief.date)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(brief)
        .map_err(|err| format!("failed to serialize brief: {err}"))?;
    fs::write(&path, text).map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

pub fn has_successful_brief(date: &str) -> Result<bool, String> {
    Ok(load_brief(date)?
        .map(|brief| brief.status == "ok")
        .unwrap_or(false))
}

#[cfg(test)]
pub(crate) static BRIEFS_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BriefModelInfo, TodayBriefSection};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_briefs_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "token-usage-briefs-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn round_trips_ok_brief_without_api_key() {
        let _guard = BRIEFS_ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let dir = temp_briefs_dir();
        let previous = std::env::var_os(BRIEFS_DIR_ENV);
        std::env::set_var(BRIEFS_DIR_ENV, &dir);

        let brief = TodayBriefResponse {
            date: "2026-07-15".into(),
            status: "ok".into(),
            generated_at: "2026-07-15T10:00:00+08:00".into(),
            trigger: "manual".into(),
            model: BriefModelInfo {
                base_url: "http://127.0.0.1:8317/v1".into(),
                model_id: "deepseek-v4-flash".into(),
            },
            enabled_sources: vec!["claude".into()],
            content_fingerprint: "abc".into(),
            summary: String::new(),
            cards: Vec::new(),
            sections: vec![TodayBriefSection {
                source: "claude".into(),
                headline: "测试".into(),
                bullets: vec!["一条".into()],
                session_count: 1,
                coverage: "full".into(),
            }],
            hours: None,
            error: None,
        };

        save_brief(&brief).unwrap();
        let loaded = load_brief("2026-07-15").unwrap().unwrap();
        assert_eq!(loaded.status, "ok");
        assert_eq!(loaded.cards.len(), 1);
        assert_eq!(loaded.cards[0].headline, "测试");
        assert!(!loaded.summary.is_empty());
        assert!(has_successful_brief("2026-07-15").unwrap());

        restore_env(BRIEFS_DIR_ENV, previous);
        let _ = fs::remove_dir_all(&dir);
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
