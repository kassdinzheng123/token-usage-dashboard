pub mod extract;
pub mod llm;
pub mod store;

use crate::{
    ledger::UsageLedger,
    protocol::{
        build_board_summary, BriefGenerateRequest, BriefModelInfo, Source, TodayBriefCard,
        TodayBriefHour, TodayBriefResponse, View,
    },
    sources,
};
use chrono::{Local, SecondsFormat};
use extract::{extract_for_source, ExtractedSession, SourceExtract};
use llm::{summarize_hour, summarize_project, LlmConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

pub const BRIEF_SOURCES: [Source; 5] = [
    Source::Claude,
    Source::Codex,
    Source::Cursor,
    Source::Zcode,
    Source::Kimi,
];

pub fn local_today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn load_today_brief() -> Result<Option<TodayBriefResponse>, String> {
    Ok(store::load_brief(&local_today())?.map(TodayBriefResponse::normalized))
}

pub fn generate_today_brief(request: BriefGenerateRequest) -> Result<TodayBriefResponse, String> {
    let today = local_today();
    let force = request.force.unwrap_or(false);
    let trigger = request
        .trigger
        .as_deref()
        .unwrap_or("manual")
        .trim()
        .to_string();
    let trigger = if trigger.is_empty() {
        "manual".to_string()
    } else {
        trigger
    };

    if !force {
        if let Some(existing) = store::load_brief(&today)? {
            // Briefs cached before the per-hour timeline existed regenerate once.
            if existing.status == "ok" && existing.hours.is_some() {
                return Ok(existing.normalized());
            }
        }
    }

    let enabled_sources = normalize_sources(request.sources.as_deref());
    let model = request.model.clone().unwrap_or_else(default_model_request);
    let model_info = BriefModelInfo {
        base_url: model.base_url.clone(),
        model_id: model.model_id.clone(),
    };

    if enabled_sources.is_empty() {
        let brief = TodayBriefResponse {
            date: today,
            status: "ok".into(),
            generated_at: now_iso_local(),
            trigger,
            model: model_info,
            enabled_sources: Vec::new(),
            content_fingerprint: fingerprint(&json!({"sources": []})),
            summary: "今日暂无项目摘要".into(),
            cards: Vec::new(),
            sections: Vec::new(),
            hours: Some(Vec::new()),
            error: None,
        };
        store::save_brief(&brief)?;
        return Ok(brief);
    }

    let api_key = model.api_key.clone().unwrap_or_default();
    if api_key.trim().is_empty() {
        let brief = error_brief(
            &today,
            &trigger,
            model_info,
            &enabled_sources,
            "model.apiKey is required",
        );
        store::save_brief(&brief)?;
        return Ok(brief);
    }

    let ledger = UsageLedger::default()?;
    let mut extracts = Vec::new();
    let mut extract_errors = Vec::new();
    let mut session_hours: HashMap<String, i64> = HashMap::new();

    for source in &enabled_sources {
        if let Err(err) = ingest_source(&ledger, *source) {
            extract_errors.push(format!("{source}: ingest failed: {err}"));
        }
        match ledger.load_view(*source, View::Sessions) {
            Ok(rows) => {
                let today_rows: Vec<Value> = rows
                    .into_iter()
                    .filter(|row| row.get("date").and_then(Value::as_str) == Some(today.as_str()))
                    .collect();
                for row in &today_rows {
                    if let (Some(session_id), Some(hour)) = (
                        row.get("sessionId").and_then(Value::as_str),
                        row_hour(row),
                    ) {
                        session_hours.insert(session_id.to_string(), hour);
                    }
                }
                match extract_for_source(*source, &today_rows) {
                    Ok(extract) => extracts.push(extract),
                    Err(err) => extract_errors.push(format!("{source}: {err}")),
                }
            }
            Err(err) => extract_errors.push(format!("{source}: {err}")),
        }
    }

    let fingerprint_payload = json!({
        "date": today,
        "extracts": extracts,
    });
    let content_fingerprint = fingerprint(&fingerprint_payload);

    let llm = LlmConfig {
        base_url: model.base_url.clone(),
        api_key,
        model_id: model.model_id.clone(),
    };

    let mut cards = Vec::new();
    let mut llm_errors = Vec::new();
    let mut handles = Vec::new();

    for extract in &extracts {
        for project in extract.projects() {
            if !project.has_text_content() {
                continue;
            }
            let project = project.clone();
            let llm = llm.clone();
            handles.push(std::thread::spawn(move || {
                let payload = project.to_llm_payload();
                let summary = summarize_project(
                    &llm,
                    &project.source,
                    &project.project,
                    &payload,
                );
                (project, summary)
            }));
        }
        // Brief is narrative-only. Cursor API / unmatched Cursor++ usage stays
        // on the Usage pane — do not emit usage_only brief cards.
    }

    for handle in handles {
        match handle.join() {
            Ok((project, Ok(summary))) => {
                cards.push(TodayBriefCard {
                    id: project.card_id(),
                    source: project.source.clone(),
                    project: project.project.clone(),
                    headline: summary.headline,
                    bullets: summary.bullets,
                    session_count: project.sessions.len(),
                    coverage: project.coverage().to_string(),
                });
            }
            Ok((project, Err(err))) => {
                llm_errors.push(format!("{} / {}: {err}", project.source, project.project));
                if project.source == "cursor" {
                    cards.push(TodayBriefCard {
                        id: project.card_id(),
                        source: project.source.clone(),
                        project: project.project.clone(),
                        headline: format!("{} · 用量已知，总结失败", project.project),
                        bullets: vec![
                            format!("{} 个 session", project.sessions.len()),
                            "本地 transcript 已找到，但模型总结失败。".to_string(),
                            err,
                        ],
                        session_count: project.sessions.len(),
                        coverage: project.coverage().to_string(),
                    });
                }
            }
            Err(_) => llm_errors.push("LLM worker panicked".to_string()),
        }
    }

    let hours = summarize_hours(&extracts, &session_hours, &llm, &mut llm_errors);

    cards.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.project.cmp(&right.project))
    });

    let mut errors = extract_errors;
    errors.extend(llm_errors);
    let (status, error) = if errors.is_empty() {
        ("ok".to_string(), None)
    } else if cards.is_empty() {
        ("error".to_string(), Some(errors.join("; ")))
    } else {
        ("ok".to_string(), Some(errors.join("; ")))
    };

    let summary = build_board_summary(&cards);
    let brief = TodayBriefResponse {
        date: today,
        status,
        generated_at: now_iso_local(),
        trigger,
        model: model_info,
        enabled_sources: enabled_sources.iter().map(ToString::to_string).collect(),
        content_fingerprint,
        summary,
        cards,
        sections: Vec::new(),
        hours: Some(hours),
        error,
    };
    store::save_brief(&brief)?;
    Ok(brief)
}

fn row_hour(row: &Value) -> Option<i64> {
    let time = row.get("time").and_then(Value::as_str)?;
    let (hour, _) = time.split_once(':')?;
    hour.trim().parse::<i64>().ok().filter(|hour| (0..24).contains(hour))
}

/// Groups today's sessions by their ledger hour and summarizes each hour.
/// Hours without any dialog content skip the LLM call; LLM failures keep the
/// hour with a fallback headline instead of failing the whole brief.
fn summarize_hours(
    extracts: &[SourceExtract],
    session_hours: &HashMap<String, i64>,
    llm: &LlmConfig,
    llm_errors: &mut Vec<String>,
) -> Vec<TodayBriefHour> {
    let mut groups: BTreeMap<i64, Vec<(String, ExtractedSession)>> = BTreeMap::new();
    for extract in extracts {
        for session in &extract.sessions {
            let Some(hour) = session_hours.get(&session.session_id).copied() else {
                continue;
            };
            groups
                .entry(hour)
                .or_default()
                .push((extract.source.clone(), session.clone()));
        }
    }

    let mut hours = Vec::new();
    let mut handles = Vec::new();
    for (hour, sessions) in groups {
        let session_count = sessions.len();
        let tokens: i64 = sessions.iter().map(|(_, session)| session.token_hint).sum();
        let text_sessions: Vec<&(String, ExtractedSession)> = sessions
            .iter()
            .filter(|(_, session)| {
                !session.usage_only
                    && (session.title.is_some() || !session.user_texts.is_empty())
            })
            .collect();
        if text_sessions.is_empty() {
            hours.push(TodayBriefHour {
                hour,
                headline: "仅有用量记录，无对话内容".into(),
                session_count,
                tokens,
            });
            continue;
        }
        let payload = json!({
            "hour": hour,
            "sessions": text_sessions.iter().map(|(source, session)| {
                json!({
                    "source": source,
                    "project": session.project,
                    "title": session.title,
                    "userTexts": session.user_texts,
                })
            }).collect::<Vec<_>>(),
        });
        let llm = llm.clone();
        handles.push(std::thread::spawn(move || {
            let summary = summarize_hour(&llm, hour, &payload);
            (hour, session_count, tokens, summary)
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok((hour, session_count, tokens, Ok(headline))) => hours.push(TodayBriefHour {
                hour,
                headline,
                session_count,
                tokens,
            }),
            Ok((hour, session_count, tokens, Err(err))) => {
                llm_errors.push(format!("hour {hour}: {err}"));
                hours.push(TodayBriefHour {
                    hour,
                    headline: "本小时摘要生成失败".into(),
                    session_count,
                    tokens,
                });
            }
            Err(_) => llm_errors.push("LLM hour worker panicked".to_string()),
        }
    }

    hours.sort_by_key(|hour| hour.hour);
    hours
}

fn normalize_sources(raw: Option<&[String]>) -> Vec<Source> {
    let supported: Vec<Source> = match raw {
        Some(values) if !values.is_empty() => values
            .iter()
            .filter_map(|value| Source::from_str(value).ok())
            .filter(|source| BRIEF_SOURCES.contains(source))
            .collect(),
        _ => BRIEF_SOURCES.to_vec(),
    };
    let mut unique = Vec::new();
    for source in supported {
        if !unique.contains(&source) {
            unique.push(source);
        }
    }
    unique
}

fn default_model_request() -> crate::protocol::BriefModelConfig {
    crate::protocol::BriefModelConfig {
        base_url: "http://127.0.0.1:8317/v1".into(),
        api_key: None,
        model_id: "deepseek-v4-flash".into(),
    }
}

fn ingest_source(ledger: &UsageLedger, source: Source) -> Result<(), String> {
    let source_name = source.to_string();
    // Scan incrementally using the server's ingest watermark, but never write
    // it here: brief ingests sessions only, so advancing the watermark would
    // starve the server's blocks ingest (claude/codex) of older files.
    let watermark_ms = ledger.ingest_watermark(source).unwrap_or(None);
    let sessions =
        sources::load_source_view_since(&source_name, "sessions", true, watermark_ms)
            .map_err(|err| err.to_string())?;
    ledger.ingest_live_sessions(source, &sessions)?;
    Ok(())
}

fn fingerprint(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_iso_local() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn error_brief(
    date: &str,
    trigger: &str,
    model: BriefModelInfo,
    enabled_sources: &[Source],
    message: &str,
) -> TodayBriefResponse {
    TodayBriefResponse {
        date: date.to_string(),
        status: "error".into(),
        generated_at: now_iso_local(),
        trigger: trigger.to_string(),
        model,
        enabled_sources: enabled_sources.iter().map(ToString::to_string).collect(),
        content_fingerprint: String::new(),
        summary: String::new(),
        cards: Vec::new(),
        sections: Vec::new(),
        hours: None,
        error: Some(message.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BriefModelInfo;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn force_false_returns_cached_ok_brief() {
        let _guard = store::BRIEFS_ENV_LOCK.get_or_init(|| std::sync::Mutex::new(())).lock().unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-brief-mod-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("TOKEN_USAGE_BRIEFS_DIR");
        std::env::set_var("TOKEN_USAGE_BRIEFS_DIR", &dir);

        let today = local_today();
        let cached = TodayBriefResponse {
            date: today,
            status: "ok".into(),
            generated_at: now_iso_local(),
            trigger: "auto".into(),
            model: BriefModelInfo {
                base_url: "http://127.0.0.1:8317/v1".into(),
                model_id: "deepseek-v4-flash".into(),
            },
            enabled_sources: vec!["claude".into()],
            content_fingerprint: "abc".into(),
            summary: "1 个项目：Claude·token-usage".into(),
            cards: vec![TodayBriefCard {
                id: "claude:token-usage".into(),
                source: "claude".into(),
                project: "token-usage".into(),
                headline: "缓存".into(),
                bullets: vec!["已缓存".into()],
                session_count: 1,
                coverage: "full".into(),
            }],
            sections: Vec::new(),
            hours: Some(Vec::new()),
            error: None,
        };
        store::save_brief(&cached).unwrap();

        let response = generate_today_brief(BriefGenerateRequest {
            force: Some(false),
            trigger: Some("auto".into()),
            sources: Some(vec!["claude".into()]),
            model: None,
        })
        .unwrap();

        assert_eq!(response.cards[0].headline, "缓存");
        assert!(response.summary.contains("token-usage"));
        match previous {
            Some(value) => std::env::set_var("TOKEN_USAGE_BRIEFS_DIR", value),
            None => std::env::remove_var("TOKEN_USAGE_BRIEFS_DIR"),
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn source_extract_groups_by_project() {
        let extract = SourceExtract {
            source: "claude".into(),
            sessions: vec![
                extract::ExtractedSession {
                    session_id: "a".into(),
                    project: "token-usage".into(),
                    project_key: "claude:token-usage".into(),
                    title: Some("t".into()),
                    user_texts: vec!["hello".into()],
                    token_hint: 10,
                    usage_only: false,
                },
                extract::ExtractedSession {
                    session_id: "b".into(),
                    project: "token-usage".into(),
                    project_key: "claude:token-usage".into(),
                    title: None,
                    user_texts: vec!["world".into()],
                    token_hint: 5,
                    usage_only: false,
                },
                extract::ExtractedSession {
                    session_id: "c".into(),
                    project: "other".into(),
                    project_key: "claude:other".into(),
                    title: None,
                    user_texts: vec!["x".into()],
                    token_hint: 1,
                    usage_only: false,
                },
            ],
        };
        let projects = extract.projects();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].sessions.len() + projects[1].sessions.len(), 3);
    }
}
