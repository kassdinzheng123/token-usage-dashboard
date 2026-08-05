use crate::pricing::{model_cost_usd, TokenUsage};
use crate::sources::{cluster_model_name_at, home_dir, LocalSession, SourceError};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const CHERRY_HOME_ENV: &str = "TOKEN_USAGE_CHERRY_STUDIO_HOME";
const UNKNOWN_MODEL: &str = "unknown";

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(data_dir) = cherry_studio_data_dir() else {
        return Ok(Vec::new());
    };

    let mut sessions = load_sqlite_sessions(&data_dir)?;
    if sessions.is_empty() {
        sessions = load_indexeddb_sessions(&data_dir)?;
    }

    sessions.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then_with(|| left.time.cmp(&right.time))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    Ok(sessions)
}

fn cherry_studio_data_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var(CHERRY_HOME_ENV) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Some(path);
        }
    }

    let home = home_dir()?;
    #[cfg(target_os = "macos")]
    {
        let path = home.join("Library/Application Support/CherryStudio");
        if path.is_dir() {
            return Some(path);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let path = home.join(".config/CherryStudio");
        if path.is_dir() {
            return Some(path);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(app_data) = std::env::var("APPDATA") {
            let path = PathBuf::from(app_data).join("CherryStudio");
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

fn load_sqlite_sessions(data_dir: &Path) -> Result<Vec<LocalSession>, SourceError> {
    let db_path = data_dir.join("cherrystudio.sqlite");
    if !db_path.is_file() || file_size(&db_path)? == 0 {
        return Ok(Vec::new());
    }

    let connection = Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        r#"
        SELECT id, created_at, stats, model_snapshot
        FROM message
        WHERE role = 'assistant'
          AND status = 'success'
        "#,
    )?;

    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        let (message_id, created_at, stats_json, model_snapshot_json) = row?;
        let Some(session) = sqlite_row_to_session(
            &message_id,
            created_at.as_deref(),
            stats_json.as_deref(),
            model_snapshot_json.as_deref(),
        ) else {
            continue;
        };
        if session.total_tokens() > 0 {
            sessions.push(session);
        }
    }

    Ok(sessions)
}

fn sqlite_row_to_session(
    message_id: &str,
    created_at: Option<&str>,
    stats_json: Option<&str>,
    model_snapshot_json: Option<&str>,
) -> Option<LocalSession> {
    let stats = stats_json.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
    let model_snapshot =
        model_snapshot_json.and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());

    let input_tokens = token_count(
        stats.as_ref(),
        &["promptTokens", "prompt_tokens", "inputTokens", "input_tokens"],
    );
    let output_tokens = token_count(
        stats.as_ref(),
        &[
            "completionTokens",
            "completion_tokens",
            "outputTokens",
            "output_tokens",
        ],
    );
    if input_tokens == 0 && output_tokens == 0 {
        return None;
    }

    let model_name = model_snapshot
        .as_ref()
        .and_then(|value| value.get("id").and_then(|id| id.as_str()))
        .or_else(|| {
            model_snapshot
                .as_ref()
                .and_then(|value| value.get("name").and_then(|name| name.as_str()))
        })
        .unwrap_or(UNKNOWN_MODEL)
        .to_string();

    let (date, time) = parse_created_at(created_at)?;
    let usage = TokenUsage {
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
    };
    let stored_cost = stats
        .as_ref()
        .and_then(|value| value.get("cost"))
        .and_then(|value| value.as_f64())
        .filter(|cost| cost.is_finite() && *cost > 0.0)
        .unwrap_or_else(|| model_cost_usd(&cluster_model_name_at(&model_name, Some(&date)), usage));

    Some(LocalSession {
        session_id: message_id.to_string(),
        date,
        time,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        total_tokens_override: stats
            .as_ref()
            .and_then(|value| value.get("totalTokens").and_then(|value| value.as_i64()))
            .or_else(|| {
                stats
                    .as_ref()
                    .and_then(|value| value.get("total_tokens").and_then(|value| value.as_i64()))
            }),
        total_cost: stored_cost,
        model_breakdowns: Vec::new(),
    })
}

fn load_indexeddb_sessions(data_dir: &Path) -> Result<Vec<LocalSession>, SourceError> {
    let indexeddb_dir = data_dir.join("IndexedDB/file__0.indexeddb.leveldb");
    if !indexeddb_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut data = Vec::new();
    let mut files = fs::read_dir(&indexeddb_dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "ldb" || ext == "log")
        })
        .collect::<Vec<_>>();
    files.sort();

    for path in files {
        data.extend(fs::read(&path)?);
    }
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut seen = HashSet::new();
    let marker = b"usageo\"\rprompt_tokensI";
    let completion_marker = b"\"\x11completion_tokensI";
    let total_marker = b"\"\x0ctotal_tokensI";
    let mut offset = 0usize;
    while let Some(index) = find_subslice(&data[offset..], marker) {
        let start = offset + index + marker.len();
        offset = start.saturating_add(1);
        let Some((prompt, mut next)) = read_u16(&data, start) else {
            continue;
        };
        if next + completion_marker.len() <= data.len() {
            next += completion_marker.len();
        }
        let Some((usage_completion, mut next)) = read_u16(&data, next) else {
            continue;
        };
        if next + total_marker.len() <= data.len() {
            next += total_marker.len();
        }
        let Some((_, next)) = read_u16(&data, next) else {
            continue;
        };

        let chunk = &data[next..data.len().min(next + 2_500)];
        if !matches_field(chunk, b"\x04role\"\t", b"assistant") {
            continue;
        }
        if !matches_field(chunk, b"\x06status\"\x07", b"success") {
            continue;
        }

        let Some(message_id) = extract_prefixed_string(chunk, b"\x02id\"") else {
            continue;
        };
        if !seen.insert(message_id.clone()) {
            continue;
        }

        let prompt_tokens = field_u16(chunk, b"prompt_tokens").unwrap_or(prompt);
        let output_tokens = field_u16(chunk, b"completion_tokens").unwrap_or(usage_completion);
        if prompt_tokens == 0 && output_tokens == 0 {
            continue;
        }

        let model_name = extract_model_id(chunk).unwrap_or_else(|| UNKNOWN_MODEL.to_string());
        let created_at = extract_created_at(chunk);
        let Some((date, time)) = parse_created_at(created_at.as_deref()) else {
            continue;
        };
        let usage = TokenUsage {
            input_tokens: prompt_tokens,
            output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };

        let clustered_model = cluster_model_name_at(&model_name, Some(&date));
        sessions.push(LocalSession {
            session_id: message_id,
            date,
            time,
            model_name,
            input_tokens: prompt_tokens,
            output_tokens,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            total_tokens_override: None,
            total_cost: model_cost_usd(&clustered_model, usage),
            model_breakdowns: Vec::new(),
        });
    }

    Ok(sessions)
}

fn file_size(path: &Path) -> Result<u64, SourceError> {
    Ok(fs::metadata(path)?.len())
}

fn token_count(stats: Option<&serde_json::Value>, keys: &[&str]) -> i64 {
    let Some(stats) = stats else {
        return 0;
    };
    for key in keys {
        if let Some(value) = stats.get(*key).and_then(|value| value.as_i64()) {
            return value.max(0);
        }
    }
    0
}

fn parse_created_at(created_at: Option<&str>) -> Option<(String, String)> {
    let created_at = created_at?;
    let parsed = DateTime::parse_from_rfc3339(created_at)
        .map(|value| value.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            created_at
                .parse::<DateTime<Utc>>()
                .ok()
        })?;
    Some((
        parsed.format("%Y-%m-%d").to_string(),
        parsed.format("%H:%M").to_string(),
    ))
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn read_u16(data: &[u8], offset: usize) -> Option<(i64, usize)> {
    let bytes = data.get(offset..offset + 2)?;
    Some((i64::from(u16::from_le_bytes([bytes[0], bytes[1]])), offset + 2))
}

fn field_u16(chunk: &[u8], label: &[u8]) -> Option<i64> {
    let mut marker = Vec::with_capacity(label.len() + 1);
    marker.extend_from_slice(label);
    marker.push(b'I');
    let mut best = None;
    let mut offset = 0usize;
    while let Some(index) = find_subslice(&chunk[offset..], &marker) {
        let pos = offset + index + marker.len();
        if let Some((value, _)) = read_u16(chunk, pos) {
            if value > 0 {
                best = Some(value);
            }
        }
        offset = offset + index + 1;
    }
    best
}

fn matches_field(chunk: &[u8], prefix: &[u8], expected: &[u8]) -> bool {
    let Some(index) = find_subslice(chunk, prefix) else {
        return false;
    };
    chunk
        .get(index + prefix.len()..index + prefix.len() + expected.len())
        == Some(expected)
}

fn extract_prefixed_string(chunk: &[u8], prefix: &[u8]) -> Option<String> {
    let index = find_subslice(chunk, prefix)?;
    let value = &chunk[index + prefix.len()..];
    let end = value.iter().position(|byte| *byte < b' ' || *byte == b'"')?;
    std::str::from_utf8(&value[..end])
        .ok()
        .map(str::to_string)
}

fn extract_created_at(chunk: &[u8]) -> Option<String> {
    let prefix = b"\x09createdAt\"\x18";
    let index = find_subslice(chunk, prefix)?;
    let value = &chunk[index + prefix.len()..];
    let end = value.iter().position(|byte| *byte == b'Z')?;
    std::str::from_utf8(&value[..end])
        .ok()
        .map(|timestamp| format!("{timestamp}Z"))
}

fn extract_model_id(chunk: &[u8]) -> Option<String> {
    let prefix = b"\x05modelo\"\x02id\"";
    let index = find_subslice(chunk, prefix)?;
    let value = &chunk[index + prefix.len()..];
    if value.is_empty() {
        return None;
    }

    let model = if value[0] < 0x20 {
        let length = usize::from(value[0]);
        value.get(1..1 + length)?
    } else {
        let end = value.iter().position(|byte| *byte < b' ' || *byte == b'"')?;
        value.get(..end)?
    };

    std::str::from_utf8(model).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{extract_model_id, matches_field};

    #[test]
    fn matches_status_and_role_fields() {
        let chunk = b"\x04role\"\tassistant\x06status\"\x07success";
        assert!(matches_field(chunk, b"\x04role\"\t", b"assistant"));
        assert!(matches_field(chunk, b"\x06status\"\x07", b"success"));
    }

    #[test]
    fn extract_model_id_reads_length_prefixed_value() {
        let chunk = b"\x05modelo\"\x02id\"\x07gpt-5.5\x04name\"\x07gpt-5.5";
        assert_eq!(extract_model_id(chunk).as_deref(), Some("gpt-5.5"));
    }

    #[test]
    #[ignore = "requires local Cherry Studio installation"]
    fn loads_local_cherry_sessions_when_installed() {
        let sessions = super::load_sessions().expect("load cherry sessions");
        eprintln!("cherry session count = {}", sessions.len());
        assert!(
            !sessions.is_empty(),
            "expected Cherry Studio IndexedDB sessions on this machine"
        );
    }
}
