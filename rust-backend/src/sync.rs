use crate::{
    ledger::UsageLedger,
    protocol::{Source, View},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
};

const FORMAT_VERSION: u32 = 1;
const SYNC_DIRECTORY: &str = ".token-usage-sync/v1/devices";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub files: usize,
    pub sessions: usize,
    pub blocks: usize,
    pub messages: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSyncResult {
    pub imported: SyncSummary,
    pub exported: SyncSummary,
    pub committed: bool,
    pub pushed: bool,
    pub commit: Option<String>,
    pub attempts: usize,
}

impl SyncSummary {
    pub fn records(self) -> usize {
        self.sessions + self.blocks + self.messages
    }

    fn add_kind(&mut self, kind: RecordKind) {
        match kind {
            RecordKind::Session => self.sessions += 1,
            RecordKind::Block => self.blocks += 1,
            RecordKind::Message => self.messages += 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RecordKind {
    Session,
    Block,
    Message,
}

impl RecordKind {
    fn id_field(self) -> &'static str {
        match self {
            Self::Session => "sessionId",
            Self::Block => "blockId",
            Self::Message => "messageId",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SyncRecord {
    version: u32,
    kind: RecordKind,
    source: String,
    row: Value,
}

struct Candidate {
    record: SyncRecord,
    source: Source,
    total_tokens: i64,
    canonical: String,
}

pub fn export_to_git_repo(
    ledger: &UsageLedger,
    repository: &Path,
    device_id: &str,
) -> Result<(PathBuf, SyncSummary), String> {
    validate_device_id(device_id)?;
    let repository = git_repository_root(repository)?;
    let devices_directory = repository.join(SYNC_DIRECTORY);
    fs::create_dir_all(&devices_directory).map_err(|err| {
        format!(
            "failed to create sync directory {}: {err}",
            devices_directory.display()
        )
    })?;

    let mut records = Vec::new();
    for source in Source::ALL {
        append_records(
            &mut records,
            source,
            RecordKind::Session,
            ledger.load_view(source, View::Sessions)?,
        )?;
        append_records(
            &mut records,
            source,
            RecordKind::Block,
            ledger.load_view(source, View::Blocks)?,
        )?;
        append_records(
            &mut records,
            source,
            RecordKind::Message,
            ledger.load_message_sync_rows(source)?,
        )?;
    }
    records.sort_by_key(record_key);

    let mut contents = String::new();
    let mut summary = SyncSummary::default();
    for record in records {
        summary.add_kind(record.kind);
        contents.push_str(
            &serde_json::to_string(&record)
                .map_err(|err| format!("failed to serialize sync record: {err}"))?,
        );
        contents.push('\n');
    }

    let snapshot_path = devices_directory.join(format!("{device_id}.jsonl"));
    atomic_write(&snapshot_path, contents.as_bytes())?;
    summary.files = 1;
    Ok((snapshot_path, summary))
}

pub fn import_from_git_repo(
    ledger: &UsageLedger,
    repository: &Path,
) -> Result<SyncSummary, String> {
    let repository = git_repository_root(repository)?;
    let devices_directory = repository.join(SYNC_DIRECTORY);
    if !devices_directory.exists() {
        return Ok(SyncSummary::default());
    }

    let entries = fs::read_dir(&devices_directory).map_err(|err| {
        format!(
            "failed to read sync directory {}: {err}",
            devices_directory.display()
        )
    })?;
    let mut snapshot_paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| {
            format!(
                "failed to read an entry in {}: {err}",
                devices_directory.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("failed to inspect sync snapshot {}: {err}", path.display()))?;
        if file_type.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
        {
            snapshot_paths.push(path);
        }
    }
    snapshot_paths.sort();

    let mut merged = BTreeMap::new();
    for path in &snapshot_paths {
        let contents = fs::read_to_string(path)
            .map_err(|err| format!("failed to read sync snapshot {}: {err}", path.display()))?;
        for (line_index, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let record = serde_json::from_str::<SyncRecord>(line).map_err(|err| {
                format!(
                    "invalid sync record at {}:{}: {err}",
                    path.display(),
                    line_index + 1
                )
            })?;
            let candidate = validate_record(record).map_err(|err| {
                format!(
                    "invalid sync record at {}:{}: {err}",
                    path.display(),
                    line_index + 1
                )
            })?;
            let key = record_key(&candidate.record);
            match merged.entry(key) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(candidate);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    let current = entry.get();
                    if should_replace(current, &candidate) {
                        entry.insert(candidate);
                    }
                }
            }
        }
    }

    let mut summary = SyncSummary {
        files: snapshot_paths.len(),
        ..SyncSummary::default()
    };
    for source in Source::ALL {
        let mut sessions = Vec::new();
        let mut blocks = Vec::new();
        let mut messages = Vec::new();
        for candidate in merged.values().filter(|item| item.source == source) {
            summary.add_kind(candidate.record.kind);
            match candidate.record.kind {
                RecordKind::Session => sessions.push(candidate.record.row.clone()),
                RecordKind::Block => blocks.push(candidate.record.row.clone()),
                RecordKind::Message => messages.push(candidate.record.row.clone()),
            }
        }
        ledger.upsert_view_rows(source, View::Sessions, &sessions)?;
        ledger.upsert_view_rows(source, View::Blocks, &blocks)?;
        ledger.ingest_live_messages(source, &messages)?;
    }

    Ok(summary)
}

pub fn sync_with_git(
    ledger: &UsageLedger,
    repository: &Path,
    device_id: &str,
) -> Result<GitSyncResult, String> {
    validate_device_id(device_id)?;
    let repository = git_repository_root(repository)?;
    ensure_clean_worktree(&repository)?;
    ensure_no_git_operation(&repository)?;
    ensure_tracking_branch(&repository)?;
    pull_with_rebase(&repository)?;

    let mut committed = false;
    for attempt in 1..=2 {
        let imported = import_from_git_repo(ledger, &repository)?;
        let (_, current_export) = export_to_git_repo(ledger, &repository, device_id)?;
        let exported = current_export;
        run_git_checked(
            &repository,
            &["add", "--", ".token-usage-sync"],
            "stage the sync snapshot",
        )?;

        if has_staged_sync_changes(&repository)? {
            run_git_checked(
                &repository,
                &[
                    "-c",
                    "user.name=Token Usage Sync",
                    "-c",
                    "user.email=token-usage-sync@localhost",
                    "commit",
                    "-m",
                    &format!("Sync token usage from {device_id}"),
                    "--",
                    ".token-usage-sync",
                ],
                "commit the sync snapshot",
            )?;
            committed = true;
        }

        if commits_ahead_of_upstream(&repository)? == 0 {
            return Ok(GitSyncResult {
                imported,
                exported,
                committed,
                pushed: false,
                commit: None,
                attempts: attempt,
            });
        }

        let push = git_output(&repository, &["push"])?;
        if push.status.success() {
            return Ok(GitSyncResult {
                imported,
                exported,
                committed,
                pushed: true,
                commit: Some(current_commit(&repository)?),
                attempts: attempt,
            });
        }

        let detail = git_failure_detail(&push);
        if attempt == 2 || !is_non_fast_forward(&detail) {
            return Err(format!("failed to push sync snapshot: {detail}"));
        }
        pull_with_rebase(&repository)?;
    }

    unreachable!("bounded sync loop always returns")
}

fn append_records(
    records: &mut Vec<SyncRecord>,
    source: Source,
    kind: RecordKind,
    rows: Vec<Value>,
) -> Result<(), String> {
    for row in rows {
        let record = SyncRecord {
            version: FORMAT_VERSION,
            kind,
            source: source.to_string(),
            row,
        };
        validate_record(record.clone())?;
        records.push(record);
    }
    Ok(())
}

fn validate_record(mut record: SyncRecord) -> Result<Candidate, String> {
    if record.version != FORMAT_VERSION {
        return Err(format!(
            "unsupported format version {}; expected {FORMAT_VERSION}",
            record.version
        ));
    }
    let source = Source::from_str(&record.source).map_err(|err| err.to_string())?;
    record.source = source.to_string();
    if record
        .row
        .get(record.kind.id_field())
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(format!("missing {}", record.kind.id_field()));
    }
    if record
        .row
        .get("date")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("missing date".to_string());
    }
    let total_tokens = record
        .row
        .get("totalTokens")
        .and_then(Value::as_i64)
        .ok_or_else(|| "totalTokens must be an integer".to_string())?;
    if total_tokens < 0 {
        return Err("totalTokens must not be negative".to_string());
    }
    let canonical = serde_json::to_string(&record)
        .map_err(|err| format!("failed to canonicalize sync record: {err}"))?;
    Ok(Candidate {
        record,
        source,
        total_tokens,
        canonical,
    })
}

fn should_replace(current: &Candidate, candidate: &Candidate) -> bool {
    if candidate.source == Source::Cursor && candidate.record.kind == RecordKind::Session {
        if cursor_session_correction(&current.record.row, &candidate.record.row) {
            return true;
        }
        if cursor_session_correction(&candidate.record.row, &current.record.row) {
            return false;
        }
    }
    candidate.total_tokens > current.total_tokens
        || (candidate.total_tokens == current.total_tokens
            && candidate.canonical > current.canonical)
}

fn cursor_session_correction(existing: &Value, incoming: &Value) -> bool {
    let existing_cache =
        row_i64(existing, "cacheCreationTokens") + row_i64(existing, "cacheReadTokens");
    existing_cache > 0
        && row_i64(existing, "inputTokens")
            >= row_i64(incoming, "inputTokens")
                + row_i64(incoming, "cacheCreationTokens")
                + row_i64(incoming, "cacheReadTokens")
}

fn row_i64(row: &Value, field: &str) -> i64 {
    row.get(field).and_then(Value::as_i64).unwrap_or_default()
}

fn record_key(record: &SyncRecord) -> (RecordKind, String, String) {
    (
        record.kind,
        record.source.clone(),
        record
            .row
            .get(record.kind.id_field())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    )
}

fn validate_device_id(device_id: &str) -> Result<(), String> {
    let valid = !device_id.is_empty()
        && device_id.len() <= 64
        && device_id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        });
    if valid {
        Ok(())
    } else {
        Err(
            "device id must be 1-64 lowercase ASCII letters, digits, hyphens, or underscores"
                .to_string(),
        )
    }
}

fn git_repository_root(path: &Path) -> Result<PathBuf, String> {
    if !path.is_dir() {
        return Err(format!(
            "git repository path is not a directory: {}",
            path.display()
        ));
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map_err(|err| format!("failed to run git: {err}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            format!("not a git repository: {}", path.display())
        } else {
            format!("not a git repository: {}: {detail}", path.display())
        });
    }
    let root = String::from_utf8(output.stdout)
        .map_err(|_| "git returned a non-UTF-8 repository path".to_string())?;
    let root = root.trim();
    if root.is_empty() {
        return Err("git returned an empty repository path".to_string());
    }
    Ok(PathBuf::from(root))
}

fn ensure_clean_worktree(repository: &Path) -> Result<(), String> {
    let output = git_output(
        repository,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    if !output.status.success() {
        return Err(format!(
            "failed to inspect git working tree: {}",
            git_failure_detail(&output)
        ));
    }
    let status = String::from_utf8(output.stdout)
        .map_err(|_| "git returned non-UTF-8 working tree status".to_string())?;
    if status.trim().is_empty() {
        Ok(())
    } else {
        Err(format!(
            "git working tree must be clean before syncing; commit or stash these changes:\n{}",
            status.trim_end()
        ))
    }
}

fn ensure_tracking_branch(repository: &Path) -> Result<(), String> {
    let branch = run_git_checked(
        repository,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        "read the current git branch",
    )?;
    if branch.trim() == "HEAD" {
        return Err("git sync requires a branch; detached HEAD is not supported".to_string());
    }
    run_git_checked(
        repository,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        "find the branch upstream",
    )
    .map(|_| ())
    .map_err(|_| {
        format!(
            "git branch {} has no upstream; push it with --set-upstream first",
            branch.trim()
        )
    })
}

fn ensure_no_git_operation(repository: &Path) -> Result<(), String> {
    for marker in [
        "rebase-merge",
        "rebase-apply",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
    ] {
        let path = run_git_checked(
            repository,
            &["rev-parse", "--git-path", marker],
            "inspect git operation state",
        )?;
        let path = PathBuf::from(path.trim());
        let path = if path.is_absolute() {
            path
        } else {
            repository.join(path)
        };
        if path.exists() {
            return Err(
                "git repository has an unfinished rebase, merge, or cherry-pick; finish or abort it before syncing"
                    .to_string(),
            );
        }
    }
    Ok(())
}

fn pull_with_rebase(repository: &Path) -> Result<(), String> {
    let output = git_output(repository, &["pull", "--rebase"])?;
    if output.status.success() {
        return Ok(());
    }
    let detail = git_failure_detail(&output);
    let _ = git_output(repository, &["rebase", "--abort"]);
    Err(format!("failed to pull sync repository: {detail}"))
}

fn has_staged_sync_changes(repository: &Path) -> Result<bool, String> {
    let output = git_output(
        repository,
        &["diff", "--cached", "--quiet", "--", ".token-usage-sync"],
    )?;
    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(format!(
            "failed to inspect staged sync changes: {}",
            git_failure_detail(&output)
        )),
    }
}

fn commits_ahead_of_upstream(repository: &Path) -> Result<usize, String> {
    let output = run_git_checked(
        repository,
        &["rev-list", "--count", "@{upstream}..HEAD"],
        "compare the local and upstream branches",
    )?;
    output
        .trim()
        .parse()
        .map_err(|err| format!("git returned an invalid ahead count: {err}"))
}

fn current_commit(repository: &Path) -> Result<String, String> {
    run_git_checked(
        repository,
        &["rev-parse", "--short", "HEAD"],
        "read the sync commit",
    )
    .map(|value| value.trim().to_string())
}

fn run_git_checked(repository: &Path, args: &[&str], action: &str) -> Result<String, String> {
    let output = git_output(repository, args)?;
    if !output.status.success() {
        return Err(format!(
            "failed to {action}: {}",
            git_failure_detail(&output)
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|_| format!("git returned non-UTF-8 output for {action}"))
}

fn git_output(repository: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .map_err(|err| format!("failed to run git: {err}"))
}

fn git_failure_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    if detail.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        detail.to_string()
    }
}

fn is_non_fast_forward(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    detail.contains("non-fast-forward")
        || detail.contains("fetch first")
        || detail.contains("[rejected]")
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("sync snapshot has no parent: {}", path.display()))?;
    let temporary_path = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("snapshot"),
        std::process::id()
    ));
    let mut file = File::create(&temporary_path).map_err(|err| {
        format!(
            "failed to create temporary snapshot {}: {err}",
            temporary_path.display()
        )
    })?;
    if let Err(err) = file
        .write_all(contents)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary_path, path))
    {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!(
            "failed to write sync snapshot {}: {err}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn snapshot_round_trip_preserves_sessions_and_message_hours() {
        let directory = TestDirectory::new("round-trip");
        let repository = directory.path().join("repo");
        init_git_repository(&repository);
        let source_ledger =
            UsageLedger::new(directory.path().join("source.sqlite")).expect("source ledger");
        source_ledger
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[session_row("session-1", "2026-07-26", "10:15", 120)],
            )
            .expect("insert source session");
        source_ledger
            .ingest_live_messages(
                Source::Codex,
                &[message_row(
                    "message-1",
                    "session-1",
                    "2026-07-26",
                    "10:15",
                    120,
                )],
            )
            .expect("insert source message");

        let (path, exported) =
            export_to_git_repo(&source_ledger, &repository, "macbook-a").expect("export snapshot");
        let first_contents = fs::read(&path).expect("read first snapshot");
        let (_, exported_again) =
            export_to_git_repo(&source_ledger, &repository, "macbook-a").expect("repeat export");

        assert_eq!(exported.records(), 2);
        assert_eq!(exported, exported_again);
        assert_eq!(
            fs::read(&path).expect("read second snapshot"),
            first_contents
        );

        let target_ledger =
            UsageLedger::new(directory.path().join("target.sqlite")).expect("target ledger");
        let imported =
            import_from_git_repo(&target_ledger, &repository).expect("import target snapshot");
        assert_eq!(imported.records(), 2);
        assert_eq!(
            target_ledger
                .load_view(Source::Codex, View::Sessions)
                .expect("load sessions")
                .len(),
            1
        );
        assert_eq!(
            target_ledger
                .load_hourly(Source::Codex, "2026-07-26")
                .expect("load hourly")[0]["totalTokens"],
            json!(120)
        );

        import_from_git_repo(&target_ledger, &repository).expect("repeat import");
        assert_eq!(
            target_ledger
                .load_view(Source::Codex, View::Sessions)
                .expect("load repeated sessions")
                .len(),
            1
        );
    }

    #[test]
    fn multiple_device_snapshots_merge_by_stable_id_and_larger_total() {
        let directory = TestDirectory::new("merge");
        let repository = directory.path().join("repo");
        init_git_repository(&repository);

        let first = UsageLedger::new(directory.path().join("first.sqlite")).expect("first ledger");
        first
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[session_row("shared", "2026-07-25", "09:00", 100)],
            )
            .expect("insert first");
        export_to_git_repo(&first, &repository, "device-a").expect("export first");

        let second =
            UsageLedger::new(directory.path().join("second.sqlite")).expect("second ledger");
        second
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[
                    session_row("shared", "2026-07-25", "09:00", 150),
                    session_row("second-only", "2026-07-26", "11:00", 40),
                ],
            )
            .expect("insert second");
        export_to_git_repo(&second, &repository, "device-b").expect("export second");

        let target =
            UsageLedger::new(directory.path().join("target.sqlite")).expect("target ledger");
        let summary = import_from_git_repo(&target, &repository).expect("merge snapshots");
        let rows = target
            .load_view(Source::Codex, View::Sessions)
            .expect("load merged rows");

        assert_eq!(summary.files, 2);
        assert_eq!(summary.sessions, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .find(|row| row["sessionId"] == "shared")
                .expect("shared row")["totalTokens"],
            json!(150)
        );
    }

    #[test]
    fn automatic_git_sync_round_trips_between_devices_and_becomes_a_noop() {
        let directory = TestDirectory::new("automatic");
        let (first_repository, second_repository) = init_sync_remote(directory.path());

        let first = UsageLedger::new(directory.path().join("first.sqlite")).expect("first ledger");
        first
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[session_row("first-only", "2026-07-25", "09:00", 100)],
            )
            .expect("insert first device row");
        let first_result =
            sync_with_git(&first, &first_repository, "device-a").expect("sync first device");
        assert!(first_result.committed);
        assert!(first_result.pushed);

        let second =
            UsageLedger::new(directory.path().join("second.sqlite")).expect("second ledger");
        second
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[session_row("second-only", "2026-07-26", "11:00", 40)],
            )
            .expect("insert second device row");
        let second_result =
            sync_with_git(&second, &second_repository, "device-b").expect("sync second device");
        assert_eq!(second_result.imported.sessions, 1);
        assert!(second_result.pushed);

        let merged =
            sync_with_git(&first, &first_repository, "device-a").expect("merge on first device");
        let rows = first
            .load_view(Source::Codex, View::Sessions)
            .expect("load merged sessions");
        assert_eq!(rows.len(), 2);
        assert!(merged.pushed);

        let no_op =
            sync_with_git(&first, &first_repository, "device-a").expect("repeat automatic sync");
        assert!(!no_op.committed);
        assert!(!no_op.pushed);
    }

    #[test]
    fn automatic_git_sync_rejects_a_dirty_worktree() {
        let directory = TestDirectory::new("dirty");
        let (repository, _) = init_sync_remote(directory.path());
        fs::write(repository.join("local-change.txt"), "not committed").expect("write dirty file");
        let ledger = UsageLedger::new(directory.path().join("ledger.sqlite")).expect("ledger");

        let error =
            sync_with_git(&ledger, &repository, "device-a").expect_err("reject dirty worktree");
        assert!(error.contains("must be clean"));
    }

    #[test]
    fn cursor_cache_correction_can_replace_a_larger_legacy_total() {
        let directory = TestDirectory::new("cursor-correction");
        let repository = directory.path().join("repo");
        init_git_repository(&repository);

        let legacy =
            UsageLedger::new(directory.path().join("legacy.sqlite")).expect("legacy ledger");
        legacy
            .upsert_view_rows(
                Source::Cursor,
                View::Sessions,
                &[cursor_session_row(100, 20, 30, 150)],
            )
            .expect("insert legacy row");
        export_to_git_repo(&legacy, &repository, "device-a").expect("export legacy row");

        let corrected =
            UsageLedger::new(directory.path().join("corrected.sqlite")).expect("corrected ledger");
        corrected
            .upsert_view_rows(
                Source::Cursor,
                View::Sessions,
                &[cursor_session_row(50, 20, 30, 100)],
            )
            .expect("insert corrected row");
        export_to_git_repo(&corrected, &repository, "device-b").expect("export corrected row");

        let target =
            UsageLedger::new(directory.path().join("target.sqlite")).expect("target ledger");
        import_from_git_repo(&target, &repository).expect("import corrected row");
        let rows = target
            .load_view(Source::Cursor, View::Sessions)
            .expect("load cursor rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["totalTokens"], json!(100));
    }

    #[test]
    fn invalid_snapshot_is_rejected_before_ledger_changes() {
        let directory = TestDirectory::new("invalid");
        let repository = directory.path().join("repo");
        init_git_repository(&repository);
        let devices = repository.join(SYNC_DIRECTORY);
        fs::create_dir_all(&devices).expect("create devices");
        fs::write(devices.join("broken.jsonl"), "{\"version\":1,\"kind\":")
            .expect("write broken snapshot");

        let target =
            UsageLedger::new(directory.path().join("target.sqlite")).expect("target ledger");
        target
            .upsert_view_rows(
                Source::Codex,
                View::Sessions,
                &[session_row("local", "2026-07-26", "12:00", 20)],
            )
            .expect("insert local row");

        let error = import_from_git_repo(&target, &repository).expect_err("reject broken snapshot");
        assert!(error.contains("broken.jsonl:1"));
        let rows = target
            .load_view(Source::Codex, View::Sessions)
            .expect("load unchanged rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["sessionId"], "local");
    }

    #[test]
    fn export_rejects_unsafe_device_id() {
        let directory = TestDirectory::new("device");
        let repository = directory.path().join("repo");
        init_git_repository(&repository);
        let ledger = UsageLedger::new(directory.path().join("ledger.sqlite")).expect("ledger");

        let error = export_to_git_repo(&ledger, &repository, "../other")
            .expect_err("reject unsafe device id");
        assert!(error.contains("device id"));
    }

    fn session_row(id: &str, date: &str, time: &str, total_tokens: i64) -> Value {
        json!({
            "sessionId": id,
            "date": date,
            "time": time,
            "inputTokens": total_tokens,
            "outputTokens": 0,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 0,
            "totalTokens": total_tokens,
            "totalCost": 0.0,
            "modelsUsed": ["gpt-5"],
            "modelBreakdowns": []
        })
    }

    fn cursor_session_row(
        input_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
        total_tokens: i64,
    ) -> Value {
        json!({
            "sessionId": "cursor-session",
            "date": "2026-07-26",
            "time": "12:00",
            "inputTokens": input_tokens,
            "outputTokens": 0,
            "cacheCreationTokens": cache_creation_tokens,
            "cacheReadTokens": cache_read_tokens,
            "totalTokens": total_tokens,
            "totalCost": 0.0,
            "modelsUsed": ["cursor-model"],
            "modelBreakdowns": []
        })
    }

    fn message_row(
        message_id: &str,
        session_id: &str,
        date: &str,
        time: &str,
        total_tokens: i64,
    ) -> Value {
        json!({
            "messageId": message_id,
            "sessionId": session_id,
            "date": date,
            "time": time,
            "inputTokens": total_tokens,
            "outputTokens": 0,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 0,
            "totalTokens": total_tokens,
            "cost": 0.0
        })
    }

    fn init_git_repository(path: &Path) {
        fs::create_dir_all(path).expect("create repository");
        let status = Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(path)
            .status()
            .expect("run git init");
        assert!(status.success());
    }

    fn init_sync_remote(root: &Path) -> (PathBuf, PathBuf) {
        let remote = root.join("remote.git");
        run_git_test(
            root,
            &["init", "--bare", remote.to_str().expect("remote path")],
        );

        let seed = root.join("seed");
        run_git_test(
            root,
            &[
                "clone",
                remote.to_str().expect("remote path"),
                seed.to_str().expect("seed path"),
            ],
        );
        fs::write(seed.join("README.md"), "token usage sync\n").expect("write seed");
        run_git_test(&seed, &["add", "README.md"]);
        run_git_test(
            &seed,
            &[
                "-c",
                "user.name=Token Usage Sync Test",
                "-c",
                "user.email=sync-test@localhost",
                "commit",
                "-m",
                "Initialize sync repository",
            ],
        );
        run_git_test(&seed, &["push", "--set-upstream", "origin", "HEAD"]);

        let first = root.join("device-a-repo");
        let second = root.join("device-b-repo");
        run_git_test(
            root,
            &[
                "clone",
                remote.to_str().expect("remote path"),
                first.to_str().expect("first repository path"),
            ],
        );
        run_git_test(
            root,
            &[
                "clone",
                remote.to_str().expect("remote path"),
                second.to_str().expect("second repository path"),
            ],
        );
        (first, second)
    }

    fn run_git_test(repository: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "token-usage-sync-{label}-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
