pub mod git;
pub mod projects;
pub mod store;

use crate::{
    brief::{
        self,
        extract::{extract_for_source, plain_user_texts, ExtractedSession},
        llm::{chat_completion_content, extract_json_object, truncate, LlmConfig},
        BRIEF_SOURCES,
    },
    ledger::UsageLedger,
    protocol::{
        BriefModelInfo, DailyGenerateRequest, DailyReport, DailyWorkItem, DiscoveredProject,
        ProjectBinding, Source, View,
    },
};
use chrono::{Local, NaiveDate, SecondsFormat};
use git::{commits_for_day, CommitInfo};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn local_today() -> String {
    brief::local_today()
}

/// How confidently a session was attributed to the bound project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MatchTier {
    /// The session's recorded cwd/workspace path is inside the project path.
    Exact,
    /// Claude's `-`-encoded project directory decoded to the project path.
    /// Less reliable: the encoding is ambiguous for paths containing `-`.
    Decoded,
    /// Only the directory leaf name matched. Risky across same-named dirs.
    Fallback,
}

/// Generates (or loads cached) the daily report for one bound project and
/// date: every supported CLI's sessions attributed to the project path plus
/// the project's git commits of that day are merged into a two-part
/// narrative by the LLM.
pub fn generate_daily_report(request: DailyGenerateRequest) -> Result<DailyReport, String> {
    let date = match request
        .date
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(value) => match NaiveDate::parse_from_str(value, "%Y-%m-%d") {
            Ok(_) => value.to_string(),
            Err(_) => return Err(format!("invalid date: {value}")),
        },
        None => local_today(),
    };
    let force = request.force.unwrap_or(false);
    let project_name = request.project.trim().to_string();
    if project_name.is_empty() {
        return Err("project is required".to_string());
    }
    let binding = projects::find_project(&project_name)?
        .ok_or_else(|| format!("no such project: {project_name}; add it first"))?;

    if !force {
        if let Some(cached) = store::load_report(&binding.name, &date)? {
            return Ok(cached);
        }
    }

    let model = request
        .model
        .clone()
        .unwrap_or_else(brief::default_model_request);
    let model_info = BriefModelInfo {
        base_url: model.base_url.clone(),
        model_id: model.model_id.clone(),
    };
    let api_key = model.api_key.clone().unwrap_or_default();
    if api_key.trim().is_empty() {
        return Ok(report_error(
            &date,
            &binding,
            model_info,
            "model.apiKey is required",
        ));
    }

    let mut errors = Vec::new();
    let ledger = UsageLedger::default()?;

    // Ingest every supported CLI and attribute its day's sessions to the
    // project path. Partial CLI failures degrade to the remaining sources.
    let mut matched: Vec<(String, ExtractedSession)> = Vec::new();
    let mut tiers: Vec<MatchTier> = Vec::new();
    let mut token_total: i64 = 0;
    for source in BRIEF_SOURCES {
        if let Err(err) = brief::ingest_source(&ledger, source) {
            errors.push(format!("{source}: ingest failed: {err}"));
            continue;
        }
        let rows = match ledger.load_view(source, View::Sessions) {
            Ok(rows) => rows,
            Err(err) => {
                errors.push(format!("{source}: {err}"));
                continue;
            }
        };
        let date_rows: Vec<Value> = rows
            .into_iter()
            .filter(|row| row.get("date").and_then(Value::as_str) == Some(date.as_str()))
            .collect();
        let extract = match extract_for_source(source, &date_rows) {
            Ok(extract) => extract,
            Err(err) => {
                errors.push(format!("{source}: {err}"));
                continue;
            }
        };
        for session in extract.sessions {
            if let Some(tier) = match_tier(source, &session, &binding) {
                token_total += session.token_hint;
                matched.push((extract.source.clone(), session));
                tiers.push(tier);
            }
        }
    }

    let parsed_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|err| format!("invalid date {date}: {err}"))?;
    let commits: Vec<CommitInfo> = match commits_for_day(Path::new(&binding.path), &parsed_date) {
        Ok(commits) => commits,
        Err(err) => {
            errors.push(format!("git: {err}"));
            Vec::new()
        }
    };

    if matched.is_empty() && commits.is_empty() {
        return Ok(report_error(
            &date,
            &binding,
            model_info,
            "当日无该项目下的 CLI 会话或 git 提交",
        ));
    }

    let text_sessions: Vec<&(String, ExtractedSession)> = matched
        .iter()
        .filter(|(_, session)| {
            !session.usage_only && (session.title.is_some() || !session.user_texts.is_empty())
        })
        .collect();
    let payload = json!({
        "date": date,
        "project": binding.name,
        "sessions": text_sessions.iter().map(|(source, session)| json!({
            "source": source,
            "project": session.project,
            "title": session.title,
            "userTexts": plain_user_texts(&session.user_texts),
        })).collect::<Vec<_>>(),
        "commits": commits.iter().map(|commit| json!({
            "hash": commit.hash,
            "time": commit.time,
            "author": commit.author,
            "subject": commit.subject,
        })).collect::<Vec<_>>(),
    });

    let llm = LlmConfig {
        base_url: model.base_url,
        api_key,
        model_id: model.model_id,
    };
    let (overview, work_items) = match summarize_daily(&llm, &binding.name, &date, &payload) {
        Ok(result) => result,
        Err(err) => {
            errors.push(format!("llm: {err}"));
            ("当日摘要生成失败".to_string(), Vec::new())
        }
    };

    let report = DailyReport {
        date,
        project: binding.name.clone(),
        path: binding.path.clone(),
        status: "ok".to_string(),
        overview,
        work_items,
        session_count: matched.len() as i64,
        commit_count: commits.len() as i64,
        token_total,
        coverage: tier_label(&tiers).to_string(),
        generated_at: now_iso_local(),
        model: model_info,
        error: if errors.is_empty() {
            None
        } else {
            Some(errors.join("; "))
        },
    };
    store::save_report(&report)?;
    Ok(report)
}

/// Scans every supported CLI's recorded sessions and returns project paths
/// that have not been bound yet. Paths come from the same `project_key`
/// signals the daily report matches on: each CLI's recorded cwd/workspace is
/// authoritative for codex/opencode/cursor/kimi/zcode/grok/pi, and Claude's
/// `-`-encoded directory is decoded. Non-path keys (general / unclassified /
/// api / unknown) and paths already covered by a binding's primary path or
/// aliases are dropped. Each candidate is required to still exist on disk so
/// the returned list is directly bindable.
pub fn discover_projects() -> Result<Vec<DiscoveredProject>, String> {
    let ledger = UsageLedger::default()?;
    let bindings = projects::list_projects()?;
    let bound: BTreeSet<String> = bindings
        .iter()
        .flat_map(|binding| {
            std::iter::once(projects::normalize_path(&binding.path)).chain(
                binding
                    .aliases
                    .iter()
                    .map(|alias| projects::normalize_path(alias)),
            )
        })
        .collect();

    // path (normalized) -> (sources, session_count, display_path)
    let mut found: BTreeMap<String, (BTreeSet<String>, i64, String)> = BTreeMap::new();
    for source in BRIEF_SOURCES {
        if brief::ingest_source(&ledger, source).is_err() {
            continue;
        }
        let rows = match ledger.load_view(source, View::Sessions) {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        let extract = match extract_for_source(source, &rows) {
            Ok(extract) => extract,
            Err(_) => continue,
        };
        for session in extract.sessions {
            let Some(raw_path) = candidate_path(source, &session.project_key) else {
                continue;
            };
            let norm = projects::normalize_path(&raw_path);
            if norm.is_empty() || bound.contains(&norm) {
                continue;
            }
            if !Path::new(&raw_path).is_dir() {
                continue;
            }
            let entry = found
                .entry(norm.clone())
                .or_insert_with(|| (BTreeSet::new(), 0, raw_path));
            entry.0.insert(source.to_string());
            entry.1 += 1;
        }
    }

    let mut projects: Vec<DiscoveredProject> = found
        .into_iter()
        .map(|(_, (sources, count, display_path))| DiscoveredProject {
            suggested_name: leaf_name(&display_path),
            path: display_path,
            sources: sources.into_iter().collect(),
            session_count: count,
        })
        .collect();
    projects.sort_by(|left, right| {
        right
            .session_count
            .cmp(&left.session_count)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(projects)
}

/// Extracts an absolute filesystem path from a session's `project_key` when
/// it reliably carries one. Claude encodes the directory (`-`/`--`); other
/// CLIs embed the raw cwd after the `:` prefix. Non-path sentinel values are
/// rejected.
fn candidate_path(source: Source, project_key: &str) -> Option<String> {
    let rest = project_key
        .split_once(':')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        return None;
    }
    match source {
        Source::Claude => {
            let decoded = decode_claude_path(trimmed);
            if decoded.starts_with('/') {
                Some(decoded)
            } else {
                None
            }
        }
        _ => {
            if matches!(trimmed, "general" | "unclassified" | "unknown" | "api") {
                return None;
            }
            // Aliases/cwds from codex/opencode/cursor/kimi/zcode/grok/pi are
            // recorded as absolute paths; anything else is an encoded leaf
            // name we cannot reliably reverse.
            if trimmed.starts_with('/') {
                Some(trimmed.to_string())
            } else {
                None
            }
        }
    }
}

/// Attributes a session to the project binding, best tier wins. A session's
/// project key carries its workspace path for pi/codex/opencode/cursor/zcode/
/// kimi/grok; claude only keeps a `-`-encoded directory name, so its decoded
/// form is tried before falling back to the leaf name. Aliases are explicit
/// user-asserted path equivalences, so a session under any alias matches at
/// the Exact tier.
fn match_tier(
    source: Source,
    session: &ExtractedSession,
    binding: &ProjectBinding,
) -> Option<MatchTier> {
    let primary = projects::normalize_path(&binding.path);
    if let Some(tier) = path_match_tier(source, session, &primary) {
        return Some(tier);
    }
    for alias in &binding.aliases {
        let alias = projects::normalize_path(alias);
        if path_match_tier(source, session, &alias).is_some() {
            return Some(MatchTier::Exact);
        }
    }
    if session.project == leaf_name(&primary) {
        return Some(MatchTier::Fallback);
    }
    None
}

/// Exact/Decoded attribution against a single candidate path (no leaf
/// fallback), shared by the primary path and each alias.
fn path_match_tier(source: Source, session: &ExtractedSession, path: &str) -> Option<MatchTier> {
    if let Some(candidate) = key_path(&session.project_key) {
        let candidate = projects::normalize_path(&candidate);
        if is_within(&candidate, path) {
            return Some(MatchTier::Exact);
        }
        if source == Source::Claude {
            let decoded = projects::normalize_path(&decode_claude_path(&candidate));
            if is_within(&decoded, path) {
                return Some(MatchTier::Decoded);
            }
        }
    }
    None
}

/// The path embedded in a project key (`<source>:<path>`), when present.
fn key_path(project_key: &str) -> Option<String> {
    let (_, rest) = project_key.split_once(':')?;
    let rest = rest.trim();
    if rest.is_empty() || matches!(rest, "general" | "api" | "unclassified") {
        return None;
    }
    Some(rest.to_string())
}

/// Best-effort reversal of Claude's directory encoding: `/` becomes `-`
/// and a literal `-` in the path becomes `--`. Decoding is therefore exact
/// for every path, including ones containing hyphens.
fn decode_claude_path(encoded: &str) -> String {
    encoded
        .replace("--", "\u{0}")
        .replace('-', "/")
        .replace('\u{0}', "-")
}

fn is_within(candidate: &str, project: &str) -> bool {
    candidate == project || candidate.starts_with(&format!("{project}/"))
}

fn leaf_name(path: &str) -> String {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn tier_label(tiers: &[MatchTier]) -> &'static str {
    match tiers.iter().min() {
        Some(MatchTier::Exact) => "exact",
        Some(MatchTier::Decoded) => "decoded",
        Some(MatchTier::Fallback) => "fallback",
        None => "none",
    }
}

/// LLM call: sessions (grouped per CLI) plus git commits become a two-part
/// daily work summary. Commits are woven into the narrative, not listed.
fn summarize_daily(
    config: &LlmConfig,
    project_label: &str,
    date: &str,
    payload: &Value,
) -> Result<(String, Vec<DailyWorkItem>), String> {
    let system = format!(
        "你是 Token Usage Dashboard 的日报助手。根据项目「{project_label}」在 {date} 的各 CLI 会话用户原文与该项目的 git 提交，用中文写一份该日的工作纪要。\
只依据提供的材料，禁止编造未出现的事实。输出严格 JSON：{{\"overview\":\"...\",\"workItems\":[{{\"title\":\"...\",\"detail\":\"...\"}}]}}。\
overview 为 2 到 3 句话的总括；workItems 为 2 到 8 条当天完成的事项，title 用短句（如「完成 X 功能」），detail 说明具体做了什么，涉及提交时引用其短 hash。"
    );
    let user = serde_json::to_string_pretty(payload)
        .map_err(|err| format!("failed to serialize daily payload: {err}"))?;
    let content = chat_completion_content(config, &system, &user)?;
    parse_daily_report(&content)
}

fn parse_daily_report(content: &str) -> Result<(String, Vec<DailyWorkItem>), String> {
    let trimmed = content.trim();
    let json_text = extract_json_object(trimmed).unwrap_or(trimmed);
    let parsed: Value = serde_json::from_str(json_text).map_err(|err| {
        format!(
            "failed to parse daily JSON: {err}; body={}",
            truncate(trimmed, 300)
        )
    })?;
    let overview = parsed
        .get("overview")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "LLM returned empty daily overview".to_string())?;
    let work_items = parsed
        .get("workItems")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let title = item.get("title").and_then(Value::as_str)?.trim();
                    if title.is_empty() {
                        return None;
                    }
                    Some(DailyWorkItem {
                        title: title.to_string(),
                        detail: item
                            .get("detail")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .trim()
                            .to_string(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok((overview.to_string(), work_items))
}

fn report_error(
    date: &str,
    binding: &ProjectBinding,
    model: BriefModelInfo,
    message: &str,
) -> DailyReport {
    DailyReport {
        date: date.to_string(),
        project: binding.name.clone(),
        path: binding.path.clone(),
        status: "error".into(),
        overview: String::new(),
        work_items: Vec::new(),
        session_count: 0,
        commit_count: 0,
        token_total: 0,
        coverage: "none".into(),
        generated_at: now_iso_local(),
        model,
        error: Some(message.to_string()),
    }
}

fn now_iso_local() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Secs, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ProjectBinding;
    use crate::protocol::Source::{Claude, Codex, Grok, Pi};

    fn session(source: &str, project: &str, project_key: &str) -> ExtractedSession {
        ExtractedSession {
            session_id: format!("{source}-{project}"),
            project: project.to_string(),
            project_key: project_key.to_string(),
            title: None,
            user_texts: Vec::new(),
            token_hint: 0,
            usage_only: true,
        }
    }

    fn binding(path: &str) -> ProjectBinding {
        ProjectBinding {
            name: "token-usage".into(),
            path: path.into(),
            aliases: Vec::new(),
            added_at: "2026-08-01T00:00:00+08:00".into(),
        }
    }

    #[test]
    fn matches_exact_workspace_paths() {
        let bound = binding("/Users/demo/CodeSpace/token-usage");
        assert_eq!(
            match_tier(
                Pi,
                &session("pi", "token-usage", "pi:/Users/demo/CodeSpace/token-usage"),
                &bound
            ),
            Some(MatchTier::Exact)
        );
        // cwd inside a subdirectory of the project still matches.
        assert_eq!(
            match_tier(
                Codex,
                &session(
                    "codex",
                    "rust-backend",
                    "codex:/Users/demo/CodeSpace/token-usage/rust-backend"
                ),
                &bound
            ),
            Some(MatchTier::Exact)
        );
        // A different project with the same leaf name must not match exactly.
        assert_ne!(
            match_tier(
                Pi,
                &session("pi", "token-usage", "pi:/Users/demo/Other/token-usage"),
                &bound
            ),
            Some(MatchTier::Exact)
        );
        // A session under an alias path matches at the Exact tier.
        let aliased = ProjectBinding {
            name: "token-usage".into(),
            path: "/Users/demo/CodeSpace/elsewhere".into(),
            aliases: vec!["/Users/demo/CodeSpace/token-usage".into()],
            added_at: "2026-08-01T00:00:00+08:00".into(),
        };
        assert_eq!(
            match_tier(
                Codex,
                &session(
                    "codex",
                    "rust-backend",
                    "codex:/Users/demo/CodeSpace/token-usage/rust-backend"
                ),
                &aliased
            ),
            Some(MatchTier::Exact)
        );
        // Unmatched sessions without a leaf-name hit stay unattributed.
        assert_eq!(
            match_tier(
                Pi,
                &session("pi", "summer", "pi:/Users/demo/CodeSpace/summer"),
                &bound
            ),
            None
        );
    }

    #[test]
    fn matches_claude_encoded_paths_via_decoding() {
        let bound = binding("/Users/demo/CodeSpace/token-usage");
        assert_eq!(
            match_tier(
                Claude,
                &session(
                    "claude",
                    "token-usage",
                    "claude:-Users-demo-CodeSpace-token--usage"
                ),
                &bound
            ),
            Some(MatchTier::Decoded)
        );
    }

    #[test]
    fn falls_back_to_leaf_name_when_path_unavailable() {
        let bound = binding("/Users/demo/CodeSpace/token-usage");
        // Claude paths containing '-' decode exactly via the `--` escape.
        let claude_hyphen = session("claude", "my-pi", "claude:-Users-demo-CodeSpace-my--pi");
        assert_eq!(match_tier(Claude, &claude_hyphen, &bound), None);
        let pi_bound = binding("/Users/demo/CodeSpace/my-pi");
        assert_eq!(
            match_tier(Claude, &claude_hyphen, &pi_bound),
            Some(MatchTier::Decoded)
        );
        // Pi's own `--`-wrapped encoding is not Claude's and cannot be
        // reversed; only the leaf name matches.
        assert_eq!(
            match_tier(
                Pi,
                &session("pi", "my-pi", "pi:--Users-demo-CodeSpace-my-pi--"),
                &pi_bound
            ),
            Some(MatchTier::Fallback)
        );
        // No path at all: leaf name is the last resort.
        assert_eq!(
            match_tier(
                Grok,
                &session("grok", "token-usage", "grok:general"),
                &bound
            ),
            Some(MatchTier::Fallback)
        );
    }

    #[test]
    fn tier_label_uses_best_tier() {
        assert_eq!(tier_label(&[]), "none");
        assert_eq!(
            tier_label(&[MatchTier::Fallback, MatchTier::Exact]),
            "exact"
        );
        assert_eq!(
            tier_label(&[MatchTier::Decoded, MatchTier::Fallback]),
            "decoded"
        );
        assert_eq!(tier_label(&[MatchTier::Fallback]), "fallback");
    }

    #[test]
    fn candidate_path_extracts_absolute_paths_and_decodes_claude() {
        assert_eq!(
            candidate_path(Claude, "claude:-Users-demo-CodeSpace-token--usage"),
            Some("/Users/demo/CodeSpace/token-usage".to_string())
        );
        assert_eq!(
            candidate_path(Codex, "codex:/Users/demo/CodeSpace/token-usage"),
            Some("/Users/demo/CodeSpace/token-usage".to_string())
        );
        // Sentinel / non-absolute keys are dropped.
        assert_eq!(candidate_path(Pi, "pi:general"), None);
        assert_eq!(candidate_path(Claude, "claude:general"), None);
        assert_eq!(candidate_path(Grok, "grok:relative-leaf"), None);
    }

    #[test]
    fn decodes_claude_encoded_paths() {
        assert_eq!(
            decode_claude_path("-Users-demo-CodeSpace-token--usage"),
            "/Users/demo/CodeSpace/token-usage"
        );
        assert_eq!(
            decode_claude_path("-Users-demo-CodeSpace-my--pi"),
            "/Users/demo/CodeSpace/my-pi"
        );
        assert_eq!(decode_claude_path("relative--dir"), "relative-dir");
    }

    #[test]
    fn parses_daily_report_json() {
        let content = r#"{"overview":"今天推进了日报功能。","workItems":[{"title":"实现生成流程","detail":"完成聚合与 LLM 调用 (abc1234)。"},{"title":"修复匹配","detail":"处理 claude 编码路径。"}]}"#;
        let (overview, items) = parse_daily_report(content).unwrap();
        assert_eq!(overview, "今天推进了日报功能。");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "实现生成流程");
        assert!(items[0].detail.contains("abc1234"));

        let fenced = "```json\n{\"overview\":\"概述\",\"workItems\":[]}\n```";
        let (overview, items) = parse_daily_report(fenced).unwrap();
        assert_eq!(overview, "概述");
        assert!(items.is_empty());

        assert!(parse_daily_report("{\"overview\":\"  \"}").is_err());
    }

    #[test]
    fn validates_daily_request_fields() {
        assert!(generate_daily_report(DailyGenerateRequest {
            project: "".into(),
            date: None,
            force: None,
            model: None,
        })
        .is_err());
        assert!(generate_daily_report(DailyGenerateRequest {
            project: "token-usage".into(),
            date: Some("2026-13-99".into()),
            force: None,
            model: None,
        })
        .is_err());
    }
}
