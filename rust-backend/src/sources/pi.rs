use crate::{
    pricing::{model_cost_usd, TokenUsage},
    sources::{home_dir, num, to_i64},
};
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    convert::TryFrom,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

const PI_AGENT_DIR_ENV: &str = "PI_AGENT_DIR";
const OH_MY_PI_AGENT_DIR_ENV: &str = "PI_CODING_AGENT_DIR";
const OH_MY_PI_CONFIG_DIR_ENV: &str = "PI_CONFIG_DIR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceView {
    Daily,
    Monthly,
    Sessions,
    Blocks,
    Messages,
}

impl TryFrom<&str> for SourceView {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "daily" => Ok(Self::Daily),
            "monthly" => Ok(Self::Monthly),
            "sessions" => Ok(Self::Sessions),
            "blocks" => Ok(Self::Blocks),
            "messages" => Ok(Self::Messages),
            other => Err(format!("unsupported view: {other}")),
        }
    }
}

#[derive(Debug, Clone)]
struct UsageEntry {
    timestamp: DateTime<Local>,
    session_id: String,
    project_path: String,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
    /// Stable content hash assigned while scanning; used as the message id.
    message_key: String,
}

impl UsageEntry {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Clone)]
struct WorkflowRunUsage {
    run_id: String,
    session_id: String,
    timestamp: DateTime<Local>,
    model_name: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl WorkflowRunUsage {
    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }

    fn to_usage_entry(&self, project_path: &str) -> UsageEntry {
        UsageEntry {
            timestamp: self.timestamp,
            session_id: self.session_id.clone(),
            project_path: project_path.to_owned(),
            model_name: self.model_name.clone(),
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            cache_creation_tokens: self.cache_creation_tokens,
            cache_read_tokens: self.cache_read_tokens,
            total_cost: self.total_cost,
            message_key: String::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
struct Totals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_cost: f64,
}

impl Totals {
    fn add_entry(&mut self, entry: &UsageEntry) {
        self.input_tokens += entry.input_tokens;
        self.output_tokens += entry.output_tokens;
        self.cache_creation_tokens += entry.cache_creation_tokens;
        self.cache_read_tokens += entry.cache_read_tokens;
        self.total_cost += entry.total_cost;
    }

    fn total_tokens(&self) -> i64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

#[derive(Debug, Default, Clone)]
struct Aggregate {
    totals: Totals,
    by_model: BTreeMap<String, Totals>,
}

impl Aggregate {
    fn add_entry(&mut self, entry: &UsageEntry) {
        self.totals.add_entry(entry);
        let model_name = super::cluster_model_name(&entry.model_name);
        self.by_model
            .entry(model_name.to_string())
            .or_default()
            .add_entry(entry);
    }

    fn models_used(&self) -> Vec<String> {
        self.by_model.keys().cloned().collect()
    }

    fn model_breakdowns(&self) -> Vec<Value> {
        self.by_model
            .iter()
            .map(|(model, totals)| {
                json!({
                    "modelName": model,
                    "inputTokens": totals.input_tokens,
                    "outputTokens": totals.output_tokens,
                    "cacheCreationTokens": totals.cache_creation_tokens,
                    "cacheReadTokens": totals.cache_read_tokens,
                    "cost": totals.total_cost,
                })
            })
            .collect()
    }
}

pub fn load_source_view(view: &str, refresh: bool) -> Result<Vec<Value>, String> {
    load_source_view_since(view, refresh, None)
}

pub fn load_source_view_since(
    view: &str,
    _refresh: bool,
    watermark_ms: Option<i64>,
) -> Result<Vec<Value>, String> {
    let view = SourceView::try_from(view)?;

    if view == SourceView::Blocks {
        return Ok(Vec::new());
    }

    let entries = load_usage_entries(watermark_ms)?;
    Ok(match view {
        SourceView::Daily => entries_to_daily(&entries),
        SourceView::Monthly => entries_to_monthly(&entries),
        SourceView::Sessions => entries_to_sessions(&entries),
        SourceView::Blocks => Vec::new(),
        SourceView::Messages => entries_to_messages(&entries),
    })
}

fn load_usage_entries(watermark_ms: Option<i64>) -> Result<Vec<UsageEntry>, String> {
    let sessions_dirs = discover_sessions_dirs();
    if sessions_dirs.is_empty() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut workflow_runs_by_cwd = BTreeMap::new();

    for sessions_dir in sessions_dirs {
        if !sessions_dir.is_dir() {
            continue;
        }

        let mut files = Vec::new();
        collect_jsonl_files(&sessions_dir, &mut files)?;
        files.sort();

        for file in files {
            if let Some(watermark) = watermark_ms {
                if !super::file_modified_after(&file, watermark) {
                    continue;
                }
            }
            append_entries_from_file(
                &sessions_dir,
                &file,
                &mut seen,
                &mut entries,
                &mut workflow_runs_by_cwd,
            )?;
        }
    }

    entries.sort_by_key(|entry| entry.timestamp.timestamp_millis());
    Ok(entries)
}

fn discover_sessions_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let pi_agent_dir = std::env::var_os(PI_AGENT_DIR_ENV).map(PathBuf::from);
    let oh_my_pi_agent_dir = std::env::var_os(OH_MY_PI_AGENT_DIR_ENV).map(PathBuf::from);
    let oh_my_pi_config_dir = std::env::var_os(OH_MY_PI_CONFIG_DIR_ENV).map(PathBuf::from);
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);

    discover_sessions_dirs_from(
        home.as_deref(),
        pi_agent_dir.as_deref(),
        oh_my_pi_agent_dir.as_deref(),
        oh_my_pi_config_dir.as_deref(),
        xdg_data_home.as_deref(),
    )
}

fn discover_sessions_dirs_from(
    home: Option<&Path>,
    pi_agent_dir: Option<&Path>,
    oh_my_pi_agent_dir: Option<&Path>,
    oh_my_pi_config_dir: Option<&Path>,
    xdg_data_home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(path) = pi_agent_dir {
        if path.is_dir() {
            push_unique_dir(&mut dirs, path.to_path_buf());
            if let Some(path) = oh_my_pi_agent_dir {
                push_unique_dir(&mut dirs, path.join("sessions"));
            }
            return dirs;
        }
    }

    if let Some(home) = home {
        push_unique_dir(&mut dirs, home.join(".pi").join("agent").join("sessions"));
    }

    if let Some(path) = oh_my_pi_agent_dir {
        push_unique_dir(&mut dirs, path.join("sessions"));
        return dirs;
    }

    if let Some(path) = xdg_data_home
        .map(|path| path.join("omp"))
        .filter(|path| path.is_dir())
    {
        push_unique_dir(&mut dirs, path.join("sessions"));
        return dirs;
    }

    let Some(home) = home else {
        return dirs;
    };
    let oh_my_pi_config_dir = oh_my_pi_config_dir.unwrap_or_else(|| Path::new(".omp"));
    push_unique_dir(
        &mut dirs,
        home.join(oh_my_pi_config_dir)
            .join("agent")
            .join("sessions"),
    );

    dirs
}

fn push_unique_dir(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if !dirs.iter().any(|existing| existing == &path) {
        dirs.push(path);
    }
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(format!("failed to read {}: {err}", dir.display())),
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("jsonl"))
        {
            files.push(path);
        }
    }

    Ok(())
}

fn append_entries_from_file(
    sessions_dir: &Path,
    file_path: &Path,
    seen: &mut BTreeSet<String>,
    entries: &mut Vec<UsageEntry>,
    workflow_runs_by_cwd: &mut BTreeMap<PathBuf, Vec<WorkflowRunUsage>>,
) -> Result<(), String> {
    let file = match File::open(file_path) {
        Ok(file) => file,
        Err(_) => return Ok(()),
    };

    let session_id = extract_session_id(file_path);
    let project_path = extract_project_path(sessions_dir, file_path);
    let mut session_cwd: Option<PathBuf> = None;

    for line in BufReader::new(file).lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        if session_cwd.is_none() {
            session_cwd = extract_session_cwd(&value);
        }
        for (entry_index, entry) in parse_usage_entries(&value, &session_id, &project_path)
            .into_iter()
            .enumerate()
        {
            let hash = format!(
                "pi:{}:{}:{}:{}:{}:{}:{}",
                project_path,
                session_id,
                entry.timestamp.to_rfc3339(),
                entry.model_name,
                entry.total_tokens(),
                entry.total_cost,
                entry_index
            );
            if seen.insert(hash.clone()) {
                entries.push(UsageEntry {
                    message_key: hash,
                    ..entry
                });
            }
        }
    }

    append_workflow_entries_from_cwd(
        session_cwd.as_deref(),
        &session_id,
        &project_path,
        seen,
        entries,
        workflow_runs_by_cwd,
    )?;

    Ok(())
}

fn append_workflow_entries_from_cwd(
    session_cwd: Option<&Path>,
    session_id: &str,
    project_path: &str,
    seen: &mut BTreeSet<String>,
    entries: &mut Vec<UsageEntry>,
    workflow_runs_by_cwd: &mut BTreeMap<PathBuf, Vec<WorkflowRunUsage>>,
) -> Result<(), String> {
    let Some(session_cwd) = session_cwd else {
        return Ok(());
    };

    if !workflow_runs_by_cwd.contains_key(session_cwd) {
        let workflow_runs = load_workflow_run_usages(session_cwd)?;
        workflow_runs_by_cwd.insert(session_cwd.to_path_buf(), workflow_runs);
    }
    let Some(workflow_runs) = workflow_runs_by_cwd.get(session_cwd) else {
        return Ok(());
    };

    for workflow_run in workflow_runs
        .iter()
        .filter(|workflow_run| workflow_run.session_id == session_id)
    {
        let entry = workflow_run.to_usage_entry(project_path);
        let hash = format!(
            "pi-workflow:{}:{}:{}:{}:{}",
            project_path,
            session_id,
            workflow_run.run_id,
            entry.timestamp.to_rfc3339(),
            entry.total_tokens()
        );
        if seen.insert(hash.clone()) {
            entries.push(UsageEntry {
                message_key: hash,
                ..entry
            });
        }
    }

    Ok(())
}

fn load_workflow_run_usages(session_cwd: &Path) -> Result<Vec<WorkflowRunUsage>, String> {
    let runs_dir = session_cwd.join(".pi").join("workflows").join("runs");
    let run_files = match fs::read_dir(&runs_dir) {
        Ok(run_files) => run_files,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(format!("failed to read {}: {err}", runs_dir.display())),
    };

    let mut workflow_runs = Vec::new();
    for run_file in run_files {
        let Ok(run_file) = run_file else {
            continue;
        };
        let path = run_file.path();
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&contents) else {
            continue;
        };
        let Some(workflow_run) = workflow_run_usage_from_value(&value, &path) else {
            continue;
        };
        workflow_runs.push(workflow_run);
    }

    Ok(workflow_runs)
}

fn parse_usage_entries(value: &Value, session_id: &str, project_path: &str) -> Vec<UsageEntry> {
    parse_usage_entry(value, session_id, project_path)
        .into_iter()
        .chain(parse_subagent_usage_entries(
            value,
            session_id,
            project_path,
        ))
        .collect()
}

fn parse_usage_entry(value: &Value, session_id: &str, project_path: &str) -> Option<UsageEntry> {
    let entry_type = value.get("type").and_then(Value::as_str);
    if entry_type.is_some_and(|entry_type| entry_type != "message") {
        return None;
    }

    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)?;
    let message = value.get("message")?.as_object()?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }

    let usage = message.get("usage")?.as_object()?;
    let input = usage.get("input")?;
    let output = usage.get("output")?;
    let input_tokens = to_i64(input);
    let output_tokens = to_i64(output);
    let cache_creation_tokens = usage.get("cacheWrite").map(to_i64).unwrap_or_default();
    let cache_read_tokens = usage.get("cacheRead").map(to_i64).unwrap_or_default();
    let provider_name = message
        .get("provider")
        .and_then(Value::as_str)
        .filter(|provider| !provider.is_empty());
    let raw_model_name = message
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .unwrap_or("unknown");
    let model_name = normalize_model_name(provider_name, raw_model_name);
    let explicit_cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .map(num)
        .unwrap_or_default();
    let total_cost = if explicit_cost > 0.0 {
        explicit_cost
    } else {
        model_cost_usd(
            &model_name,
            TokenUsage {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            },
        )
    };

    let entry = UsageEntry {
        timestamp,
        session_id: session_id.to_owned(),
        project_path: project_path.to_owned(),
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
        message_key: String::new(),
    };

    (entry.total_tokens() > 0 || entry.total_cost > 0.0).then_some(entry)
}

fn parse_subagent_usage_entries(
    value: &Value,
    session_id: &str,
    project_path: &str,
) -> Vec<UsageEntry> {
    let entry_type = value.get("type").and_then(Value::as_str);
    if entry_type.is_some_and(|entry_type| entry_type != "message") {
        return Vec::new();
    }

    let Some(timestamp) = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
    else {
        return Vec::new();
    };
    let Some(message) = value.get("message").and_then(Value::as_object) else {
        return Vec::new();
    };
    if message.get("role").and_then(Value::as_str) != Some("toolResult")
        || message.get("toolName").and_then(Value::as_str) != Some("subagent")
    {
        return Vec::new();
    }

    message
        .get("details")
        .and_then(|details| details.get("results"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let result = result.as_object()?;
            let usage = result.get("usage")?.as_object()?;
            let input_tokens = usage.get("input").map(to_i64).unwrap_or_default();
            let output_tokens = usage.get("output").map(to_i64).unwrap_or_default();
            let cache_creation_tokens = usage.get("cacheWrite").map(to_i64).unwrap_or_default();
            let cache_read_tokens = usage.get("cacheRead").map(to_i64).unwrap_or_default();
            let raw_model_name = result
                .get("model")
                .and_then(Value::as_str)
                .filter(|model| !model.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| {
                    result
                        .get("agent")
                        .and_then(Value::as_str)
                        .filter(|agent| !agent.is_empty())
                        .map(|agent| format!("subagent/{agent}"))
                })
                .unwrap_or_else(|| "subagent/unknown".to_string());
            let model_name = normalize_model_name(None, &raw_model_name);
            let explicit_cost = usage.get("cost").map(num).unwrap_or_default();
            let total_cost = if explicit_cost > 0.0 {
                explicit_cost
            } else {
                model_cost_usd(
                    &model_name,
                    TokenUsage {
                        input_tokens,
                        output_tokens,
                        cache_creation_tokens,
                        cache_read_tokens,
                    },
                )
            };

            let entry = UsageEntry {
                timestamp,
                session_id: session_id.to_owned(),
                project_path: project_path.to_owned(),
                model_name,
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
                total_cost,
                message_key: String::new(),
            };

            (entry.total_tokens() > 0 || entry.total_cost > 0.0).then_some(entry)
        })
        .collect()
}

#[cfg(test)]
fn workflow_usage_entry_from_value(
    value: &Value,
    session_id: &str,
    project_path: &str,
) -> Option<UsageEntry> {
    let workflow_run = workflow_run_usage_from_value(value, Path::new("unknown"))?;
    if workflow_run.session_id != session_id {
        return None;
    }

    Some(workflow_run.to_usage_entry(project_path))
}

fn workflow_run_usage_from_value(value: &Value, path: &Path) -> Option<WorkflowRunUsage> {
    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.is_empty())?
        .to_owned();
    let timestamp = ["completedAt", "updatedAt", "startedAt"]
        .iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(Value::as_str)
                .and_then(parse_timestamp)
        })?;
    let usage = value.get("tokenUsage")?.as_object()?;
    let declared_total = usage.get("total").map(to_i64).unwrap_or_default();
    let mut input_tokens = usage.get("input").map(to_i64).unwrap_or_default();
    let output_tokens = usage.get("output").map(to_i64).unwrap_or_default();
    let cache_creation_tokens = usage.get("cacheWrite").map(to_i64).unwrap_or_default();
    let cache_read_tokens = usage.get("cacheRead").map(to_i64).unwrap_or_default();
    let component_total = input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens;
    if declared_total > component_total {
        input_tokens += declared_total - component_total;
    }
    let total_cost = usage
        .get("cost")
        .and_then(|cost| cost.get("total").or(Some(cost)))
        .map(num)
        .unwrap_or_default();
    let model_name = workflow_model_name(value);
    let run_id = value
        .get("runId")
        .and_then(Value::as_str)
        .filter(|run_id| !run_id.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown")
                .to_owned()
        });

    let workflow_run = WorkflowRunUsage {
        run_id,
        session_id,
        timestamp,
        model_name,
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        total_cost,
    };

    (workflow_run.total_tokens() > 0 || workflow_run.total_cost > 0.0).then_some(workflow_run)
}

fn entries_to_daily(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_by(entries, |entry| {
        entry.timestamp.format("%Y-%m-%d").to_string()
    })
    .into_iter()
    .map(|(date, aggregate)| usage_row("date", date, aggregate))
    .collect()
}

fn entries_to_monthly(entries: &[UsageEntry]) -> Vec<Value> {
    aggregate_by(entries, |entry| entry.timestamp.format("%Y-%m").to_string())
        .into_iter()
        .map(|(month, aggregate)| usage_row("month", month, aggregate))
        .collect()
}

fn entries_to_sessions(entries: &[UsageEntry]) -> Vec<Value> {
    let mut grouped: BTreeMap<(String, String), (DateTime<Local>, Aggregate)> = BTreeMap::new();

    for entry in entries {
        let key = (entry.project_path.clone(), entry.session_id.clone());
        let (last_activity, aggregate) = grouped
            .entry(key)
            .or_insert_with(|| (entry.timestamp, Aggregate::default()));
        if entry.timestamp > *last_activity {
            *last_activity = entry.timestamp;
        }
        aggregate.add_entry(entry);
    }

    let mut rows = grouped
        .into_iter()
        .map(|((project_path, session_id), (last_activity, aggregate))| {
            json!({
                "sessionId": session_id,
                "projectPath": project_path,
                "date": last_activity.format("%Y-%m-%d").to_string(),
                "time": last_activity.format("%H:%M").to_string(),
                "inputTokens": aggregate.totals.input_tokens,
                "outputTokens": aggregate.totals.output_tokens,
                "cacheCreationTokens": aggregate.totals.cache_creation_tokens,
                "cacheReadTokens": aggregate.totals.cache_read_tokens,
                "totalTokens": aggregate.totals.total_tokens(),
                "totalCost": aggregate.totals.total_cost,
                "modelsUsed": aggregate.models_used(),
                "modelBreakdowns": aggregate.model_breakdowns(),
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(sort_key);
    rows
}

/// One row per usage entry, keyed by the stable content hash assigned during
/// the scan, so ledger upserts stay idempotent across incremental re-scans.
fn entries_to_messages(entries: &[UsageEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|entry| {
            json!({
                "messageId": entry.message_key,
                "sessionId": entry.session_id,
                "date": entry.timestamp.format("%Y-%m-%d").to_string(),
                "time": entry.timestamp.format("%H:%M").to_string(),
                "inputTokens": entry.input_tokens,
                "outputTokens": entry.output_tokens,
                "cacheCreationTokens": entry.cache_creation_tokens,
                "cacheReadTokens": entry.cache_read_tokens,
                "totalTokens": entry.total_tokens(),
                "cost": entry.total_cost,
            })
        })
        .collect()
}

fn aggregate_by(
    entries: &[UsageEntry],
    key_for: impl Fn(&UsageEntry) -> String,
) -> BTreeMap<String, Aggregate> {
    let mut grouped: BTreeMap<String, Aggregate> = BTreeMap::new();
    for entry in entries {
        grouped.entry(key_for(entry)).or_default().add_entry(entry);
    }
    grouped
}

fn usage_row(key_name: &str, key: String, aggregate: Aggregate) -> Value {
    json!({
        key_name: key,
        "inputTokens": aggregate.totals.input_tokens,
        "outputTokens": aggregate.totals.output_tokens,
        "cacheCreationTokens": aggregate.totals.cache_creation_tokens,
        "cacheReadTokens": aggregate.totals.cache_read_tokens,
        "totalTokens": aggregate.totals.total_tokens(),
        "totalCost": aggregate.totals.total_cost,
        "modelsUsed": aggregate.models_used(),
        "modelBreakdowns": aggregate.model_breakdowns(),
    })
}

fn parse_timestamp(value: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date_time| date_time.with_timezone(&Local))
}

fn workflow_model_name(value: &Value) -> String {
    let mut models = BTreeSet::new();
    if let Some(agents) = value.get("agents").and_then(Value::as_array) {
        for agent in agents {
            if let Some(model) = agent
                .get("model")
                .and_then(Value::as_str)
                .filter(|model| !model.is_empty())
            {
                models.insert(model.to_owned());
            }
        }
    }

    if models.len() == 1 {
        return models
            .into_iter()
            .next()
            .unwrap_or_else(|| "workflow".to_string());
    }
    value
        .get("workflowName")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(|name| format!("workflow/{name}"))
        .unwrap_or_else(|| "workflow".to_string())
}

fn normalize_model_name(provider_name: Option<&str>, model_name: &str) -> String {
    if provider_name.is_some_and(|provider| provider.eq_ignore_ascii_case("kiro"))
        && (model_name.eq_ignore_ascii_case("claude-opus-4.7")
            || model_name.eq_ignore_ascii_case("claude-opus-4-7"))
    {
        return "kiro-claude-opus-4-7".to_string();
    }
    model_name.to_owned()
}

fn extract_session_id(file_path: &Path) -> String {
    let stem = file_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("unknown");
    stem.split_once('_')
        .map(|(_, session_id)| session_id)
        .unwrap_or(stem)
        .to_owned()
}

fn extract_project_path(sessions_dir: &Path, file_path: &Path) -> String {
    file_path
        .strip_prefix(sessions_dir)
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        .filter(|project| !project.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn extract_session_cwd(value: &Value) -> Option<PathBuf> {
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return None;
    }
    value
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !cwd.is_empty())
        .map(PathBuf::from)
}

fn sort_key(value: &Value) -> String {
    let date = value
        .get("date")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let time = value
        .get("time")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!("{date}T{time}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_usage_entry_accepts_assistant_messages_with_usage() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "assistant",
                "model": "anthropic/claude-opus-4.5",
                "usage": {
                    "input": 100,
                    "output": 40,
                    "cacheRead": 8,
                    "cacheWrite": 12,
                    "cost": { "total": 0.25 }
                }
            }
        });

        let entry = parse_usage_entry(&raw, "session-1", "project-1").unwrap();

        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.project_path, "project-1");
        assert_eq!(entry.model_name, "anthropic/claude-opus-4.5");
        assert_eq!(entry.input_tokens, 100);
        assert_eq!(entry.output_tokens, 40);
        assert_eq!(entry.cache_read_tokens, 8);
        assert_eq!(entry.cache_creation_tokens, 12);
        assert_eq!(entry.total_cost, 0.25);
    }

    #[test]
    fn parse_usage_entry_names_kiro_opus_model_like_factory_droid() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "assistant",
                "provider": "kiro",
                "model": "claude-opus-4.7",
                "usage": {
                    "input": 100,
                    "output": 40,
                    "cost": { "total": 0.25 }
                }
            }
        });

        let entry = parse_usage_entry(&raw, "session-1", "project-1").unwrap();

        assert_eq!(entry.model_name, "kiro-claude-opus-4-7");
        assert_eq!(entry.total_cost, 0.25);
    }

    #[test]
    fn parse_usage_entry_rejects_non_assistant_messages() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "user",
                "usage": {
                    "input": 100,
                    "output": 40
                }
            }
        });

        assert!(parse_usage_entry(&raw, "session-1", "project-1").is_none());
    }

    #[test]
    fn parse_usage_entries_accepts_subagent_tool_results() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "toolResult",
                "toolName": "subagent",
                "details": {
                    "mode": "parallel",
                    "results": [
                        {
                            "agent": "explorer",
                            "model": "anthropic/claude-opus-4.5",
                            "usage": {
                                "input": 100,
                                "output": 40,
                                "cacheRead": 8,
                                "cacheWrite": 12,
                                "cost": 0.25
                            },
                            "messages": [
                                {
                                    "role": "assistant",
                                    "usage": {
                                        "input": 999,
                                        "output": 999
                                    }
                                }
                            ]
                        },
                        {
                            "agent": "reviewer",
                            "usage": {
                                "input": 5,
                                "output": 7
                            }
                        }
                    ]
                }
            }
        });

        let entries = parse_usage_entries(&raw, "session-1", "project-1");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].model_name, "anthropic/claude-opus-4.5");
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[0].output_tokens, 40);
        assert_eq!(entries[0].cache_read_tokens, 8);
        assert_eq!(entries[0].cache_creation_tokens, 12);
        assert_eq!(entries[0].total_cost, 0.25);
        assert_eq!(entries[1].model_name, "subagent/reviewer");
        assert_eq!(entries[1].input_tokens, 5);
        assert_eq!(entries[1].output_tokens, 7);
    }

    #[test]
    fn parse_usage_entries_rejects_non_subagent_tool_results() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "toolResult",
                "toolName": "bash",
                "details": {
                    "results": [
                        {
                            "usage": {
                                "input": 100,
                                "output": 40
                            }
                        }
                    ]
                }
            }
        });

        assert!(parse_usage_entries(&raw, "session-1", "project-1").is_empty());
    }

    #[test]
    fn parse_usage_entry_prices_mimo_when_cost_is_missing() {
        let raw = json!({
            "type": "message",
            "timestamp": "2026-01-02T03:04:05Z",
            "message": {
                "role": "assistant",
                "model": "mimo-v2.5-pro",
                "usage": {
                    "input": 6200,
                    "output": 33
                }
            }
        });

        let entry = parse_usage_entry(&raw, "session-1", "project-1").unwrap();

        assert!(entry.total_cost > 0.0);
    }

    #[test]
    fn entries_to_sessions_groups_project_and_session() {
        let first = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T03:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-a",
                    "usage": { "input": 10, "output": 20 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();
        let second = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T04:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-b",
                    "usage": { "input": 5, "output": 7, "cacheRead": 3 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();

        let rows = entries_to_sessions(&[first, second]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("session-1"));
        assert_eq!(rows[0]["projectPath"], json!("project-1"));
        assert_eq!(rows[0]["inputTokens"], json!(15));
        assert_eq!(rows[0]["outputTokens"], json!(27));
        assert_eq!(rows[0]["cacheReadTokens"], json!(3));
        assert_eq!(rows[0]["modelsUsed"], json!(["model-a", "model-b"]));
    }

    #[test]
    fn workflow_usage_entry_counts_persisted_dynamic_workflow_run() {
        let raw = json!({
            "runId": "run-1",
            "workflowName": "repo_audit",
            "sessionId": "session-1",
            "status": "completed",
            "completedAt": "2026-01-02T05:04:05Z",
            "agents": [
                { "label": "scan", "model": "cliproxy/gpt-5.5", "tokens": 100 },
                { "label": "review", "model": "cliproxy/gpt-5.5", "tokens": 200 }
            ],
            "tokenUsage": {
                "input": 120,
                "output": 30,
                "cacheRead": 40,
                "cacheWrite": 5,
                "total": 200,
                "cost": 0.42
            }
        });

        let entry = workflow_usage_entry_from_value(&raw, "session-1", "project-1").unwrap();

        assert_eq!(entry.session_id, "session-1");
        assert_eq!(entry.project_path, "project-1");
        assert_eq!(entry.model_name, "cliproxy/gpt-5.5");
        assert_eq!(entry.input_tokens, 125);
        assert_eq!(entry.output_tokens, 30);
        assert_eq!(entry.cache_read_tokens, 40);
        assert_eq!(entry.cache_creation_tokens, 5);
        assert_eq!(entry.total_tokens(), 200);
        assert_eq!(entry.total_cost, 0.42);
    }

    #[test]
    fn workflow_usage_entry_accepts_cost_total_object() {
        let raw = json!({
            "runId": "run-1",
            "sessionId": "session-1",
            "completedAt": "2026-01-02T05:04:05Z",
            "tokenUsage": {
                "input": 120,
                "output": 30,
                "total": 150,
                "cost": { "total": 0.42 }
            }
        });

        let entry = workflow_usage_entry_from_value(&raw, "session-1", "project-1").unwrap();

        assert_eq!(entry.total_cost, 0.42);
    }

    #[test]
    fn workflow_usage_entry_rejects_other_sessions() {
        let raw = json!({
            "runId": "run-1",
            "sessionId": "other-session",
            "completedAt": "2026-01-02T05:04:05Z",
            "tokenUsage": {
                "input": 120,
                "output": 30,
                "total": 150
            }
        });

        assert!(workflow_usage_entry_from_value(&raw, "session-1", "project-1").is_none());
    }

    #[test]
    fn load_usage_entries_merges_workflow_runs_from_session_cwd() {
        let root = temp_root("pi-workflow");
        let sessions_dir = root.join("sessions");
        let project_sessions_dir = sessions_dir.join("project-1");
        let project_cwd = root.join("project");
        let runs_dir = project_cwd.join(".pi/workflows/runs");
        fs::create_dir_all(&project_sessions_dir).unwrap();
        fs::create_dir_all(&runs_dir).unwrap();
        fs::write(
            project_sessions_dir.join("2026-01-02T03-04-05-000Z_session-1.jsonl"),
            format!(
                "{}\n{}\n",
                json!({
                    "type": "session",
                    "version": 3,
                    "id": "session-1",
                    "timestamp": "2026-01-02T03:04:05Z",
                    "cwd": project_cwd
                }),
                json!({
                    "type": "message",
                    "timestamp": "2026-01-02T03:04:05Z",
                    "message": {
                        "role": "assistant",
                        "model": "model-a",
                        "usage": { "input": 10, "output": 20 }
                    }
                })
            ),
        )
        .unwrap();
        fs::write(
            runs_dir.join("run-1.json"),
            json!({
                "runId": "run-1",
                "workflowName": "repo_audit",
                "sessionId": "session-1",
                "status": "completed",
                "completedAt": "2026-01-02T05:04:05Z",
                "agents": [{ "label": "scan", "model": "cliproxy/gpt-5.5", "tokens": 200 }],
                "tokenUsage": {
                    "input": 120,
                    "output": 30,
                    "cacheRead": 40,
                    "total": 200,
                    "cost": 0.42
                }
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            runs_dir.join("other-run.json"),
            json!({
                "runId": "other-run",
                "sessionId": "other-session",
                "completedAt": "2026-01-02T05:04:05Z",
                "tokenUsage": { "input": 999, "output": 1, "total": 1000 }
            })
            .to_string(),
        )
        .unwrap();

        let mut entries = Vec::new();
        let mut seen = BTreeSet::new();
        let mut workflow_runs_by_cwd = BTreeMap::new();
        append_entries_from_file(
            &sessions_dir,
            &project_sessions_dir.join("2026-01-02T03-04-05-000Z_session-1.jsonl"),
            &mut seen,
            &mut entries,
            &mut workflow_runs_by_cwd,
        )
        .unwrap();

        let rows = entries_to_sessions(&entries);

        assert_eq!(entries.len(), 2);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], json!("session-1"));
        assert_eq!(rows[0]["inputTokens"], json!(140));
        assert_eq!(rows[0]["outputTokens"], json!(50));
        assert_eq!(rows[0]["cacheReadTokens"], json!(40));
        assert_eq!(rows[0]["totalTokens"], json!(230));
        assert_eq!(
            rows[0]["modelsUsed"],
            json!(["gpt-5.5", "model-a"])
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discover_sessions_dirs_includes_pi_and_oh_my_pi_defaults() {
        let root = temp_root("pi-dirs");
        let home = root.join("home");
        let pi_sessions = home.join(".pi/agent/sessions");
        let oh_my_pi_sessions = home.join(".omp/agent/sessions");
        fs::create_dir_all(&pi_sessions).unwrap();
        fs::create_dir_all(&oh_my_pi_sessions).unwrap();

        let dirs = discover_sessions_dirs_from(Some(&home), None, None, None, None);

        assert_eq!(dirs, vec![pi_sessions, oh_my_pi_sessions]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discover_sessions_dirs_uses_oh_my_pi_agent_override() {
        let root = temp_root("pi-omp-override");
        let home = root.join("home");
        let pi_sessions = home.join(".pi/agent/sessions");
        let oh_my_pi_agent = root.join("custom-omp-agent");
        let oh_my_pi_sessions = oh_my_pi_agent.join("sessions");
        fs::create_dir_all(&pi_sessions).unwrap();
        fs::create_dir_all(&oh_my_pi_sessions).unwrap();

        let dirs =
            discover_sessions_dirs_from(Some(&home), None, Some(&oh_my_pi_agent), None, None);

        assert_eq!(dirs, vec![pi_sessions, oh_my_pi_sessions]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discover_sessions_dirs_uses_oh_my_pi_xdg_data_home() {
        let root = temp_root("pi-omp-xdg");
        let home = root.join("home");
        let pi_sessions = home.join(".pi/agent/sessions");
        let xdg_root = root.join("xdg/omp");
        let oh_my_pi_sessions = xdg_root.join("sessions");
        fs::create_dir_all(&pi_sessions).unwrap();
        fs::create_dir_all(&oh_my_pi_sessions).unwrap();

        let dirs =
            discover_sessions_dirs_from(Some(&home), None, None, None, Some(&root.join("xdg")));

        assert_eq!(dirs, vec![pi_sessions, oh_my_pi_sessions]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn entries_to_daily_and_monthly_use_expected_period_keys() {
        let entry = parse_usage_entry(
            &json!({
                "timestamp": "2026-01-02T03:04:05Z",
                "message": {
                    "role": "assistant",
                    "model": "model-a",
                    "usage": { "input": 10, "output": 20 }
                }
            }),
            "session-1",
            "project-1",
        )
        .unwrap();

        let daily = entries_to_daily(std::slice::from_ref(&entry));
        let monthly = entries_to_monthly(&[entry]);

        assert_eq!(daily[0]["date"], json!("2026-01-02"));
        assert_eq!(monthly[0]["month"], json!("2026-01"));
    }

    #[test]
    fn extracts_session_id_after_timestamp_prefix() {
        let path = Path::new("/tmp/sessions/project/2025-12-19T08-12-33-794Z_2c16ab69.jsonl");

        assert_eq!(extract_session_id(path), "2c16ab69");
    }

    fn temp_root(label: &str) -> PathBuf {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("token-usage-{label}-{now}"))
    }
}
