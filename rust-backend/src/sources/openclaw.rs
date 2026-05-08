use crate::sources::{home_dir, num, to_i64, unix_millis_to_utc_parts, LocalSession, SourceError};
use serde_json::Value;
use std::fs;

pub fn load_sessions() -> Result<Vec<LocalSession>, SourceError> {
    let Some(agents_dir) = home_dir().map(|home| home.join(".openclaw").join("agents")) else {
        return Ok(Vec::new());
    };

    let Ok(agent_entries) = fs::read_dir(agents_dir) else {
        return Ok(Vec::new());
    };

    let mut sessions = Vec::new();
    for agent_entry in agent_entries {
        let Ok(agent_entry) = agent_entry else {
            continue;
        };
        let sessions_path = agent_entry.path().join("sessions").join("sessions.json");
        let Ok(contents) = fs::read_to_string(sessions_path) else {
            continue;
        };
        let Ok(data) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        append_sessions_from_value(&mut sessions, &data);
    }

    Ok(sessions)
}

fn append_sessions_from_value(sessions: &mut Vec<LocalSession>, data: &Value) {
    let Some(entries) = data.as_object() else {
        return;
    };

    for (key, raw) in entries {
        let Some(entry) = raw.as_object() else {
            continue;
        };

        let timestamp = entry.get("updatedAt").map(to_i64).unwrap_or_default();
        let Some((date, time)) = unix_millis_to_utc_parts(timestamp) else {
            continue;
        };

        let session_id = string_field(entry.get("sessionId")).unwrap_or_else(|| key.clone());
        let model_name = string_field(entry.get("model")).unwrap_or_else(|| "unknown".to_string());
        let input_tokens = entry.get("inputTokens").map(to_i64).unwrap_or_default();
        let output_tokens = entry.get("outputTokens").map(to_i64).unwrap_or_default();
        let cache_creation_tokens = entry.get("cacheWrite").map(to_i64).unwrap_or_default();
        let cache_read_tokens = entry.get("cacheRead").map(to_i64).unwrap_or_default();
        let total_cost = entry
            .get("costUSD")
            .map(num)
            .filter(|cost| *cost > 0.0)
            .or_else(|| entry.get("totalCost").map(num))
            .unwrap_or_default();

        let session = LocalSession {
            session_id,
            date,
            time,
            model_name,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            total_cost,
        };

        if session.total_tokens() > 0 {
            sessions.push(session);
        }
    }
}

fn string_field(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}
