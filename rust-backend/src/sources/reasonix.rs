use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{home_dir, iso8601_to_local_parts, to_i64, LocalSession, SourceError};
use chrono::{DateTime, Local};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const REASONIX_HOME_ENV: &str = "REASONIX_HOME";
const TELEMETRY_SUFFIX: &str = ".jsonl.telemetry.json";
const UNKNOWN_MODEL: &str = "unknown";

pub fn load_sessions(watermark_ms: Option<i64>) -> Result<Vec<LocalSession>, SourceError> {
    let Some(root) = discover_root() else {
        return Ok(Vec::new());
    };
    load_sessions_from_root(&root, watermark_ms)
}

fn discover_root() -> Option<PathBuf> {
    std::env::var_os(REASONIX_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".reasonix")))
        .filter(|path| path.is_dir())
}

fn load_sessions_from_root(
    root: &Path,
    watermark_ms: Option<i64>,
) -> Result<Vec<LocalSession>, SourceError> {
    let mut telemetry_files = Vec::new();
    collect_telemetry_files(&root.join("projects"), &mut telemetry_files)?;
    telemetry_files.sort();

    let mut sessions = Vec::new();
    for path in telemetry_files {
        if watermark_ms.is_some_and(|watermark| !super::file_modified_after(&path, watermark)) {
            continue;
        }
        if let Some(session) = load_session(&path) {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

fn collect_telemetry_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), SourceError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_telemetry_files(&path, files)?;
        } else if file_type.is_file()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(TELEMETRY_SUFFIX))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn load_session(telemetry_path: &Path) -> Option<LocalSession> {
    let telemetry: Value = serde_json::from_slice(&fs::read(telemetry_path).ok()?).ok()?;
    let usage = telemetry.get("usage")?;
    let prompt_tokens = to_i64(usage.get("promptTokens").unwrap_or(&Value::Null)).max(0);
    let completion_tokens = to_i64(usage.get("completionTokens").unwrap_or(&Value::Null)).max(0);
    let cache_hit_tokens = to_i64(usage.get("cacheHitTokens").unwrap_or(&Value::Null)).max(0);
    let cache_miss_tokens = to_i64(usage.get("cacheMissTokens").unwrap_or(&Value::Null)).max(0);

    let cache_read_tokens = if prompt_tokens > 0 {
        cache_hit_tokens.min(prompt_tokens)
    } else {
        cache_hit_tokens
    };
    let input_tokens = if prompt_tokens > 0 {
        prompt_tokens.saturating_sub(cache_read_tokens)
    } else {
        cache_miss_tokens
    };
    let output_tokens = completion_tokens;
    let reported_total = to_i64(usage.get("totalTokens").unwrap_or(&Value::Null)).max(0);
    let calculated_total = input_tokens + output_tokens + cache_read_tokens;
    if reported_total.max(calculated_total) <= 0 {
        return None;
    }

    let meta = read_meta(telemetry_path);
    let session_id = meta
        .as_ref()
        .and_then(|value| non_empty_string(value, "id"))
        .or_else(|| session_id_from_path(telemetry_path))?;
    let model_name = meta
        .as_ref()
        .and_then(|value| non_empty_string(value, "model"))
        .or_else(|| non_empty_string(&telemetry, "model"))
        .unwrap_or_else(|| UNKNOWN_MODEL.to_string());
    let (date, time) = meta
        .as_ref()
        .and_then(|value| non_empty_string(value, "created_at"))
        .and_then(|timestamp| iso8601_to_local_parts(&timestamp))
        .or_else(|| modified_local_parts(telemetry_path))?;
    let token_usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens,
    };

    Some(LocalSession {
        session_id: format!("reasonix:{session_id}"),
        date,
        time,
        model_name: model_name.clone(),
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens,
        total_tokens_override: (reported_total > 0).then_some(reported_total),
        total_cost: model_cost_usd(&model_name, token_usage),
        model_breakdowns: Vec::new(),
    })
}

fn read_meta(telemetry_path: &Path) -> Option<Value> {
    let file_name = telemetry_path.file_name()?.to_str()?;
    let base_name = file_name.strip_suffix(".telemetry.json")?;
    let meta_path = telemetry_path.with_file_name(format!("{base_name}.meta"));
    serde_json::from_slice(&fs::read(meta_path).ok()?).ok()
}

fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_name()?
        .to_str()?
        .strip_suffix(TELEMETRY_SUFFIX)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

fn non_empty_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn modified_local_parts(path: &Path) -> Option<(String, String)> {
    let modified: DateTime<Local> = fs::metadata(path).ok()?.modified().ok()?.into();
    Some((
        modified.format("%Y-%m-%d").to_string(),
        modified.format("%H:%M").to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::load_sessions_from_root;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("token-usage-reasonix-{nonce}"));
        fs::create_dir_all(root.join("projects/project-a/sessions")).unwrap();
        root
    }

    #[test]
    fn loads_mutable_session_telemetry_without_double_counting_cache_or_reasoning() {
        let root = fixture_root();
        let sessions = root.join("projects/project-a/sessions");
        let base = sessions.join("session-1.jsonl");
        fs::write(
            base.with_extension("jsonl.telemetry.json"),
            r#"{
                "usage": {
                    "promptTokens": 100,
                    "completionTokens": 15,
                    "totalTokens": 115,
                    "reasoningTokens": 5,
                    "cacheHitTokens": 70,
                    "cacheMissTokens": 30
                },
                "sources": {"executor": {"promptTokens": 100}}
            }"#,
        )
        .unwrap();
        fs::write(
            base.with_extension("jsonl.meta"),
            r#"{
                "id": "stable-session-id",
                "created_at": "2026-07-31T12:19:08Z",
                "model": "cursor/composer-2.5",
                "revision": 2
            }"#,
        )
        .unwrap();

        let rows = load_sessions_from_root(&root, None).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.session_id, "reasonix:stable-session-id");
        assert_eq!(row.model_name, "cursor/composer-2.5");
        assert_eq!(row.input_tokens, 30);
        assert_eq!(row.cache_read_tokens, 70);
        assert_eq!(row.output_tokens, 15);
        assert_eq!(row.total_tokens(), 115);
        assert!(row.total_cost > 0.0);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_malformed_empty_and_unchanged_telemetry() {
        let root = fixture_root();
        let sessions = root.join("projects/project-a/sessions");
        fs::write(sessions.join("bad.jsonl.telemetry.json"), b"not json").unwrap();
        fs::write(
            sessions.join("empty.jsonl.telemetry.json"),
            r#"{"usage":{"totalTokens":0}}"#,
        )
        .unwrap();

        assert!(load_sessions_from_root(&root, None).unwrap().is_empty());
        assert!(load_sessions_from_root(&root, Some(i64::MAX))
            .unwrap()
            .is_empty());

        fs::remove_dir_all(root).unwrap();
    }
}
