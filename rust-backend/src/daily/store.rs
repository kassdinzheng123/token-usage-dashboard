use crate::protocol::DailyReport;
use std::{
    fs,
    path::PathBuf,
};

const DAILY_DIR_ENV: &str = "TOKEN_USAGE_DAILY_DIR";
const APP_SUPPORT_DIR: &str = "Library/Application Support/Token Usage Dashboard";

/// Where daily reports are cached, one directory per bound project. The
/// project name is validated by `projects::validate_name`, so the directory
/// segment is a safe filesystem token.
pub fn daily_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os(DAILY_DIR_ENV).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Err("HOME is not set; cannot locate daily reports directory".to_string());
    };
    Ok(home.join(APP_SUPPORT_DIR).join("daily"))
}

pub fn report_path(project: &str, date: &str) -> Result<PathBuf, String> {
    super::projects::validate_name(project)?;
    Ok(daily_dir()?.join(project).join(format!("{date}.json")))
}

pub fn save_report(report: &DailyReport) -> Result<PathBuf, String> {
    let path = report_path(&report.project, &report.date)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(report)
        .map_err(|err| format!("failed to serialize daily report: {err}"))?;
    fs::write(&path, text).map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

pub fn load_report(project: &str, date: &str) -> Result<Option<DailyReport>, String> {
    let path = report_path(project, date)?;
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let report: DailyReport = serde_json::from_str(&text)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))?;
    Ok(Some(report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BriefModelInfo, DailyWorkItem};
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn round_trips_daily_report() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-daily-store-{stamp}"));
        let previous = std::env::var_os(DAILY_DIR_ENV);
        std::env::set_var(DAILY_DIR_ENV, &dir);

        let report = DailyReport {
            date: "2026-08-05".into(),
            project: "token-usage".into(),
            path: "/Users/demo/token-usage".into(),
            status: "ok".into(),
            overview: "完成了日报功能。".into(),
            work_items: vec![DailyWorkItem {
                title: "实现日报生成".into(),
                detail: "新增 daily 模块，聚合会话与提交 (abc1234)。".into(),
            }],
            session_count: 4,
            commit_count: 2,
            token_total: 12_345,
            coverage: "exact".into(),
            generated_at: "2026-08-05T20:00:00+08:00".into(),
            model: BriefModelInfo {
                base_url: "http://127.0.0.1:8317/v1".into(),
                model_id: "test".into(),
            },
            error: None,
        };

        save_report(&report).unwrap();
        let loaded = load_report("token-usage", "2026-08-05").unwrap().unwrap();
        assert_eq!(loaded.overview, report.overview);
        assert_eq!(loaded.work_items.len(), 1);
        assert_eq!(loaded.coverage, "exact");
        assert!(load_report("token-usage", "2026-08-04").unwrap().is_none());

        restore_env(DAILY_DIR_ENV, previous);
        let _ = fs::remove_dir_all(&dir);
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
