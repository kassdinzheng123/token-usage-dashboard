pub mod extract;
pub mod llm;
pub mod store;

use crate::{
    ledger::UsageLedger,
    protocol::{
        build_board_summary, BriefDayEntry, BriefGenerateRequest, BriefModelInfo,
        BriefMonthEntry, Source, TodayBriefCard, TodayBriefHour, TodayBriefHourProject,
        TodayBriefResponse, View,
    },
    sources,
};
use chrono::{Datelike, Local, NaiveDate, SecondsFormat};
use extract::{
    extract_for_source, filter_texts_for_hour, plain_user_texts, ExtractedSession, SourceExtract,
};
use llm::{summarize_hour, summarize_project, LlmConfig};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;

pub const BRIEF_SOURCES: [Source; 8] = [
    Source::Claude,
    Source::Codex,
    Source::Opencode,
    Source::Cursor,
    Source::Zcode,
    Source::Kimi,
    Source::Pi,
    Source::Grok,
];

pub fn local_today() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn load_today_brief() -> Result<Option<TodayBriefResponse>, String> {
    load_brief_for_date(&local_today())
}

pub fn load_brief_for_date(date: &str) -> Result<Option<TodayBriefResponse>, String> {
    Ok(store::load_brief(date)?.map(TodayBriefResponse::normalized))
}

/// Hour attribution for a session: hour -> tokens spent in that hour.
type SessionHours = HashMap<(String, String), BTreeMap<i64, i64>>;

struct DayExtract {
    extracts: Vec<SourceExtract>,
    session_hours: SessionHours,
}

pub fn generate_today_brief(request: BriefGenerateRequest) -> Result<TodayBriefResponse, String> {
    let today = local_today();
    let date = match request.date.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        Some(value) => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            Ok(_) => value.to_string(),
            Err(_) => return Err(format!("invalid date: {value}")),
        },
        None => today.clone(),
    };
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

    let requested_hours: Vec<i64> = request
        .hours
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|hour| (0..24).contains(hour))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let merge_sources = request.merge_sources.unwrap_or(false)
        && request
            .sources
            .as_ref()
            .map(|sources| !sources.is_empty())
            .unwrap_or(false);

    let existing = store::load_brief(&date)?.map(TodayBriefResponse::normalized);

    if !force {
        if let Some(existing) = &existing {
            // Briefs cached before the per-hour timeline existed regenerate once.
            if existing.status == "ok" && existing.hours.is_some() {
                return Ok(existing.clone());
            }
        }
    }

    // Partial regeneration modes need a cached brief to merge into.
    let partial_hours = if requested_hours.is_empty() {
        None
    } else {
        existing.as_ref().map(|_| requested_hours.clone())
    };
    let merge_sources = merge_sources && existing.is_some();

    let enabled_sources = normalize_sources(request.sources.as_deref());
    let model = request.model.clone().unwrap_or_else(default_model_request);
    let model_info = BriefModelInfo {
        base_url: model.base_url.clone(),
        model_id: model.model_id.clone(),
    };

    if enabled_sources.is_empty() {
        let brief = TodayBriefResponse {
            date,
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
            &date,
            &trigger,
            model_info,
            &enabled_sources,
            "model.apiKey is required",
        );
        store::save_brief(&brief)?;
        return Ok(brief);
    }

    let ledger = UsageLedger::default()?;

    // Hours for the timeline come from every CLI the cached brief covers, so
    // CLI-scoped regeneration keeps the other CLIs' hour context.
    let hour_sources: Vec<Source> = if merge_sources || partial_hours.is_some() {
        let mut combined = enabled_sources.clone();
        if let Some(existing) = &existing {
            for source in &existing.enabled_sources {
                if let Ok(source) = Source::from_str(source) {
                    if BRIEF_SOURCES.contains(&source) && !combined.contains(&source) {
                        combined.push(source);
                    }
                }
            }
        }
        combined
    } else {
        enabled_sources.clone()
    };

    let mut extract_errors = Vec::new();
    let day = collect_day_extracts(&ledger, &date, &hour_sources, &mut extract_errors);

    let content_fingerprint = fingerprint(&json!({
        "date": date,
        "extracts": day.extracts,
    }));

    let llm = LlmConfig {
        base_url: model.base_url.clone(),
        api_key,
        model_id: model.model_id.clone(),
    };

    let mut llm_errors = Vec::new();

    // --- Project cards -----------------------------------------------------
    // Full: summarize every project. CLI-scoped: summarize only the selected
    // CLIs and keep the cached cards of the others.
    let card_extracts: Vec<&SourceExtract> = if merge_sources {
        day.extracts
            .iter()
            .filter(|extract| {
                enabled_sources
                    .iter()
                    .any(|source| source.to_string() == extract.source)
            })
            .collect()
    } else {
        day.extracts.iter().collect()
    };

    let mut new_cards = Vec::new();
    let mut handles = Vec::new();
    for extract in card_extracts {
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
                new_cards.push(TodayBriefCard {
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
                    new_cards.push(TodayBriefCard {
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

    let mut cards = if merge_sources {
        let mut kept: Vec<TodayBriefCard> = existing
            .as_ref()
            .map(|brief| {
                brief
                    .cards
                    .iter()
                    .filter(|card| {
                        !enabled_sources
                            .iter()
                            .any(|source| source.to_string() == card.source)
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        kept.extend(new_cards);
        kept
    } else if partial_hours.is_some() {
        existing
            .as_ref()
            .map(|brief| brief.cards.clone())
            .unwrap_or_default()
    } else {
        new_cards
    };

    // --- Hour timeline -------------------------------------------------------
    let groups = hour_groups(&day.extracts, &day.session_hours);
    let hours = match &partial_hours {
        Some(requested) => {
            let requested_set: HashSet<i64> = requested.iter().copied().collect();
            let sub_groups: BTreeMap<i64, Vec<(String, ExtractedSession, i64)>> = groups
                .into_iter()
                .filter(|(hour, _)| requested_set.contains(hour))
                .collect();
            let regenerated = summarize_hours(sub_groups, &llm, &mut llm_errors);
            let mut merged: Vec<TodayBriefHour> = existing
                .as_ref()
                .and_then(|brief| brief.hours.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|hour| !requested_set.contains(&hour.hour))
                .collect();
            merged.extend(regenerated);
            merged.sort_by_key(|hour| hour.hour);
            merged
        }
        None => summarize_hours(groups, &llm, &mut llm_errors),
    };

    cards.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.project.cmp(&right.project))
    });

    let mut errors = extract_errors;
    errors.extend(llm_errors);
    let (status, error) = if errors.is_empty() {
        ("ok".to_string(), None)
    } else if cards.is_empty() && existing.is_none() {
        ("error".to_string(), Some(errors.join("; ")))
    } else {
        ("ok".to_string(), Some(errors.join("; ")))
    };

    let summary = build_board_summary(&cards);
    let brief = TodayBriefResponse {
        date,
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

/// Ingests each source and extracts its sessions for `date`, building the
/// hour attribution map. Message-level rows spread a session's tokens across
/// the hours its messages actually happened; sessions without message rows
/// fall back to their session-level hour.
fn collect_day_extracts(
    ledger: &UsageLedger,
    date: &str,
    sources: &[Source],
    errors: &mut Vec<String>,
) -> DayExtract {
    let mut extracts = Vec::new();
    let mut session_hours: SessionHours = HashMap::new();

    for source in sources {
        if let Err(err) = ingest_source(ledger, *source) {
            errors.push(format!("{source}: ingest failed: {err}"));
        }
        match ledger.load_view(*source, View::Sessions) {
            Ok(rows) => {
                let date_rows: Vec<Value> = rows
                    .into_iter()
                    .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
                    .collect();

                let source_name = source.to_string();
                let message_hours = ledger.session_hour_tokens(*source, date).unwrap_or_default();
                for (session_id, hours) in message_hours {
                    session_hours.insert((source_name.clone(), session_id), hours);
                }
                for row in &date_rows {
                    let Some(session_id) = row.get("sessionId").and_then(Value::as_str) else {
                        continue;
                    };
                    let key = (source_name.clone(), session_id.to_string());
                    if let Some(hour) = row_hour(row) {
                        session_hours
                            .entry(key)
                            .or_insert_with(|| BTreeMap::from([(hour, 0)]));
                    }
                }

                match extract_for_source(*source, &date_rows) {
                    Ok(extract) => extracts.push(extract),
                    Err(err) => errors.push(format!("{source}: {err}")),
                }
            }
            Err(err) => errors.push(format!("{source}: {err}")),
        }
    }

    DayExtract {
        extracts,
        session_hours,
    }
}

/// Groups extracted sessions by hour. Sessions recorded only in the ledger
/// (dropped by extraction) still appear as usage-only entries so every hour
/// with usage shows up in the timeline.
///
/// Dialog text is filtered to the target hour so a long-running session does
/// not feed the same day-long `userTexts` into every hour's LLM summary.
fn hour_groups(
    extracts: &[SourceExtract],
    session_hours: &SessionHours,
) -> BTreeMap<i64, Vec<(String, ExtractedSession, i64)>> {
    let mut groups: BTreeMap<i64, Vec<(String, ExtractedSession, i64)>> = BTreeMap::new();
    let mut covered: HashSet<(String, String)> = HashSet::new();

    for extract in extracts {
        for session in &extract.sessions {
            let key = (extract.source.clone(), session.session_id.clone());
            covered.insert(key.clone());
            let Some(hours) = session_hours.get(&key) else {
                continue;
            };
            for (hour, tokens) in hours {
                groups.entry(*hour).or_default().push((
                    extract.source.clone(),
                    session_for_hour(session, *hour, hours),
                    *tokens,
                ));
            }
        }
    }

    for ((source, session_id), hours) in session_hours {
        if covered.contains(&(source.clone(), session_id.clone())) {
            continue;
        }
        for (hour, tokens) in hours {
            groups.entry(*hour).or_default().push((
                source.clone(),
                ExtractedSession {
                    session_id: session_id.clone(),
                    project: String::new(),
                    project_key: String::new(),
                    title: None,
                    user_texts: Vec::new(),
                    token_hint: *tokens,
                    usage_only: true,
                },
                *tokens,
            ));
        }
    }

    groups
}

/// Clone of `session` scoped to `hour`: only that hour's user texts, and the
/// day-level title only when this hour still has dialog (otherwise every hour
/// of a long session would share the same title-driven headline).
fn session_for_hour(
    session: &ExtractedSession,
    hour: i64,
    session_hours: &BTreeMap<i64, i64>,
) -> ExtractedSession {
    let user_texts = filter_texts_for_hour(&session.user_texts, hour, session_hours);
    let title = if user_texts.is_empty() {
        None
    } else {
        session.title.clone()
    };
    ExtractedSession {
        session_id: session.session_id.clone(),
        project: session.project.clone(),
        project_key: session.project_key.clone(),
        title,
        user_texts,
        token_hint: session.token_hint,
        usage_only: session.usage_only,
    }
}

fn row_hour(row: &Value) -> Option<i64> {
    let time = row.get("time").and_then(Value::as_str)?;
    let (hour, _) = time.split_once(':')?;
    hour.trim().parse::<i64>().ok().filter(|hour| (0..24).contains(hour))
}

/// An hour's sessions grouped into a single project bucket. Sessions across
/// multiple CLIs working on the same project name collapse into one bucket so
/// the timeline reads as "[summer] …" rather than one row per (CLI, project).
struct HourProjectBucket {
    project: String,
    /// CLI that contributed the most tokens — used as the bucket's accent
    /// color in the UI.
    source: String,
    tokens: i64,
    session_count: usize,
    /// (source, session) pairs feeding the per-project LLM summary. Only
    /// sessions with dialog content are kept; usage-only rows are dropped.
    text_sessions: Vec<(String, ExtractedSession)>,
}

/// Aggregates an hour's sessions into per-project buckets, merging CLIs that
/// share a project name. Buckets are sorted by tokens descending.
fn aggregate_hour_projects(
    sessions: &[(String, ExtractedSession, i64)],
) -> Vec<HourProjectBucket> {
    // First-seen project name wins (matches SourceExtract::projects).
    struct Acc {
        project: String,
        tokens_by_source: BTreeMap<String, i64>,
        session_count: usize,
        text_sessions: Vec<(String, ExtractedSession)>,
    }
    let mut groups: BTreeMap<String, Acc> = BTreeMap::new();
    for (source, session, tokens) in sessions {
        let project_name = if session.project.trim().is_empty() {
            "General".to_string()
        } else {
            session.project.clone()
        };
        let entry = groups.entry(project_name.clone()).or_insert_with(|| Acc {
            project: project_name,
            tokens_by_source: BTreeMap::new(),
            session_count: 0,
            text_sessions: Vec::new(),
        });
        *entry.tokens_by_source.entry(source.clone()).or_insert(0) += tokens;
        entry.session_count += 1;
        if !session.usage_only
            && (session.title.is_some() || !session.user_texts.is_empty())
        {
            entry.text_sessions.push((source.clone(), session.clone()));
        }
    }
    let mut buckets: Vec<HourProjectBucket> = groups
        .into_iter()
        .map(|(_, acc)| {
            let (source, _) = acc
                .tokens_by_source
                .iter()
                .max_by_key(|(_, tokens)| *tokens)
                .map(|(source, tokens)| (source.clone(), *tokens))
                .unwrap_or_default();
            let tokens: i64 = acc.tokens_by_source.values().sum();
            HourProjectBucket {
                project: acc.project,
                source,
                tokens,
                session_count: acc.session_count,
                text_sessions: acc.text_sessions,
            }
        })
        .collect();
    // Drop buckets with no tokens: these are usage-only placeholder sessions
    // (no extractable content) that carry no signal for the timeline.
    buckets.retain(|b| b.tokens > 0);
    buckets.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    buckets
}

/// Summarizes each hour group into a single headline. The model is prompted
/// to distinguish projects inline ("在 X 项目上，…；在 Y 项目上，…") rather than
/// us splitting per project. `projects` carries only token stats (no
/// headline) for the UI's project tags. Hours without dialog content skip the
/// LLM call and fall back to the usage-only marker.
fn summarize_hours(
    groups: BTreeMap<i64, Vec<(String, ExtractedSession, i64)>>,
    llm: &LlmConfig,
    llm_errors: &mut Vec<String>,
) -> Vec<TodayBriefHour> {
    let mut hours = Vec::new();
    let mut handles = Vec::new();
    for (hour, sessions) in groups {
        let session_count = sessions.len();
        let tokens: i64 = sessions.iter().map(|(_, _, tokens)| tokens).sum();
        let buckets = aggregate_hour_projects(&sessions);
        // Token-only breakdown for the UI; no per-project headline.
        let projects: Vec<TodayBriefHourProject> = buckets
            .iter()
            .map(|bucket| TodayBriefHourProject {
                source: bucket.source.clone(),
                project: bucket.project.clone(),
                tokens: bucket.tokens,
                session_count: bucket.session_count,
                headline: String::new(),
            })
            .collect();

        let text_buckets: Vec<&HourProjectBucket> =
            buckets.iter().filter(|b| !b.text_sessions.is_empty()).collect();
        if text_buckets.is_empty() {
            hours.push(TodayBriefHour {
                hour,
                headline: "仅有用量记录，无对话内容".into(),
                session_count,
                tokens,
                projects,
            });
            continue;
        }

        // Group text sessions by project so the model can tell them apart.
        let payload = json!({
            "hour": hour,
            "projects": text_buckets.iter().map(|bucket| {
                json!({
                    "project": bucket.project,
                    "sessions": bucket.text_sessions.iter().map(|(source, session)| {
                        json!({
                            "source": source,
                            "title": session.title,
                            "userTexts": plain_user_texts(&session.user_texts),
                        })
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        });
        let llm = llm.clone();
        handles.push(std::thread::spawn(move || {
            let summary = summarize_hour(&llm, hour, &payload);
            (hour, session_count, tokens, projects, summary)
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok((hour, session_count, tokens, projects, Ok(headline))) => hours.push(
                TodayBriefHour {
                    hour,
                    headline,
                    session_count,
                    tokens,
                    projects,
                },
            ),
            Ok((hour, session_count, tokens, projects, Err(err))) => {
                llm_errors.push(format!("hour {hour}: {err}"));
                hours.push(TodayBriefHour {
                    hour,
                    headline: "本小时摘要生成失败".into(),
                    session_count,
                    tokens,
                    projects,
                });
            }
            Err(_) => llm_errors.push("LLM hour worker panicked".to_string()),
        }
    }

    hours.sort_by_key(|hour| hour.hour);
    hours
}

/// Day-by-day entries for the brief month view: usage from the ledger plus
/// the saved brief (project count, summary, top projects) when one exists.
pub fn month_days(month: &str) -> Result<Vec<BriefDayEntry>, String> {
    let first = NaiveDate::parse_from_str(&format!("{month}-01"), "%Y-%m-%d")
        .map_err(|_| format!("invalid month: {month}"))?;
    let next = if first.month() == 12 {
        NaiveDate::from_ymd_opt(first.year() + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(first.year(), first.month() + 1, 1)
    }
    .ok_or_else(|| format!("invalid month: {month}"))?;
    let last = next.pred_opt().ok_or_else(|| format!("invalid month: {month}"))?;
    let since = first.format("%Y-%m-%d").to_string();
    let until = last.format("%Y-%m-%d").to_string();

    let ledger = UsageLedger::default()?;
    let rows = ledger.daily_usage_rollup(&since, &until)?;

    let mut entries: BTreeMap<String, BriefDayEntry> = BTreeMap::new();
    for row in rows {
        let Some(date) = row.get("date").and_then(Value::as_str) else {
            continue;
        };
        entries.insert(
            date.to_string(),
            BriefDayEntry {
                date: date.to_string(),
                total_tokens: row.get("totalTokens").and_then(Value::as_i64).unwrap_or(0),
                total_cost: row.get("totalCost").and_then(Value::as_f64).unwrap_or(0.0),
                sessions: row.get("sessions").and_then(Value::as_i64).unwrap_or(0),
                sources: string_list(row.get("sources")),
                projects: None,
                brief_summary: None,
                top_projects: Vec::new(),
                has_brief: false,
            },
        );
    }

    for date in store::list_brief_dates()? {
        if !date.starts_with(month) {
            continue;
        }
        let Ok(Some(brief)) = store::load_brief(&date) else {
            continue;
        };
        let entry = entries.entry(date.clone()).or_insert_with(|| BriefDayEntry {
            date: date.clone(),
            total_tokens: 0,
            total_cost: 0.0,
            sessions: 0,
            sources: Vec::new(),
            projects: None,
            brief_summary: None,
            top_projects: Vec::new(),
            has_brief: false,
        });
        entry.has_brief = true;
        entry.projects = Some(brief.cards.len() as i64);
        entry.brief_summary = Some(brief.summary.clone());
        entry.top_projects = top_projects(std::iter::once(&brief), 3);
        if entry.sources.is_empty() {
            entry.sources = brief.enabled_sources.clone();
        }
    }

    Ok(entries.into_values().collect())
}

/// Month-by-month entries for the brief all view, oldest first.
pub fn all_months() -> Result<Vec<BriefMonthEntry>, String> {
    let ledger = UsageLedger::default()?;
    let rows = ledger.monthly_usage_rollup()?;

    let mut entries: BTreeMap<String, BriefMonthEntry> = BTreeMap::new();
    for row in rows {
        let Some(month) = row.get("month").and_then(Value::as_str) else {
            continue;
        };
        entries.insert(
            month.to_string(),
            BriefMonthEntry {
                month: month.to_string(),
                total_tokens: row.get("totalTokens").and_then(Value::as_i64).unwrap_or(0),
                total_cost: row.get("totalCost").and_then(Value::as_f64).unwrap_or(0.0),
                sessions: row.get("sessions").and_then(Value::as_i64).unwrap_or(0),
                active_days: row.get("activeDays").and_then(Value::as_i64).unwrap_or(0),
                sources: string_list(row.get("sources")),
                projects: 0,
                brief_days: 0,
                top_projects: Vec::new(),
            },
        );
    }

    let mut briefs_by_month: BTreeMap<String, Vec<TodayBriefResponse>> = BTreeMap::new();
    for date in store::list_brief_dates()? {
        let Ok(Some(brief)) = store::load_brief(&date) else {
            continue;
        };
        briefs_by_month
            .entry(date[..7].to_string())
            .or_default()
            .push(brief);
    }

    for (month, briefs) in &briefs_by_month {
        let entry = entries
            .entry(month.clone())
            .or_insert_with(|| BriefMonthEntry {
                month: month.clone(),
                total_tokens: 0,
                total_cost: 0.0,
                sessions: 0,
                active_days: 0,
                sources: Vec::new(),
                projects: 0,
                brief_days: 0,
                top_projects: Vec::new(),
            });
        entry.brief_days = briefs.len() as i64;
        entry.projects = briefs.iter().map(|brief| brief.cards.len() as i64).sum();
        entry.top_projects = top_projects(briefs.iter(), 3);
        for brief in briefs {
            for source in &brief.enabled_sources {
                if !entry.sources.contains(source) {
                    entry.sources.push(source.clone());
                }
            }
        }
        entry.sources.sort();
    }

    Ok(entries.into_values().collect())
}

/// Top projects across the given briefs, ranked by total session count,
/// rendered as "CLI·project".
fn top_projects<'a>(briefs: impl Iterator<Item = &'a TodayBriefResponse>, limit: usize) -> Vec<String> {
    // Aggregate by project name across CLIs (first-seen display name wins),
    // so "summer" worked on via codex + cursor + opencode surfaces once as
    // "summer" rather than three "Codex·summer" / "Cursor·summer" entries.
    let mut counts: HashMap<String, (String, usize)> = HashMap::new();
    for brief in briefs {
        for card in &brief.cards {
            let entry = counts
                .entry(card.project.clone())
                .or_insert_with(|| (card.project.clone(), 0));
            entry.1 += card.session_count;
        }
    }
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|left, right| left.0.cmp(&right.0));
    ranked.sort_by(|left, right| right.1.1.cmp(&left.1.1));
    ranked
        .into_iter()
        .take(limit)
        .map(|(_, (project, _))| project)
        .collect()
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
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
        model_id: "gpt-5.6-luna".into(),
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
            hours: None,
            merge_sources: None,
            date: None,
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
    fn partial_hour_regeneration_merges_cached_hours() {
        let _guard = store::BRIEFS_ENV_LOCK.get_or_init(|| std::sync::Mutex::new(())).lock().unwrap();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-brief-hours-{stamp}"));
        let ledger_dir = std::env::temp_dir().join(format!("token-usage-ledger-hours-{stamp}"));
        let claude_dir = std::env::temp_dir().join(format!("token-usage-claude-hours-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        fs::create_dir_all(&claude_dir).unwrap();
        let previous_briefs = std::env::var_os("TOKEN_USAGE_BRIEFS_DIR");
        let previous_ledger = std::env::var_os("TOKEN_USAGE_LEDGER_PATH");
        let previous_claude = std::env::var_os("CLAUDE_CONFIG_DIR");
        std::env::set_var("TOKEN_USAGE_BRIEFS_DIR", &dir);
        std::env::set_var("TOKEN_USAGE_LEDGER_PATH", ledger_dir.join("ledger.sqlite"));
        std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);

        let today = local_today();
        let cached = TodayBriefResponse {
            date: today.clone(),
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
            hours: Some(vec![
                TodayBriefHour {
                    hour: 9,
                    headline: "旧 9 点".into(),
                    session_count: 1,
                    tokens: 100,
                    projects: Vec::new(),
                },
                TodayBriefHour {
                    hour: 14,
                    headline: "旧 14 点".into(),
                    session_count: 1,
                    tokens: 200,
                    projects: Vec::new(),
                },
            ]),
            error: None,
        };
        store::save_brief(&cached).unwrap();

        let response = generate_today_brief(BriefGenerateRequest {
            force: Some(true),
            trigger: Some("manual".into()),
            sources: Some(vec!["claude".into()]),
            model: Some(crate::protocol::BriefModelConfig {
                base_url: "http://127.0.0.1:1/v1".into(),
                api_key: Some("test-key".into()),
                model_id: "test".into(),
            }),
            hours: Some(vec![9]),
            merge_sources: None,
            date: None,
        })
        .unwrap();

        // Hour 9 has no sessions in the empty ledger, so it drops out; the
        // untouched hour keeps its cached headline, and cards stay as cached.
        let hours = response.hours.unwrap();
        assert_eq!(hours.len(), 1);
        assert_eq!(hours[0].hour, 14);
        assert_eq!(hours[0].headline, "旧 14 点");
        assert_eq!(response.cards.len(), 1);
        assert_eq!(response.cards[0].headline, "缓存");

        restore_env("TOKEN_USAGE_BRIEFS_DIR", previous_briefs);
        restore_env("TOKEN_USAGE_LEDGER_PATH", previous_ledger);
        restore_env("CLAUDE_CONFIG_DIR", previous_claude);
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(ledger_dir);
        let _ = fs::remove_dir_all(claude_dir);
    }

    fn restore_env(key: &str, previous: Option<std::ffi::OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
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

    #[test]
    fn aggregate_hour_projects_merges_clis_by_project_name() {
        let mk = |project: &str, project_key: &str, source: &str, usage_only: bool, tokens: i64| {
            (
                source.to_string(),
                ExtractedSession {
                    session_id: format!("{source}-{project}"),
                    project: project.into(),
                    project_key: project_key.into(),
                    title: None,
                    user_texts: vec![],
                    token_hint: 0,
                    usage_only,
                },
                tokens,
            )
        };
        let sessions: Vec<(String, ExtractedSession, i64)> = vec![
            // summer spans two CLIs → must merge into one bucket.
            mk("summer", "codex:summer", "codex", false, 500),
            mk("summer", "claude:summer", "claude", false, 130),
            mk("token-usage", "claude:token-usage", "claude", false, 200),
            // usage-only, empty project → "General", kept (has tokens).
            (
                "kimi".into(),
                ExtractedSession {
                    session_id: "kimi-d".into(),
                    project: String::new(),
                    project_key: String::new(),
                    title: None,
                    user_texts: vec![],
                    token_hint: 0,
                    usage_only: true,
                },
                50,
            ),
        ];
        let buckets = aggregate_hour_projects(&sessions);
        // Sorted by tokens desc: summer (630), token-usage (200), General (50).
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].project, "summer");
        assert_eq!(buckets[0].tokens, 630);
        assert_eq!(buckets[0].session_count, 2);
        // Primary source is the CLI that contributed the most tokens (codex).
        assert_eq!(buckets[0].source, "codex");
        assert_eq!(buckets[1].project, "token-usage");
        assert_eq!(buckets[1].source, "claude");
        assert_eq!(buckets[2].project, "General");
        // No sessions carried dialog content, so text_sessions stay empty.
        assert!(buckets.iter().all(|b| b.text_sessions.is_empty()));
    }

    #[test]
    fn aggregate_hour_projects_collects_text_sessions_for_llm() {
        let sessions: Vec<(String, ExtractedSession, i64)> = vec![
            (
                "codex".into(),
                ExtractedSession {
                    session_id: "a".into(),
                    project: "summer".into(),
                    project_key: "codex:summer".into(),
                    title: Some("t".into()),
                    user_texts: vec!["hi".into()],
                    token_hint: 0,
                    usage_only: false,
                },
                100,
            ),
            // usage-only sibling of the same project → merged, no text.
            (
                "codex".into(),
                ExtractedSession {
                    session_id: "b".into(),
                    project: "summer".into(),
                    project_key: "codex:summer".into(),
                    title: None,
                    user_texts: vec![],
                    token_hint: 0,
                    usage_only: true,
                },
                20,
            ),
        ];
        let buckets = aggregate_hour_projects(&sessions);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].project, "summer");
        assert_eq!(buckets[0].tokens, 120);
        assert_eq!(buckets[0].session_count, 2);
        // Only the text-bearing session is kept for the LLM payload.
        assert_eq!(buckets[0].text_sessions.len(), 1);
        assert_eq!(buckets[0].text_sessions[0].0, "codex");
    }

    #[test]
    fn hour_groups_filters_texts_per_hour() {
        let extract = SourceExtract {
            source: "claude".into(),
            sessions: vec![ExtractedSession {
                session_id: "long".into(),
                project: "summer".into(),
                project_key: "claude:summer".into(),
                title: Some("Perturb Wiki".into()),
                user_texts: vec![
                    extract::TimedUserText::at_hour("整理交接文档", 6),
                    extract::TimedUserText::at_hour("验证因果链", 11),
                ],
                token_hint: 1000,
                usage_only: false,
            }],
        };
        let mut session_hours: SessionHours = HashMap::new();
        session_hours.insert(
            ("claude".into(), "long".into()),
            BTreeMap::from([(6, 100), (11, 200)]),
        );
        let groups = hour_groups(&[extract], &session_hours);
        let six = &groups[&6];
        assert_eq!(six.len(), 1);
        assert_eq!(
            six[0]
                .1
                .user_texts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            vec!["整理交接文档"]
        );
        assert_eq!(six[0].1.title.as_deref(), Some("Perturb Wiki"));

        let eleven = &groups[&11];
        assert_eq!(
            eleven[0]
                .1
                .user_texts
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>(),
            vec!["验证因果链"]
        );

        // Hour with tokens but no matching texts → title cleared so LLM
        // does not reuse the day title as a fake per-hour narrative.
        let mut session_hours2: SessionHours = HashMap::new();
        session_hours2.insert(
            ("claude".into(), "long".into()),
            BTreeMap::from([(6, 100), (7, 50), (11, 200)]),
        );
        let extract2 = SourceExtract {
            source: "claude".into(),
            sessions: vec![ExtractedSession {
                session_id: "long".into(),
                project: "summer".into(),
                project_key: "claude:summer".into(),
                title: Some("Perturb Wiki".into()),
                user_texts: vec![
                    extract::TimedUserText::at_hour("整理交接文档", 6),
                    extract::TimedUserText::at_hour("验证因果链", 11),
                ],
                token_hint: 1000,
                usage_only: false,
            }],
        };
        let groups2 = hour_groups(&[extract2], &session_hours2);
        assert!(groups2[&7][0].1.user_texts.is_empty());
        assert!(groups2[&7][0].1.title.is_none());
    }
}
