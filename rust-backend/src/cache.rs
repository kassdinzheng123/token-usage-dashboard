use crate::protocol::{HealthResponse, WarmStatus};
use chrono::{SecondsFormat, Utc};
use serde_json::Value;
use std::{
    cmp,
    collections::{BTreeSet, HashMap},
    sync::Arc,
};
use tokio::sync::RwLock;

#[derive(Debug, Default)]
pub struct UsageCache {
    data: RwLock<HashMap<String, Arc<Vec<Value>>>>,
    errors: RwLock<HashMap<String, String>>,
    warm: RwLock<WarmStatus>,
}

impl UsageCache {
    pub async fn has(&self, key: &str) -> bool {
        self.data.read().await.contains_key(key)
    }

    pub async fn get(&self, key: &str) -> Arc<Vec<Value>> {
        self.data.read().await.get(key).cloned().unwrap_or_default()
    }

    pub async fn store_success(&self, key: &str, data: Vec<Value>) {
        self.data
            .write()
            .await
            .insert(key.to_owned(), Arc::new(data));
        self.errors.write().await.remove(key);
    }

    pub async fn replace_rows_by_string_field(
        &self,
        key: &str,
        field: &str,
        values: &BTreeSet<String>,
        updates: Vec<Value>,
    ) {
        let mut data = self.data.write().await;
        let rows = Arc::make_mut(data.entry(key.to_owned()).or_default());
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
        self.errors.write().await.remove(key);
    }

    pub async fn store_error(&self, key: &str, message: String) {
        self.errors.write().await.insert(key.to_owned(), message);
        let mut data = self.data.write().await;
        data.entry(key.to_owned()).or_default();
    }

    pub async fn health(&self, expected: usize) -> HealthResponse {
        let data = self.data.read().await;
        let mut keys = data.keys().cloned().collect::<Vec<_>>();
        keys.sort();

        let mut warm = self.warm.read().await.clone();
        warm.completed = cmp::min(keys.len(), expected);

        HealthResponse {
            status: "ok",
            cached: keys.len(),
            expected,
            keys,
            errors: self.errors.read().await.clone(),
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

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
