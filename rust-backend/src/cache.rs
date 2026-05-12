use crate::protocol::{HealthResponse, WarmStatus};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::{
    cmp,
    collections::{BTreeSet, HashMap},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(30 * 60);
const DEFAULT_CACHE_MAX_ENTRIES: usize = 500;

#[derive(Debug)]
pub struct UsageCache {
    data: RwLock<HashMap<String, CacheEntry>>,
    errors: RwLock<HashMap<String, ErrorEntry>>,
    warm: RwLock<WarmStatus>,
    ttl: Duration,
    max_entries: usize,
}

#[derive(Debug)]
struct CacheEntry {
    rows: Arc<Vec<Value>>,
    stored_at: Instant,
    last_accessed: Instant,
}

#[derive(Debug)]
struct ErrorEntry {
    message: String,
    stored_at: Instant,
    last_accessed: Instant,
}

impl ErrorEntry {
    fn new(message: String, now: Instant) -> Self {
        Self {
            message,
            stored_at: now,
            last_accessed: now,
        }
    }

    fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        now.duration_since(self.stored_at) >= ttl
    }
}

impl CacheEntry {
    fn new(rows: Vec<Value>, now: Instant) -> Self {
        Self {
            rows: Arc::new(rows),
            stored_at: now,
            last_accessed: now,
        }
    }

    fn is_expired(&self, now: Instant, ttl: Duration) -> bool {
        now.duration_since(self.stored_at) >= ttl
    }
}

impl Default for UsageCache {
    fn default() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            errors: RwLock::new(HashMap::new()),
            warm: RwLock::new(WarmStatus::default()),
            ttl: DEFAULT_CACHE_TTL,
            max_entries: DEFAULT_CACHE_MAX_ENTRIES,
        }
    }
}

impl UsageCache {
    pub async fn has(&self, key: &str) -> bool {
        let mut data = self.data.write().await;
        prune_expired(&mut data, self.ttl);
        data.get_mut(key).is_some_and(|entry| {
            entry.last_accessed = Instant::now();
            true
        })
    }

    pub async fn get(&self, key: &str) -> Arc<Vec<Value>> {
        let mut data = self.data.write().await;
        prune_expired(&mut data, self.ttl);
        data.get_mut(key)
            .map(|entry| {
                entry.last_accessed = Instant::now();
                Arc::clone(&entry.rows)
            })
            .unwrap_or_default()
    }

    pub async fn store_success(&self, key: &str, data: Vec<Value>) {
        let mut cached = self.data.write().await;
        cached.insert(key.to_owned(), CacheEntry::new(data, Instant::now()));
        enforce_limits(&mut cached, self.ttl, self.max_entries);
        drop(cached);
        let mut errors = self.errors.write().await;
        errors.remove(key);
        enforce_error_limits(&mut errors, self.ttl, self.max_entries);
    }

    pub async fn replace_rows_by_string_field(
        &self,
        key: &str,
        field: &str,
        values: &BTreeSet<String>,
        updates: Vec<Value>,
    ) {
        let mut data = self.data.write().await;
        prune_expired(&mut data, self.ttl);
        let rows = Arc::make_mut(
            &mut data
                .entry(key.to_owned())
                .or_insert_with(|| CacheEntry::new(Vec::new(), Instant::now()))
                .rows,
        );
        rows.retain(|row| {
            !row.get(field)
                .and_then(Value::as_str)
                .is_some_and(|value| values.contains(value))
        });
        rows.extend(updates);
        rows.sort_by(|left, right| {
            left.get(field)
                .and_then(Value::as_str)
                .cmp(&right.get(field).and_then(Value::as_str))
        });
        if let Some(entry) = data.get_mut(key) {
            let now = Instant::now();
            entry.stored_at = now;
            entry.last_accessed = now;
        }
        enforce_limits(&mut data, self.ttl, self.max_entries);
        drop(data);
        let mut errors = self.errors.write().await;
        errors.remove(key);
        enforce_error_limits(&mut errors, self.ttl, self.max_entries);
    }

    pub async fn store_error(&self, key: &str, message: String) {
        let mut errors = self.errors.write().await;
        errors.insert(key.to_owned(), ErrorEntry::new(message, Instant::now()));
        enforce_error_limits(&mut errors, self.ttl, self.max_entries);
        drop(errors);
        let mut data = self.data.write().await;
        data.entry(key.to_owned())
            .or_insert_with(|| CacheEntry::new(Vec::new(), Instant::now()));
        enforce_limits(&mut data, self.ttl, self.max_entries);
    }

    pub async fn health(&self, expected: usize) -> HealthResponse {
        let mut data = self.data.write().await;
        prune_expired(&mut data, self.ttl);
        let mut keys = data.keys().cloned().collect::<Vec<_>>();
        keys.sort();

        let mut warm = self.warm.read().await.clone();
        warm.completed = cmp::min(keys.len(), expected);

        let mut errors = self.errors.write().await;
        prune_expired_errors(&mut errors, self.ttl);
        let error_messages = errors
            .iter_mut()
            .map(|(key, entry)| {
                entry.last_accessed = Instant::now();
                (key.clone(), entry.message.clone())
            })
            .collect();

        HealthResponse {
            status: "ok",
            cached: keys.len(),
            expected,
            keys,
            errors: error_messages,
            warm,
        }
    }

    pub async fn is_warming(&self) -> bool {
        self.warm.read().await.warming
    }

    pub async fn begin_warm(&self, total: usize) {
        let mut warm = self.warm.write().await;
        warm.warming = true;
        warm.total = total;
        warm.completed = 0;
        warm.current_key = None;
        warm.current_label = None;
        warm.started_at = Some(now_iso());
        warm.finished_at = None;
    }

    pub async fn set_current(&self, key: Option<String>, label: Option<String>) {
        let mut warm = self.warm.write().await;
        warm.current_key = key;
        warm.current_label = label;
    }

    pub async fn increment_completed(&self, amount: usize) {
        let mut warm = self.warm.write().await;
        let total = if warm.total == 0 {
            usize::MAX
        } else {
            warm.total
        };
        warm.completed = cmp::min(warm.completed + amount, total);
    }

    pub async fn finish_warm(&self) {
        let mut warm = self.warm.write().await;
        warm.warming = false;
        warm.current_key = None;
        warm.current_label = None;
        warm.finished_at = Some(now_iso());
    }
}

fn prune_expired(data: &mut HashMap<String, CacheEntry>, ttl: Duration) {
    let now = Instant::now();
    data.retain(|_, entry| !entry.is_expired(now, ttl));
}

fn enforce_limits(data: &mut HashMap<String, CacheEntry>, ttl: Duration, max_entries: usize) {
    prune_expired(data, ttl);
    while data.len() > max_entries {
        let Some(key) = data
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        data.remove(&key);
    }
}

fn prune_expired_errors(errors: &mut HashMap<String, ErrorEntry>, ttl: Duration) {
    let now = Instant::now();
    errors.retain(|_, entry| !entry.is_expired(now, ttl));
}

fn enforce_error_limits(
    errors: &mut HashMap<String, ErrorEntry>,
    ttl: Duration,
    max_entries: usize,
) {
    prune_expired_errors(errors, ttl);
    while errors.len() > max_entries {
        let Some(key) = errors
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        errors.remove(&key);
    }
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
