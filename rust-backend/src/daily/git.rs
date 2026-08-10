use crate::sync::git_repository_root;
use chrono::{DateTime, Days, Local, NaiveDate};
use std::{
    path::Path,
    process::Command,
};

/// One commit of the target day (author date in local time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    /// Local HH:MM of the author date.
    pub time: String,
    pub subject: String,
}

/// Commits authored on `date` in the repository containing `path`. The git
/// window is widened beyond the day because `--since/--until` filter on the
/// committer date: a commit made the next morning for yesterday's work still
/// has yesterday's author date and must be included. Only `--since` is
/// bounded — the committer date of a relevant commit always follows its
/// author date, but `--until` would drop commits whose committer date lands
/// after the day. Results are filtered by author date in the local timezone.
pub fn commits_for_day(repo: &Path, date: &NaiveDate) -> Result<Vec<CommitInfo>, String> {
    let root = git_repository_root(repo)?;
    let since = date
        .checked_sub_days(Days::new(7))
        .unwrap_or(*date)
        .format("%Y-%m-%d")
        .to_string();

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("log")
        .arg(format!("--since={since} 00:00:00"))
        .arg("--no-show-signature")
        .arg("--pretty=format:%h%x1f%an%x1f%aI%x1f%s")
        .output()
        .map_err(|err| format!("failed to run git log: {err}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if detail.is_empty() {
            "git log failed".to_string()
        } else {
            format!("git log failed: {detail}")
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(4, '\u{1f}');
        let (Some(hash), Some(author), Some(author_time), Some(subject)) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        ) else {
            continue;
        };
        let Ok(parsed) = DateTime::parse_from_rfc3339(author_time) else {
            continue;
        };
        let local = parsed.with_timezone(&Local);
        if local.date_naive() != *date {
            continue;
        }
        commits.push(CommitInfo {
            hash: hash.to_string(),
            author: author.to_string(),
            time: local.format("%H:%M").to_string(),
            subject: subject.to_string(),
        });
    }
    Ok(commits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn init_repo() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let repo = std::env::temp_dir().join(format!("token-usage-daily-git-{stamp}"));
        fs::create_dir_all(&repo).unwrap();
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .arg("init")
            .arg("-q")
            .output()
            .unwrap();
        repo
    }

    fn commit(repo: &Path, file: &str, message: &str, author_date: &str) {
        fs::write(repo.join(file), message).unwrap();
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("add")
            .arg(".")
            .output()
            .unwrap();
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("-c")
            .arg("user.name=test")
            .arg("-c")
            .arg("user.email=test@example.com")
            .arg("commit")
            .arg("-q")
            .arg("-m")
            .arg(message)
            .env("GIT_AUTHOR_DATE", author_date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn filters_commits_by_author_date_in_local_time() {
        let _guard = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let repo = init_repo();
        // Author dates in +08:00. The committer date is "now", which proves
        // the widened window + author-date filter works.
        commit(&repo, "a.txt", "feature a", "2026-08-05T10:00:00+08:00");
        commit(&repo, "b.txt", "feature b", "2026-08-05T22:30:00+08:00");
        commit(&repo, "c.txt", "other day", "2026-08-03T10:00:00+08:00");

        let date = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let commits = commits_for_day(&repo, &date).unwrap();

        assert_eq!(commits.len(), 2);
        assert!(commits.iter().all(|commit| commit.subject != "other day"));
        assert_eq!(
            commits
                .iter()
                .map(|commit| commit.subject.as_str())
                .collect::<Vec<_>>(),
            vec!["feature b", "feature a"]
        );
        assert!(commits.iter().all(|commit| commit.hash.len() == 7));

        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn rejects_non_repository_paths() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("token-usage-daily-git-norepo-{stamp}"));
        fs::create_dir_all(&dir).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 8, 5).unwrap();
        let err = commits_for_day(&dir, &date).unwrap_err();
        assert!(err.contains("not a git repository"), "unexpected error: {err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
