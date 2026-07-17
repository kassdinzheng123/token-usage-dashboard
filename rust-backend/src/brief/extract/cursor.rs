//! Cursor Daily Brief content comes from today's active local composers
//! (`state.vscdb` bubbles + optional agent-transcripts).
//!
//! Usage rows still come from Cursor++ / Cursor API:
//! - Cursor++ requestIds may attach token hints onto matching composers
//! - unmatched Cursor++ + Cursor API remain usage-only residual sessions
//!
//! Join key for token attach: `cursor.requestTraces.log` (`requestId` +
//! `composerId`). Bubble `requestId` is a different ID space.

use super::{
    display_project_name, push_capped_text, session_id_of, session_token_hint, ExtractedSession,
    SourceExtract, MAX_USER_MESSAGES,
};
use crate::sources::home_dir;
use chrono::{Duration, Local, TimeZone};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const CURSOR_STATE_DB_ENV: &str = "TOKEN_USAGE_CURSOR_STATE_DB";
const CURSOR_PROJECTS_ENV: &str = "TOKEN_USAGE_CURSOR_PROJECTS_DIR";
const CURSOR_LOGS_ENV: &str = "TOKEN_USAGE_CURSOR_LOGS_DIR";

pub fn extract(session_rows: &[Value]) -> Result<SourceExtract, String> {
    let mut needed_request_ids = HashSet::new();
    let mut usage_pending = Vec::new();

    for row in session_rows {
        let Some(session_id) = session_id_of(row) else {
            continue;
        };
        let token_hint = session_token_hint(row);
        let ids = parse_session_ids(&session_id);
        if let Some(request_id) = ids.request_id.clone() {
            needed_request_ids.insert(request_id);
        }
        usage_pending.push((session_id, token_hint, ids));
    }

    let request_to_composer = load_request_to_composer_map(&needed_request_ids);

    let mut composer_ids: HashSet<String> = list_today_active_composer_ids()
        .unwrap_or_default()
        .into_iter()
        .collect();
    for (_, _, ids) in &usage_pending {
        if let Some(composer_id) = ids.composer_id.as_ref() {
            composer_ids.insert(composer_id.clone());
        }
        if let Some(request_id) = ids.request_id.as_ref() {
            if let Some(composer_id) = request_to_composer.get(request_id) {
                composer_ids.insert(composer_id.clone());
            }
        }
    }

    let index = CursorContentIndex::load_for_composers(&composer_ids)
        .unwrap_or_else(|_| CursorContentIndex::default());

    let mut token_by_composer: HashMap<String, i64> = HashMap::new();
    let mut sessions = Vec::new();

    // Content-first: one session per active composer that has readable text.
    let mut content_composer_ids = HashSet::new();
    let mut sorted_ids: Vec<String> = composer_ids.into_iter().collect();
    sorted_ids.sort();
    for composer_id in sorted_ids {
        let mut content = index.get(&composer_id);
        if content.user_texts.is_empty() {
            content.user_texts =
                load_transcript_user_texts(content.project_path.as_deref(), &composer_id);
        }
        if content.user_texts.is_empty() && content.title.is_none() {
            continue;
        }
        if content.user_texts.is_empty() {
            // Title-only drafts are not useful for summarization.
            continue;
        }

        let (project, project_key) = match content.project_path.as_deref() {
            Some(path) if !path.trim().is_empty() => {
                let name = display_project_name(path);
                (name, format!("cursor:{path}"))
            }
            _ => ("未分类".to_string(), "cursor:unclassified".to_string()),
        };

        content_composer_ids.insert(composer_id.clone());
        sessions.push(ExtractedSession {
            session_id: format!("cursor:composer:{composer_id}"),
            project,
            project_key,
            title: content.title,
            user_texts: content.user_texts,
            token_hint: 0,
            usage_only: false,
        });
    }

    // Attach Cursor++ tokens onto composers when joinable; otherwise keep as
    // usage-only residual (Cursor API always residual).
    for (session_id, token_hint, ids) in usage_pending {
        if ids.is_api {
            sessions.push(ExtractedSession {
                session_id,
                project: "Cursor".into(),
                project_key: "cursor:api".into(),
                title: None,
                user_texts: Vec::new(),
                token_hint,
                usage_only: true,
            });
            continue;
        }

        let composer_id = ids.composer_id.clone().or_else(|| {
            ids.request_id
                .as_ref()
                .and_then(|request_id| request_to_composer.get(request_id).cloned())
        });

        if let Some(composer_id) = composer_id {
            if content_composer_ids.contains(&composer_id) {
                *token_by_composer.entry(composer_id).or_insert(0) += token_hint;
                continue;
            }
        }

        sessions.push(ExtractedSession {
            session_id,
            project: "未分类".into(),
            project_key: "cursor:unclassified".into(),
            title: None,
            user_texts: Vec::new(),
            token_hint,
            usage_only: true,
        });
    }

    for session in &mut sessions {
        if let Some(composer_id) = session.session_id.strip_prefix("cursor:composer:") {
            if let Some(tokens) = token_by_composer.get(composer_id) {
                session.token_hint = *tokens;
            }
        }
    }

    Ok(SourceExtract {
        source: "cursor".into(),
        sessions,
    })
}

#[derive(Debug, Default, Clone)]
struct SessionIds {
    request_id: Option<String>,
    composer_id: Option<String>,
    is_api: bool,
}

fn parse_session_ids(session_id: &str) -> SessionIds {
    if session_id.starts_with("cursor:api:") || session_id.starts_with("cursor:api") {
        return SessionIds {
            is_api: true,
            ..SessionIds::default()
        };
    }

    let rest = session_id.strip_prefix("cursorpp:").unwrap_or(session_id);
    // cursorpp:{requestId}
    // cursorpp:{conversationId}:{YYYYMMDDHHMMSS}
    if let Some((first, second)) = rest.split_once(':') {
        if second.chars().all(|c| c.is_ascii_digit()) && second.len() >= 8 {
            return SessionIds {
                composer_id: Some(first.to_string()),
                request_id: None,
                is_api: false,
            };
        }
    }

    if rest.contains(':') {
        // sanitized path-like synthetic id — no join key
        return SessionIds::default();
    }

    SessionIds {
        request_id: Some(rest.to_string()),
        composer_id: None,
        is_api: false,
    }
}

#[derive(Debug, Default, Clone)]
struct ResolvedContent {
    project_path: Option<String>,
    title: Option<String>,
    user_texts: Vec<String>,
}

#[derive(Debug, Default)]
struct CursorContentIndex {
    by_composer_id: HashMap<String, ResolvedContent>,
}

impl CursorContentIndex {
    fn load_for_composers(composer_ids: &HashSet<String>) -> Result<Self, String> {
        if composer_ids.is_empty() {
            return Ok(Self::default());
        }

        let Some(db_path) = discover_state_db() else {
            return Ok(Self::default());
        };
        if !db_path.is_file() {
            return Ok(Self::default());
        }

        let connection = open_state_db(&db_path)?;
        let headers = load_composer_headers(&connection)?;
        let mut by_composer: HashMap<String, ResolvedContent> = HashMap::new();

        for composer_id in composer_ids {
            let header = headers.get(composer_id);
            by_composer.insert(
                composer_id.clone(),
                ResolvedContent {
                    project_path: header.and_then(|h| h.project_path.clone()),
                    title: header.and_then(|h| h.title.clone()),
                    user_texts: Vec::new(),
                },
            );

            let mut statement = connection
                .prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE ?1")
                .map_err(|err| err.to_string())?;
            let pattern = format!("bubbleId:{composer_id}:%");
            let rows = statement
                .query_map([&pattern], |row| {
                    let key: String = row.get(0)?;
                    let value = match row.get_ref(1)? {
                        rusqlite::types::ValueRef::Text(text) => {
                            String::from_utf8_lossy(text).into_owned()
                        }
                        rusqlite::types::ValueRef::Blob(blob) => {
                            String::from_utf8_lossy(blob).into_owned()
                        }
                        _ => String::new(),
                    };
                    Ok((key, value))
                })
                .map_err(|err| err.to_string())?;

            for row in rows {
                let (_key, text) = row.map_err(|err| err.to_string())?;
                if text.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                // type 1 = user bubble
                if value.get("type").and_then(Value::as_i64) != Some(1) {
                    continue;
                }
                let Some(user_text) = value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(ToString::to_string)
                else {
                    continue;
                };
                if let Some(entry) = by_composer.get_mut(composer_id) {
                    push_capped_text(&mut entry.user_texts, &user_text);
                }
            }
        }

        Ok(Self {
            by_composer_id: by_composer,
        })
    }

    fn get(&self, composer_id: &str) -> ResolvedContent {
        self.by_composer_id
            .get(composer_id)
            .cloned()
            .unwrap_or_default()
    }
}

fn list_today_active_composer_ids() -> Result<Vec<String>, String> {
    let Some(db_path) = discover_state_db() else {
        return Ok(Vec::new());
    };
    if !db_path.is_file() {
        return Ok(Vec::new());
    }
    let connection = open_state_db(&db_path)?;
    let (start_ms, end_ms) = local_day_ms_range();
    let mut statement = connection
        .prepare(
            "SELECT composerId, value FROM composerHeaders
             WHERE (lastUpdatedAt >= ?1 AND lastUpdatedAt < ?2)
                OR (createdAt >= ?1 AND createdAt < ?2)",
        )
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map(rusqlite::params![start_ms, end_ms], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|err| err.to_string())?;

    let mut ids = Vec::new();
    for row in rows {
        let (composer_id, value_text) = row.map_err(|err| err.to_string())?;
        if should_skip_composer(&composer_id, &value_text) {
            continue;
        }
        ids.push(composer_id);
    }
    Ok(ids)
}

fn should_skip_composer(composer_id: &str, value_text: &str) -> bool {
    if composer_id == "empty-state-draft" || composer_id.starts_with("empty-state") {
        return true;
    }
    let Ok(value) = serde_json::from_str::<Value>(value_text) else {
        return false;
    };
    if value.get("isArchived").and_then(Value::as_bool) == Some(true) {
        return true;
    }
    if value.get("isDraft").and_then(Value::as_bool) == Some(true) {
        let has_name = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .is_some();
        if !has_name {
            return true;
        }
    }
    false
}

fn local_day_ms_range() -> (i64, i64) {
    let now = Local::now();
    let start_naive = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid");
    let start = Local
        .from_local_datetime(&start_naive)
        .single()
        .unwrap_or(now);
    let end = start + Duration::days(1);
    (start.timestamp_millis(), end.timestamp_millis())
}

fn open_state_db(db_path: &Path) -> Result<Connection, String> {
    match Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(connection) => Ok(connection),
        Err(err) => {
            // Live Cursor may lock the DB; read a temp copy instead.
            let temp_dir = std::env::temp_dir().join(format!(
                "token-usage-cursor-state-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&temp_dir);
            fs::create_dir_all(&temp_dir).map_err(|copy_err| {
                format!(
                    "failed to open {}: {err}; temp dir: {copy_err}",
                    db_path.display()
                )
            })?;
            let temp_db = temp_dir.join("state.vscdb");
            fs::copy(db_path, &temp_db).map_err(|copy_err| {
                format!(
                    "failed to open {}: {err}; copy failed: {copy_err}",
                    db_path.display()
                )
            })?;
            Connection::open_with_flags(&temp_db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(
                |open_err| {
                    format!(
                        "failed to open {} (and temp copy): {err}; {open_err}",
                        db_path.display()
                    )
                },
            )
        }
    }
}

#[derive(Debug, Default)]
struct ComposerHeader {
    title: Option<String>,
    project_path: Option<String>,
}

fn load_composer_headers(
    connection: &Connection,
) -> Result<HashMap<String, ComposerHeader>, String> {
    let mut statement = connection
        .prepare("SELECT composerId, workspaceId, value FROM composerHeaders")
        .map_err(|err| err.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|err| err.to_string())?;

    let mut headers = HashMap::new();
    for row in rows {
        let (composer_id, workspace_id, value_text) = row.map_err(|err| err.to_string())?;
        let Ok(value) = serde_json::from_str::<Value>(&value_text) else {
            continue;
        };
        let title = value
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToString::to_string);
        let project_path = project_path_from_header(&value)
            .or_else(|| workspace_id.as_deref().and_then(project_path_from_workspace_id));
        headers.insert(
            composer_id,
            ComposerHeader {
                title,
                project_path,
            },
        );
    }
    Ok(headers)
}

fn project_path_from_header(value: &Value) -> Option<String> {
    if let Some(path) = value
        .pointer("/workspaceIdentifier/uri/fsPath")
        .or_else(|| value.pointer("/workspaceIdentifier/uri/path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return Some(path.to_string());
    }
    value
        .get("trackedGitRepos")
        .and_then(Value::as_array)
        .and_then(|repos| repos.first())
        .and_then(|repo| repo.get("repoPath"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(ToString::to_string)
}

fn project_path_from_workspace_id(workspace_id: &str) -> Option<String> {
    if workspace_id.is_empty() || workspace_id == "empty-window" {
        return None;
    }
    let path = home_dir()?
        .join("Library")
        .join("Application Support")
        .join("Cursor")
        .join("User")
        .join("workspaceStorage")
        .join(workspace_id)
        .join("workspace.json");
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if let Some(folder) = value.get("folder").and_then(Value::as_str) {
        return Some(file_uri_to_path(folder));
    }
    if let Some(workspace) = value.get("workspace").and_then(Value::as_str) {
        return Some(file_uri_to_path(workspace));
    }
    None
}

fn file_uri_to_path(uri: &str) -> String {
    let stripped = uri
        .strip_prefix("file://")
        .unwrap_or(uri)
        .replace("%20", " ");
    stripped
}

fn discover_state_db() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(CURSOR_STATE_DB_ENV) {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb")
    })
}

fn discover_projects_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(CURSOR_PROJECTS_ENV) {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| home.join(".cursor").join("projects"))
}

fn discover_logs_dir() -> Option<PathBuf> {
    if let Some(raw) = std::env::var_os(CURSOR_LOGS_ENV) {
        return Some(PathBuf::from(raw));
    }
    if let Some(raw) = std::env::var_os("CURSORPP_LOG_ROOT") {
        return Some(PathBuf::from(raw));
    }
    home_dir().map(|home| {
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
            .join("logs")
    })
}

/// Map AgentService/Cursor++ requestId → composerId.
fn load_request_to_composer_map(needed: &HashSet<String>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if needed.is_empty() {
        return map;
    }
    let Some(logs_dir) = discover_logs_dir() else {
        return map;
    };
    if !logs_dir.is_dir() {
        return map;
    }

    let mut files = Vec::new();
    collect_log_files(&logs_dir, &mut files);
    files.sort_by_key(|path| {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "cursor.requestTraces.log" {
            0
        } else if name.starts_with("Cursor++") {
            1
        } else {
            2
        }
    });

    for path in files {
        if map.len() >= needed.len() && needed.iter().all(|id| map.contains_key(id)) {
            break;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "cursor.requestTraces.log" {
            ingest_request_traces(&path, needed, &mut map);
        } else if name.starts_with("Cursor++") && name.ends_with(".log") {
            ingest_cursorpp_conversation_fallback(&path, needed, &mut map);
        }
    }
    map
}

fn collect_log_files(root: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_log_files(&path, files);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name == "cursor.requestTraces.log"
            || (name.starts_with("Cursor++") && name.ends_with(".log"))
        {
            files.push(path);
        }
    }
}

fn ingest_request_traces(path: &Path, needed: &HashSet<String>, map: &mut HashMap<String, String>) {
    let Ok(file) = File::open(path) else {
        return;
    };
    for line in BufReader::new(file).lines().flatten() {
        if !line.contains("requestId=") || !line.contains("composerId=") {
            continue;
        }
        let Some(request_id) = capture_after(&line, "requestId=") else {
            continue;
        };
        if !needed.contains(request_id) || map.contains_key(request_id) {
            continue;
        }
        if let Some(composer_id) = capture_after(&line, "composerId=") {
            map.insert(request_id.to_string(), composer_id.to_string());
        }
    }
}

fn ingest_cursorpp_conversation_fallback(
    path: &Path,
    needed: &HashSet<String>,
    map: &mut HashMap<String, String>,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut current_request_id: Option<String> = None;
    for line in BufReader::new(file).lines().flatten() {
        if line.contains("AgentService/RunSSE started") {
            current_request_id = json_string_field_after(&line, "requestId");
            continue;
        }
        let Some(request_id) = current_request_id.as_deref() else {
            continue;
        };
        if !needed.contains(request_id) || map.contains_key(request_id) {
            continue;
        }
        if let Some(conversation_id) = json_string_field_after(&line, "conversationId") {
            map.insert(request_id.to_string(), conversation_id);
        }
    }
}

fn capture_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(rest.len());
    let value = &rest[..end];
    if value.len() >= 4 {
        Some(value)
    } else {
        None
    }
}

fn json_string_field_after(line: &str, field: &str) -> Option<String> {
    let marker = format!("\"{field}\":");
    let start = line.find(&marker)? + marker.len();
    let rest = line[start..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let value = &rest[..end];
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn load_transcript_user_texts(project_path: Option<&str>, composer_id: &str) -> Vec<String> {
    let Some(projects_dir) = discover_projects_dir() else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    if let Some(project_path) = project_path {
        candidates.push(
            projects_dir
                .join(encode_project_slug(project_path))
                .join("agent-transcripts")
                .join(composer_id)
                .join(format!("{composer_id}.jsonl")),
        );
    }
    if let Ok(entries) = fs::read_dir(&projects_dir) {
        for entry in entries.flatten() {
            let path = entry
                .path()
                .join("agent-transcripts")
                .join(composer_id)
                .join(format!("{composer_id}.jsonl"));
            if path.is_file() {
                candidates.push(path);
            }
        }
    }

    for path in candidates {
        if let Ok(texts) = read_transcript_jsonl(&path) {
            if !texts.is_empty() {
                return texts;
            }
        }
    }
    Vec::new()
}

fn encode_project_slug(project_path: &str) -> String {
    let trimmed = project_path.trim().trim_end_matches('/');
    let without_leading = trimmed.strip_prefix('/').unwrap_or(trimmed);
    without_leading.replace('/', "-")
}

fn read_transcript_jsonl(path: &Path) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let mut user_texts = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("role").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(extracted) = extract_transcript_user_text(&value) {
            push_capped_text(&mut user_texts, &extracted);
            if user_texts.len() >= MAX_USER_MESSAGES {
                break;
            }
        }
    }
    Ok(user_texts)
}

fn extract_transcript_user_text(value: &Value) -> Option<String> {
    let content = value.pointer("/message/content")?;
    match content {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(strip_user_query_wrapper(trimmed))
            }
        }
        Value::Array(parts) => {
            let mut chunks = Vec::new();
            for part in parts {
                if part.get("type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                if let Some(text) = part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    chunks.push(strip_user_query_wrapper(text));
                }
            }
            if chunks.is_empty() {
                None
            } else {
                Some(chunks.join("\n"))
            }
        }
        _ => None,
    }
}

fn strip_user_query_wrapper(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(inner) = trimmed
        .strip_prefix("<user_query>")
        .and_then(|rest| rest.strip_suffix("</user_query>"))
    {
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn setup_db(root: &Path) -> (PathBuf, i64) {
        let db_path = root.join("state.vscdb");
        let connection = Connection::open(&db_path).unwrap();
        connection
            .execute_batch(
                r#"
                CREATE TABLE composerHeaders (
                    composerId TEXT PRIMARY KEY,
                    workspaceId TEXT,
                    createdAt INTEGER,
                    lastUpdatedAt INTEGER,
                    isArchived INTEGER,
                    isSubagent INTEGER,
                    recency INTEGER,
                    checkpointAt INTEGER,
                    value TEXT
                );
                CREATE TABLE cursorDiskKV (
                    key TEXT,
                    value BLOB
                );
                "#,
            )
            .unwrap();
        let now_ms = Local::now().timestamp_millis();
        (db_path, now_ms)
    }

    #[test]
    fn parses_cursorpp_session_ids() {
        let request = parse_session_ids("cursorpp:req-123");
        assert_eq!(request.request_id.as_deref(), Some("req-123"));
        assert!(request.composer_id.is_none());

        let composer = parse_session_ids("cursorpp:conv-abc:20260715120000");
        assert_eq!(composer.composer_id.as_deref(), Some("conv-abc"));
        assert!(composer.request_id.is_none());

        assert!(parse_session_ids("cursor:api:event:1").is_api);
    }

    #[test]
    fn scans_today_active_composers_without_usage_rows() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-cursor-scan-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let (db_path, now_ms) = setup_db(&root);
        let connection = Connection::open(&db_path).unwrap();

        let header = json!({
            "composerId": "comp-scan",
            "name": "Local scan work",
            "workspaceIdentifier": {
                "uri": { "fsPath": "/Users/demo/CodeSpace/token-usage" }
            }
        });
        connection
            .execute(
                "INSERT INTO composerHeaders (composerId, workspaceId, createdAt, lastUpdatedAt, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "comp-scan",
                    "ws-1",
                    now_ms,
                    now_ms,
                    serde_json::to_string(&header).unwrap()
                ],
            )
            .unwrap();
        let bubble = json!({
            "type": 1,
            "text": "按今日活跃 composer 扫本地"
        });
        connection
            .execute(
                "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                rusqlite::params![
                    "bubbleId:comp-scan:bubble-1",
                    serde_json::to_string(&bubble).unwrap().into_bytes()
                ],
            )
            .unwrap();

        let previous_db = std::env::var_os(CURSOR_STATE_DB_ENV);
        std::env::set_var(CURSOR_STATE_DB_ENV, &db_path);

        let extract = extract(&[]).unwrap();

        match previous_db {
            Some(value) => std::env::set_var(CURSOR_STATE_DB_ENV, value),
            None => std::env::remove_var(CURSOR_STATE_DB_ENV),
        }
        let _ = fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 1);
        let session = &extract.sessions[0];
        assert_eq!(session.session_id, "cursor:composer:comp-scan");
        assert!(!session.usage_only);
        assert_eq!(session.project, "token-usage");
        assert_eq!(session.user_texts, vec!["按今日活跃 composer 扫本地"]);
    }

    #[test]
    fn attaches_cursorpp_tokens_onto_scanned_composer() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "token-usage-cursor-attach-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let (db_path, now_ms) = setup_db(&root);
        let logs_dir = root.join("logs").join("window1");
        fs::create_dir_all(&logs_dir).unwrap();
        let connection = Connection::open(&db_path).unwrap();

        let header = json!({
            "composerId": "comp-1",
            "name": "Daily Brief work",
            "workspaceIdentifier": {
                "uri": { "fsPath": "/Users/demo/CodeSpace/token-usage" }
            }
        });
        connection
            .execute(
                "INSERT INTO composerHeaders (composerId, workspaceId, createdAt, lastUpdatedAt, value)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    "comp-1",
                    "ws-1",
                    now_ms,
                    now_ms,
                    serde_json::to_string(&header).unwrap()
                ],
            )
            .unwrap();
        let bubble = json!({
            "type": 1,
            "text": "实现 Cursor 本地 transcript 抽取",
            "requestId": "bubble-local-id"
        });
        connection
            .execute(
                "INSERT INTO cursorDiskKV (key, value) VALUES (?1, ?2)",
                rusqlite::params![
                    "bubbleId:comp-1:bubble-1",
                    serde_json::to_string(&bubble).unwrap().into_bytes()
                ],
            )
            .unwrap();

        fs::write(
            logs_dir.join("cursor.requestTraces.log"),
            "2026-07-15T10:00:00.000Z span_completed name=\"buildComposerRequestContext\" requestId=0d5c7637-5765-4ac0-add8-a79db929e338 composerId=comp-1 durationMs=1\n",
        )
        .unwrap();

        let previous_db = std::env::var_os(CURSOR_STATE_DB_ENV);
        let previous_logs = std::env::var_os(CURSOR_LOGS_ENV);
        std::env::set_var(CURSOR_STATE_DB_ENV, &db_path);
        std::env::set_var(CURSOR_LOGS_ENV, root.join("logs"));

        let extract = extract(&[
            json!({
                "sessionId": "cursorpp:0d5c7637-5765-4ac0-add8-a79db929e338",
                "totalTokens": 42
            }),
            json!({
                "sessionId": "cursor:api:event:1",
                "totalTokens": 100
            }),
        ])
        .unwrap();

        match previous_db {
            Some(value) => std::env::set_var(CURSOR_STATE_DB_ENV, value),
            None => std::env::remove_var(CURSOR_STATE_DB_ENV),
        }
        match previous_logs {
            Some(value) => std::env::set_var(CURSOR_LOGS_ENV, value),
            None => std::env::remove_var(CURSOR_LOGS_ENV),
        }
        let _ = fs::remove_dir_all(&root);

        assert_eq!(extract.sessions.len(), 2);
        let content = extract
            .sessions
            .iter()
            .find(|session| session.session_id.starts_with("cursor:composer:"))
            .unwrap();
        assert!(!content.usage_only);
        assert_eq!(content.token_hint, 42);
        assert_eq!(content.project, "token-usage");

        let api = extract
            .sessions
            .iter()
            .find(|session| session.session_id.starts_with("cursor:api:"))
            .unwrap();
        assert!(api.usage_only);
        assert_eq!(api.token_hint, 100);
    }
}
