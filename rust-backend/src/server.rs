use crate::{
    brief,
    cache::UsageCache,
    ledger::UsageLedger,
    protocol::{
        BriefGenerateRequest, HourlyResponse, HourlyRow, RefreshResponse, Source, TodayModelRow,
        TodayResponse, TodaySourceRow, View, ALL_TASKS,
    },
    sources, sync,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{Duration, Local, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    future::Future,
    net::SocketAddr,
    path::PathBuf,
    pin::Pin,
    str::FromStr,
    sync::Arc,
};
use tokio::sync::{Mutex, Semaphore};
use tower_http::compression::CompressionLayer;
use tracing::{error, info, warn};

pub type ProviderResult = Result<Vec<Value>, String>;
pub type ProviderFuture<'a> = Pin<Box<dyn Future<Output = ProviderResult> + Send + 'a>>;
const TODAY_CACHE_MAX_ENTRIES: usize = Source::ALL.len() * 8;
const REFRESH_CONCURRENCY: usize = 4;

pub trait SourceProvider: Send + Sync {
    fn load<'a>(&'a self, view: View, refresh: bool) -> ProviderFuture<'a>;

    fn has_fast_today(&self) -> bool {
        false
    }

    fn load_today_daily<'a>(&'a self, date: &'a str, refresh: bool) -> ProviderFuture<'a> {
        Box::pin(async move {
            let rows = self.load(View::Daily, refresh).await?;
            Ok(rows
                .into_iter()
                .filter(|row| row.get("date").and_then(Value::as_str) == Some(date))
                .collect())
        })
    }
}

#[derive(Debug, Default)]
pub struct EmptyProvider;

impl SourceProvider for EmptyProvider {
    fn load<'a>(&'a self, _view: View, _refresh: bool) -> ProviderFuture<'a> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

#[derive(Debug, Clone)]
pub struct LocalSourceProvider {
    source: Source,
    ledger_path: Option<PathBuf>,
}

impl LocalSourceProvider {
    pub fn new(source: Source) -> Self {
        Self {
            source,
            ledger_path: None,
        }
    }
}

impl SourceProvider for LocalSourceProvider {
    fn has_fast_today(&self) -> bool {
        matches!(
            self.source,
            Source::Claude | Source::Codex | Source::Opencode
        )
    }

    fn load<'a>(&'a self, view: View, refresh: bool) -> ProviderFuture<'a> {
        let source = self.source;
        let ledger_path = self.ledger_path.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                let ledger = open_usage_ledger(ledger_path)?;
                maybe_ingest_source_into_ledger(&ledger, source, refresh)?;
                ledger.load_view(source, view)
            })
            .await
            .map_err(|err| err.to_string())?
        })
    }

    fn load_today_daily<'a>(&'a self, date: &'a str, refresh: bool) -> ProviderFuture<'a> {
        let source = self.source;
        let ledger_path = self.ledger_path.clone();
        let date = date.to_owned();
        Box::pin(async move {
            let rows = tokio::task::spawn_blocking(move || {
                let ledger = open_usage_ledger(ledger_path)?;
                maybe_ingest_source_into_ledger(&ledger, source, refresh)?;
                Ok::<Vec<Value>, String>(
                    ledger
                        .load_view(source, View::Daily)?
                        .into_iter()
                        .filter(|row| {
                            row.get("date").and_then(Value::as_str) == Some(date.as_str())
                        })
                        .collect(),
                )
            })
            .await
            .map_err(|err| err.to_string())??;
            Ok(rows)
        })
    }
}

fn open_usage_ledger(path: Option<PathBuf>) -> Result<UsageLedger, String> {
    match path {
        Some(path) => UsageLedger::new(path),
        None => UsageLedger::default(),
    }
}

fn maybe_ingest_source_into_ledger(
    ledger: &UsageLedger,
    source: Source,
    refresh: bool,
) -> Result<(), String> {
    let has_rows = ledger.has_source_rows(source).unwrap_or(false);
    if !refresh && has_rows {
        return Ok(());
    }

    if let Err(err) = ingest_source_into_ledger(ledger, source) {
        if has_rows {
            warn!(
                source = %source,
                error = %err,
                "ledger ingest failed; serving persisted rows"
            );
            return Ok(());
        }
        return Err(err);
    }
    Ok(())
}

fn ingest_source_into_ledger(ledger: &UsageLedger, source: Source) -> Result<(), String> {
    let source_name = source.to_string();
    // Watermark the scan by its START: files written during the scan have
    // mtimes after the watermark and are re-read next time (upserts are
    // idempotent, so overlap is safe).
    let scan_started_at_ms = now_epoch_millis();
    let watermark_ms = ledger.ingest_watermark(source).unwrap_or(None);

    let sessions =
        sources::load_source_view_since(&source_name, "sessions", true, watermark_ms)
            .map_err(|err| err.to_string())?;
    ledger.ingest_live_sessions(source, &sessions)?;

    if source == Source::Claude || source == Source::Codex {
        let blocks = sources::load_source_view_since(&source_name, "blocks", true, watermark_ms)
            .map_err(|err| err.to_string())?;
        ledger.ingest_live_blocks(source, &blocks)?;
    }

    // Message-level rows power hourly aggregation; sources without them emit
    // an empty view and keep session-level hourly attribution.
    let messages =
        sources::load_source_view_since(&source_name, "messages", true, watermark_ms)
            .map_err(|err| err.to_string())?;
    ledger.ingest_live_messages(source, &messages)?;

    ledger.record_ingest_watermark(source, scan_started_at_ms)?;
    Ok(())
}

fn now_epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

#[derive(Clone)]
pub struct AppState {
    cache: Arc<UsageCache>,
    today_cache: Arc<Mutex<HashMap<(Source, String), Arc<Vec<Value>>>>>,
    providers: Arc<HashMap<Source, Arc<dyn SourceProvider>>>,
    refresh_locks: Arc<Mutex<HashMap<&'static str, Arc<Mutex<()>>>>>,
    sync_lock: Arc<Mutex<()>>,
    ledger_path: Option<PathBuf>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let empty = Arc::new(EmptyProvider) as Arc<dyn SourceProvider>;
        let mut providers: HashMap<Source, Arc<dyn SourceProvider>> = Source::ALL
            .into_iter()
            .map(|source| (source, Arc::clone(&empty)))
            .collect();
        for source in Source::ALL {
            providers.insert(
                source,
                Arc::new(LocalSourceProvider::new(source)) as Arc<dyn SourceProvider>,
            );
        }

        Self {
            cache: Arc::new(UsageCache::default()),
            today_cache: Arc::new(Mutex::new(HashMap::new())),
            providers: Arc::new(providers),
            refresh_locks: Arc::new(Mutex::new(HashMap::new())),
            sync_lock: Arc::new(Mutex::new(())),
            ledger_path: None,
        }
    }

    pub fn with_provider<P>(self, source: Source, provider: P) -> Self
    where
        P: SourceProvider + 'static,
    {
        let mut providers = (*self.providers).clone();
        providers.insert(source, Arc::new(provider));
        Self {
            cache: self.cache,
            today_cache: self.today_cache,
            providers: Arc::new(providers),
            refresh_locks: self.refresh_locks,
            sync_lock: self.sync_lock,
            ledger_path: self.ledger_path,
        }
    }

    pub fn with_ledger_path(mut self, path: PathBuf) -> Self {
        self.ledger_path = Some(path);
        self
    }

    pub async fn refresh_all(&self, visible: bool) {
        self.today_cache.lock().await.clear();

        if visible {
            self.cache.begin_warm(Source::ALL.len()).await;
        }

        // Fill missing warm keys without force-reingesting every source/view.
        // Explicit per-request `refresh=true` still forces a fresh ingest.
        self.refresh_tasks_concurrently(
            ALL_TASKS
                .into_iter()
                .map(|task| (task.key, task.source, task.view))
                .collect(),
            false,
            visible,
            visible,
        )
        .await;

        if visible {
            self.cache.finish_warm().await;
        }
    }

    pub async fn refresh_startup(&self) {
        let daily_tasks = Source::ALL.len();
        self.cache.begin_warm(daily_tasks).await;

        self.refresh_tasks_concurrently(
            Source::ALL
                .into_iter()
                .filter_map(|source| {
                    task_key(source, View::Daily).map(|key| (key, source, View::Daily))
                })
                .collect(),
            false,
            true,
            true,
        )
        .await;

        self.cache.finish_warm().await;
    }

    async fn refresh_tasks_concurrently(
        &self,
        tasks: Vec<(&'static str, Source, View)>,
        force: bool,
        count_progress: bool,
        update_current: bool,
    ) {
        let semaphore = Arc::new(Semaphore::new(REFRESH_CONCURRENCY));
        let mut handles = Vec::with_capacity(tasks.len());

        for (key, source, view) in tasks {
            let state = self.clone();
            let semaphore = Arc::clone(&semaphore);
            handles.push(tokio::spawn(async move {
                let Ok(_permit) = semaphore.acquire_owned().await else {
                    return;
                };
                state
                    .refresh_with_lock(key, source, view, force, count_progress, update_current)
                    .await;
            }));
        }

        for handle in handles {
            if let Err(err) = handle.await {
                error!(error = %err, "refresh task failed");
            }
        }
    }

    async fn refresh_single(
        &self,
        source: Source,
        view: View,
        force: bool,
        visible: bool,
        count_progress: bool,
    ) {
        let Some(key) = task_key(source, view) else {
            return;
        };
        if !force && self.cache.has(key).await {
            return;
        }

        let was_warming = visible && self.cache.is_warming().await;
        if visible && !was_warming {
            self.cache.begin_warm(1).await;
        }

        self.refresh_with_lock(
            key,
            source,
            view,
            force,
            visible && count_progress && !was_warming,
            visible && !was_warming,
        )
        .await;

        if visible && !was_warming {
            self.cache.finish_warm().await;
        }
    }

    async fn refresh_daily_range(
        &self,
        source: Source,
        since: Option<&str>,
        until: Option<&str>,
    ) -> bool {
        let Some(key) = task_key(source, View::Daily) else {
            return false;
        };
        if !self.cache.has(key).await {
            return false;
        }

        let provider = self
            .providers
            .get(&source)
            .cloned()
            .unwrap_or_else(|| Arc::new(EmptyProvider) as Arc<dyn SourceProvider>);
        if !provider.has_fast_today() {
            return false;
        }

        let Some(dates) = query_date_range(since, until) else {
            return false;
        };
        if dates.is_empty() || dates.len() > 45 {
            return false;
        }

        let refresh_lock = self.refresh_lock(key).await;
        let _guard = refresh_lock.lock().await;
        if !self.cache.has(key).await {
            return false;
        }

        let mut refreshed_dates = BTreeSet::new();
        let mut updates = Vec::new();
        let mut ingest = true;
        for date in dates {
            // Ingest at most once for the range; later dates read ledger/cache.
            match provider.load_today_daily(&date, ingest).await {
                Ok(mut rows) => {
                    refreshed_dates.insert(date);
                    updates.append(&mut rows);
                }
                Err(message) => {
                    error!(source = %source, date, error = %message, "failed to incrementally load daily usage");
                    return true;
                }
            }
            ingest = false;
        }

        if !refreshed_dates.is_empty() {
            {
                let mut today_cache = self.today_cache.lock().await;
                for date in &refreshed_dates {
                    today_cache.remove(&(source, date.clone()));
                }
            }
            self.cache
                .replace_rows_by_string_field(key, "date", &refreshed_dates, updates)
                .await;
        }

        true
    }

    async fn refresh_with_lock(
        &self,
        key: &'static str,
        source: Source,
        view: View,
        force: bool,
        count_progress: bool,
        update_current: bool,
    ) {
        if !force && self.cache.has(key).await {
            return;
        }

        let refresh_lock = self.refresh_lock(key).await;
        let _guard = refresh_lock.lock().await;
        if !force && self.cache.has(key).await {
            return;
        }

        self.refresh_task(key, source, view, force, count_progress, update_current)
            .await;
    }

    async fn refresh_lock(&self, key: &'static str) -> Arc<Mutex<()>> {
        let mut locks = self.refresh_locks.lock().await;
        Arc::clone(locks.entry(key).or_insert_with(|| Arc::new(Mutex::new(()))))
    }

    async fn refresh_task(
        &self,
        key: &'static str,
        source: Source,
        view: View,
        force: bool,
        count_progress: bool,
        update_current: bool,
    ) {
        if update_current {
            self.cache
                .set_current(
                    Some(key.to_owned()),
                    Some(format!("{} {}", source.label(), view.label())),
                )
                .await;
        }

        let provider = self
            .providers
            .get(&source)
            .cloned()
            .unwrap_or_else(|| Arc::new(EmptyProvider) as Arc<dyn SourceProvider>);

        match provider.load(view, force).await {
            Ok(data) => {
                info!(key, entries = data.len(), "loaded usage view");
                self.cache.store_success(key, data).await;
            }
            Err(message) => {
                error!(key, error = %message, "failed to load usage view");
                self.cache.store_error(key, message).await;
            }
        }

        if count_progress {
            self.cache.increment_completed(1).await;
        }
    }

    async fn today_rows_for_source(
        &self,
        source: Source,
        date: &str,
        force: bool,
    ) -> Arc<Vec<Value>> {
        if let Some(key) = task_key(source, View::Daily) {
            if !force && self.cache.has(key).await {
                return self.cache.get(key).await;
            }
        }

        let provider = self
            .providers
            .get(&source)
            .cloned()
            .unwrap_or_else(|| Arc::new(EmptyProvider) as Arc<dyn SourceProvider>);

        if !provider.has_fast_today() {
            self.refresh_single(source, View::Daily, force, false, false)
                .await;

            let Some(key) = task_key(source, View::Daily) else {
                return Arc::new(Vec::new());
            };
            return self.cache.get(key).await;
        }

        let today_cache_key = (source, date.to_owned());
        if !force {
            if let Some(rows) = self.today_cache.lock().await.get(&today_cache_key).cloned() {
                return rows;
            }
        }

        let Some(today_lock_key) = today_task_key(source) else {
            return Arc::new(Vec::new());
        };
        let refresh_lock = self.refresh_lock(today_lock_key).await;
        let _guard = refresh_lock.lock().await;

        if let Some(key) = task_key(source, View::Daily) {
            if !force && self.cache.has(key).await {
                return self.cache.get(key).await;
            }
        }
        if !force {
            if let Some(rows) = self.today_cache.lock().await.get(&today_cache_key).cloned() {
                return rows;
            }
        }

        match provider.load_today_daily(date, force).await {
            Ok(data) => {
                let data = Arc::new(data);
                let mut today_cache = self.today_cache.lock().await;
                today_cache.insert(today_cache_key, Arc::clone(&data));
                enforce_today_cache_limit(&mut today_cache);
                data
            }
            Err(message) => {
                error!(source = %source, error = %message, "failed to load today usage");
                Arc::new(Vec::new())
            }
        }
    }
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health_handler))
        .route("/api/refresh", get(refresh_handler))
        .route("/api/sync", axum::routing::post(sync_handler))
        .route("/api/today", get(today_handler))
        .route("/api/hourly", get(hourly_handler))
        .route(
            "/api/today/brief",
            get(today_brief_get_handler).post(today_brief_post_handler),
        )
        .route("/api/brief/days", get(brief_days_handler))
        .route("/api/brief/months", get(brief_months_handler))
        .route("/api/brief/{date}", get(brief_date_get_handler))
        .route("/api/{source}/{view}", get(source_view_handler))
        .layer(CompressionLayer::new())
        .with_state(state)
}

pub async fn serve_from_env() -> std::io::Result<()> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(3456);
    serve(SocketAddr::from(([127, 0, 0, 1], port))).await
}

pub async fn serve(addr: SocketAddr) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    info!("Server running on http://{}", local_addr);

    let state = AppState::new();
    let warm_state = state.clone();
    tokio::spawn(async move {
        warm_state.refresh_startup().await;
    });

    axum::serve(listener, create_app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn health_handler(State(state): State<AppState>) -> Json<crate::protocol::HealthResponse> {
    // Startup only warms daily views; keep expected aligned so clients don't
    // trigger a full ALL_TASKS force warm when daily cache is already hot.
    Json(state.cache.health(Source::ALL.len()).await)
}

async fn refresh_handler(State(state): State<AppState>) -> Json<RefreshResponse> {
    tokio::spawn(async move {
        state.refresh_all(false).await;
    });
    Json(RefreshResponse {
        status: "refreshing",
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyncRequest {
    repository: String,
    device_id: String,
}

async fn sync_handler(State(state): State<AppState>, Json(request): Json<SyncRequest>) -> Response {
    let repository = request.repository.trim();
    let device_id = request.device_id.trim();
    if repository.is_empty() || device_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "status": "error",
                "error": "repository and deviceId are required",
            })),
        )
            .into_response();
    }

    let _guard = state.sync_lock.lock().await;
    let ledger_path = state.ledger_path.clone();
    let repository = PathBuf::from(repository);
    let device_id = device_id.to_string();
    match tokio::task::spawn_blocking(move || {
        let ledger = open_usage_ledger(ledger_path)?;
        sync::sync_with_git(&ledger, &repository, &device_id)
    })
    .await
    {
        Ok(Ok(result)) => {
            state.cache.clear().await;
            state.today_cache.lock().await.clear();
            (StatusCode::OK, Json(result)).into_response()
        }
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn today_handler(
    State(state): State<AppState>,
    Query(query): Query<DataQuery>,
) -> Json<TodayResponse> {
    let force = query.refresh.as_deref() == Some("true");
    Json(state.today_summary(force).await)
}

#[derive(Debug, Deserialize)]
struct HourlyQuery {
    date: Option<String>,
    refresh: Option<String>,
}

async fn hourly_handler(
    State(state): State<AppState>,
    Query(query): Query<HourlyQuery>,
) -> Json<HourlyResponse> {
    let force = query.refresh.as_deref() == Some("true");
    let date = query
        .date
        .filter(|date| !date.is_empty())
        .unwrap_or_else(|| Local::now().format("%Y-%m-%d").to_string());
    Json(state.hourly_usage(&date, force).await)
}

async fn today_brief_get_handler() -> Response {
    match tokio::task::spawn_blocking(brief::load_today_brief).await {
        Ok(Ok(Some(brief))) => (StatusCode::OK, Json(brief)).into_response(),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "status": "missing",
                "date": brief::local_today(),
            })),
        )
            .into_response(),
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn today_brief_post_handler(Json(request): Json<BriefGenerateRequest>) -> Response {
    match tokio::task::spawn_blocking(move || brief::generate_today_brief(request)).await {
        Ok(Ok(brief)) => (StatusCode::OK, Json(brief)).into_response(),
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn brief_date_get_handler(Path(date): Path<String>) -> Response {
    match tokio::task::spawn_blocking(move || brief::load_brief_for_date(&date)).await {
        Ok(Ok(Some(brief))) => (StatusCode::OK, Json(brief)).into_response(),
        Ok(Ok(None)) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "status": "missing" })),
        )
            .into_response(),
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct BriefDaysQuery {
    month: Option<String>,
}

async fn brief_days_handler(Query(query): Query<BriefDaysQuery>) -> Response {
    let month = query
        .month
        .filter(|month| !month.is_empty())
        .unwrap_or_else(|| Local::now().format("%Y-%m").to_string());
    match tokio::task::spawn_blocking(move || brief::month_days(&month)).await {
        Ok(Ok(days)) => (StatusCode::OK, Json(json!({ "days": days }))).into_response(),
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn brief_months_handler() -> Response {
    match tokio::task::spawn_blocking(brief::all_months).await {
        Ok(Ok(months)) => (StatusCode::OK, Json(json!({ "months": months }))).into_response(),
        Ok(Err(message)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": message })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "status": "error", "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct DataQuery {
    since: Option<String>,
    until: Option<String>,
    refresh: Option<String>,
}

async fn source_view_handler(
    State(state): State<AppState>,
    Path((source, view)): Path<(String, String)>,
    Query(query): Query<DataQuery>,
) -> Response {
    let Ok(source) = Source::from_str(&source) else {
        return not_found("unknown source");
    };
    let Ok(view) = View::from_str(&view) else {
        return not_found("unknown view");
    };

    if view == View::Blocks && source != Source::Claude {
        return Json(Vec::<Value>::new()).into_response();
    }

    let Some(key) = task_key(source, view) else {
        return not_found("unknown source view");
    };

    let force = query.refresh.as_deref() == Some("true");
    let refreshed_incrementally = force
        && view == View::Daily
        && state
            .refresh_daily_range(source, query.since.as_deref(), query.until.as_deref())
            .await;
    if !refreshed_incrementally {
        state.refresh_single(source, view, force, true, true).await;
    }

    let data = state.cache.get(key).await;
    if query.since.is_none() && query.until.is_none() {
        Json(data.as_ref()).into_response()
    } else {
        Json(filter_by_date_range(
            &data,
            query.since.as_deref(),
            query.until.as_deref(),
        ))
        .into_response()
    }
}

fn not_found(message: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": message }))).into_response()
}

fn enforce_today_cache_limit(cache: &mut HashMap<(Source, String), Arc<Vec<Value>>>) {
    while cache.len() > TODAY_CACHE_MAX_ENTRIES {
        let Some(oldest_key) = cache.keys().min_by_key(|(_, date)| date.as_str()).cloned() else {
            break;
        };
        cache.remove(&oldest_key);
    }
}

fn task_key(source: Source, view: View) -> Option<&'static str> {
    ALL_TASKS
        .iter()
        .find(|task| task.source == source && task.view == view)
        .map(|task| task.key)
}

fn today_task_key(source: Source) -> Option<&'static str> {
    match source {
        Source::Claude => Some("claude:today"),
        Source::Codex => Some("codex:today"),
        Source::Opencode => Some("opencode:today"),
        Source::Hermes => Some("hermes:today"),
        Source::Openclaw => Some("openclaw:today"),
        Source::Pi => Some("pi:today"),
        Source::Grok => Some("grok:today"),
        Source::Cursor => Some("cursor:today"),
        Source::Cherry => Some("cherry:today"),
        Source::ClaudeScience => Some("claude-science:today"),
        Source::Zcode => Some("zcode:today"),
        Source::Kimi => Some("kimi:today"),
    }
}

#[derive(Debug, Default, Clone)]
struct TodayTotals {
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
    total_tokens: i64,
    total_cost: f64,
}

impl TodayTotals {
    fn add_daily_row(&mut self, source: Source, row: &Value) {
        let input_tokens = number_i64(row, "inputTokens");
        let output_tokens = number_i64(row, "outputTokens");
        let cache_creation_tokens = number_i64(row, "cacheCreationTokens");
        let cache_read_tokens = number_i64(row, "cacheReadTokens");

        self.add_usage(
            source,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );
        self.total_cost += number_f64(row, "totalCost");
    }

    fn add_model_breakdown(&mut self, source: Source, row: &Value) {
        let input_tokens = number_i64(row, "inputTokens");
        let output_tokens = number_i64(row, "outputTokens");
        let cache_creation_tokens = number_i64(row, "cacheCreationTokens");
        let cache_read_tokens = number_i64(row, "cacheReadTokens");

        self.add_usage(
            source,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );
        self.total_cost += row
            .get("totalCost")
            .map(|_| number_f64(row, "totalCost"))
            .unwrap_or_else(|| number_f64(row, "cost"));
    }

    fn add_split_daily_row(&mut self, source: Source, row: &Value, count: usize, index: usize) {
        let input_tokens = split_i64(number_i64(row, "inputTokens"), count, index);
        let output_tokens = split_i64(number_i64(row, "outputTokens"), count, index);
        let cache_creation_tokens = split_i64(number_i64(row, "cacheCreationTokens"), count, index);
        let cache_read_tokens = split_i64(number_i64(row, "cacheReadTokens"), count, index);

        self.add_usage(
            source,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );
        self.total_cost += number_f64(row, "totalCost") / count.max(1) as f64;
    }

    fn add_totals(&mut self, other: &Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_tokens += other.cache_creation_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.total_tokens += other.total_tokens;
        self.total_cost += other.total_cost;
    }

    fn total_tokens(&self) -> i64 {
        self.total_tokens
    }

    fn has_usage(&self) -> bool {
        self.total_tokens() > 0 || self.total_cost != 0.0
    }

    fn add_usage(
        &mut self,
        source: Source,
        input_tokens: i64,
        output_tokens: i64,
        cache_creation_tokens: i64,
        cache_read_tokens: i64,
    ) {
        self.input_tokens += input_tokens;
        self.output_tokens += output_tokens;
        self.cache_creation_tokens += cache_creation_tokens;
        self.cache_read_tokens += cache_read_tokens;
        self.total_tokens += source_total_tokens(
            source,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
        );
    }
}

fn source_total_tokens(
    _source: Source,
    input_tokens: i64,
    output_tokens: i64,
    cache_creation_tokens: i64,
    cache_read_tokens: i64,
) -> i64 {
    input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens
}

impl AppState {
    pub async fn today_summary(&self, force: bool) -> TodayResponse {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let mut total = TodayTotals::default();
        let mut source_rows = Vec::new();
        let mut model_rows = Vec::new();
        let mut all_models = BTreeSet::new();
        let mut handles = Vec::new();

        for source in Source::ALL {
            let state = self.clone();
            let today = today.clone();
            handles.push(tokio::spawn(async move {
                let rows = state.today_rows_for_source(source, &today, force).await;
                (source, rows)
            }));
        }

        for handle in handles {
            let Ok((source, rows)) = handle.await else {
                continue;
            };
            let mut source_total = TodayTotals::default();
            let mut source_models = BTreeSet::new();
            let mut model_totals = BTreeMap::<String, TodayTotals>::new();

            for row in rows
                .iter()
                .filter(|row| row.get("date").and_then(Value::as_str) == Some(today.as_str()))
            {
                source_total.add_daily_row(source, row);
                let mut row_models = BTreeSet::new();
                collect_model_names(row, &mut row_models);
                source_models.extend(row_models.iter().cloned());
                collect_model_totals(source, row, &row_models, &mut model_totals);
            }

            if !source_total.has_usage() {
                continue;
            }

            for model_name in model_totals.keys() {
                source_models.insert(model_name.clone());
            }
            all_models.extend(source_models.iter().cloned());

            let source_name = source.to_string();
            total.add_totals(&source_total);
            source_rows.push(TodaySourceRow {
                source: source_name.clone(),
                input_tokens: source_total.input_tokens,
                output_tokens: source_total.output_tokens,
                cache_creation_tokens: source_total.cache_creation_tokens,
                cache_read_tokens: source_total.cache_read_tokens,
                total_tokens: source_total.total_tokens(),
                total_cost: source_total.total_cost,
                model_count: source_models.len(),
            });

            for (model_name, model_total) in model_totals {
                model_rows.push(TodayModelRow {
                    source: source_name.clone(),
                    model_name,
                    input_tokens: model_total.input_tokens,
                    output_tokens: model_total.output_tokens,
                    cache_creation_tokens: model_total.cache_creation_tokens,
                    cache_read_tokens: model_total.cache_read_tokens,
                    total_tokens: model_total.total_tokens(),
                    total_cost: model_total.total_cost,
                });
            }
        }

        TodayResponse {
            date: today,
            input_tokens: total.input_tokens,
            output_tokens: total.output_tokens,
            cache_creation_tokens: total.cache_creation_tokens,
            cache_read_tokens: total.cache_read_tokens,
            total_tokens: total.total_tokens(),
            total_cost: total.total_cost,
            active_source_count: source_rows.len(),
            model_count: all_models.len(),
            source_rows,
            model_rows,
        }
    }

    /// Hourly breakdown for a single day across all sources. Reads the ledger
    /// directly (no warm cache); `force` re-ingests source files first, mirroring
    /// the today summary's refresh semantics.
    pub async fn hourly_usage(&self, date: &str, force: bool) -> HourlyResponse {
        let mut handles = Vec::new();
        for source in Source::ALL {
            let date = date.to_owned();
            handles.push(tokio::spawn(async move {
                let rows = tokio::task::spawn_blocking(move || {
                    let ledger = open_usage_ledger(None)?;
                    maybe_ingest_source_into_ledger(&ledger, source, force)?;
                    ledger.load_hourly(source, &date)
                })
                .await
                .map_err(|err| err.to_string())??;
                Ok::<Vec<Value>, String>(rows)
            }));
        }

        let mut hours = Vec::new();
        for handle in handles {
            let Ok(Ok(rows)) = handle.await else {
                continue;
            };
            for row in rows {
                match serde_json::from_value::<HourlyRow>(row) {
                    Ok(hourly_row) => hours.push(hourly_row),
                    Err(err) => warn!(error = %err, "skipping malformed hourly row"),
                }
            }
        }
        hours.sort_by(|left, right| {
            left.hour
                .cmp(&right.hour)
                .then_with(|| left.source.cmp(&right.source))
        });

        HourlyResponse {
            date: date.to_owned(),
            hours,
        }
    }
}

fn collect_model_names(row: &Value, model_names: &mut BTreeSet<String>) {
    if let Some(models) = row.get("modelsUsed").and_then(Value::as_array) {
        for model in models
            .iter()
            .filter_map(Value::as_str)
            .filter(|model| !model.is_empty())
        {
            model_names.insert(model.to_owned());
        }
    }
}

fn collect_model_totals(
    source: Source,
    row: &Value,
    source_models: &BTreeSet<String>,
    model_totals: &mut BTreeMap<String, TodayTotals>,
) {
    let Some(breakdowns) = row.get("modelBreakdowns").and_then(Value::as_array) else {
        collect_fallback_model_totals(source, row, model_totals);
        return;
    };

    if breakdowns.is_empty() {
        collect_fallback_model_totals(source, row, model_totals);
        return;
    }

    for breakdown in breakdowns {
        let model_name = breakdown
            .get("modelName")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown")
            .to_owned();
        let model_name = if is_codex_lossy_fallback(source, &model_name, source_models) {
            "unknown".to_owned()
        } else {
            model_name
        };
        model_totals
            .entry(model_name)
            .or_default()
            .add_model_breakdown(source, breakdown);
    }
}

fn is_codex_lossy_fallback(
    source: Source,
    model_name: &str,
    source_models: &BTreeSet<String>,
) -> bool {
    source == Source::Codex && model_name == "gpt-5" && !source_models.contains(model_name)
}

fn collect_fallback_model_totals(
    source: Source,
    row: &Value,
    model_totals: &mut BTreeMap<String, TodayTotals>,
) {
    let models = row
        .get("modelsUsed")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(Value::as_str)
                .filter(|model| !model.is_empty())
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .filter(|models| !models.is_empty())
        .unwrap_or_else(|| vec!["unknown".to_owned()]);

    let count = models.len();
    for (index, model_name) in models.into_iter().enumerate() {
        model_totals
            .entry(model_name)
            .or_default()
            .add_split_daily_row(source, row, count, index);
    }
}

fn split_i64(value: i64, count: usize, index: usize) -> i64 {
    let count = i64::try_from(count).unwrap_or(1).max(1);
    let base = value / count;
    let remainder = value % count;
    if i64::try_from(index).is_ok_and(|index| index < remainder) {
        base + 1
    } else {
        base
    }
}

fn number_i64(row: &Value, field: &str) -> i64 {
    row.get(field).map(value_i64).unwrap_or_default()
}

fn value_i64(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
        .or_else(|| value.as_f64().map(|value| value as i64))
        .unwrap_or_default()
}

fn number_f64(row: &Value, field: &str) -> f64 {
    row.get(field)
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_i64().map(|value| value as f64))
                .or_else(|| value.as_u64().map(|value| value as f64))
        })
        .unwrap_or_default()
}

fn filter_by_date_range(data: &[Value], since: Option<&str>, until: Option<&str>) -> Vec<Value> {
    let since = since.map(compact_to_iso);
    let until = until.map(compact_to_iso);

    data.iter()
        .filter(|entry| {
            let Some(raw) = entry
                .get("date")
                .or_else(|| entry.get("month"))
                .and_then(Value::as_str)
            else {
                return true;
            };

            if raw.len() == 7 {
                let month_start = format!("{raw}-01");
                let month_end = month_end(raw).unwrap_or_else(|| month_start.clone());
                if since
                    .as_deref()
                    .is_some_and(|value| month_end.as_str() < value)
                {
                    return false;
                }
                if until
                    .as_deref()
                    .is_some_and(|value| month_start.as_str() > value)
                {
                    return false;
                }
                return true;
            }

            if since.as_deref().is_some_and(|value| raw < value) {
                return false;
            }
            if until.as_deref().is_some_and(|value| raw > value) {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

fn query_date_range(since: Option<&str>, until: Option<&str>) -> Option<Vec<String>> {
    let since = since?;
    let until = until.unwrap_or(since);
    let mut date = parse_query_date(since)?;
    let end = parse_query_date(until)?;
    if date > end {
        return Some(Vec::new());
    }

    let mut dates = Vec::new();
    while date <= end {
        dates.push(date.format("%Y-%m-%d").to_string());
        date = date.checked_add_signed(Duration::days(1))?;
    }
    Some(dates)
}

fn parse_query_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(&compact_to_iso(value), "%Y-%m-%d").ok()
}

fn compact_to_iso(value: &str) -> String {
    if value.len() == 8 && value.chars().all(|ch| ch.is_ascii_digit()) {
        format!("{}-{}-{}", &value[0..4], &value[4..6], &value[6..8])
    } else {
        value.to_owned()
    }
}

fn month_end(month: &str) -> Option<String> {
    let (year, month) = month.split_once('-')?;
    let year = year.parse::<i32>().ok()?;
    let month = month.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) {
        return None;
    }

    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => return None,
    };
    Some(format!("{year:04}-{month:02}-{days:02}"))
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use std::{
        fs,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex as StdMutex,
        },
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::Notify;
    use tower::ServiceExt;

    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    #[derive(Debug, Clone)]
    struct StaticProvider {
        data: Vec<Value>,
        calls: Arc<AtomicUsize>,
    }

    impl StaticProvider {
        fn new(data: Vec<Value>) -> Self {
            Self {
                data,
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SourceProvider for StaticProvider {
        fn load<'a>(&'a self, _view: View, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(self.data.clone())
            })
        }
    }

    #[derive(Debug, Clone)]
    struct IncrementalDailyProvider {
        full_data: Vec<Value>,
        daily_data: Arc<std::sync::Mutex<HashMap<String, Vec<Value>>>>,
        full_calls: Arc<AtomicUsize>,
        daily_calls: Arc<AtomicUsize>,
    }

    impl IncrementalDailyProvider {
        fn new(full_data: Vec<Value>, daily_data: HashMap<String, Vec<Value>>) -> Self {
            Self {
                full_data,
                daily_data: Arc::new(std::sync::Mutex::new(daily_data)),
                full_calls: Arc::new(AtomicUsize::new(0)),
                daily_calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn full_calls(&self) -> usize {
            self.full_calls.load(Ordering::SeqCst)
        }

        fn daily_calls(&self) -> usize {
            self.daily_calls.load(Ordering::SeqCst)
        }
    }

    impl SourceProvider for IncrementalDailyProvider {
        fn load<'a>(&'a self, _view: View, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.full_calls.fetch_add(1, Ordering::SeqCst);
                Ok(self.full_data.clone())
            })
        }

        fn has_fast_today(&self) -> bool {
            true
        }

        fn load_today_daily<'a>(&'a self, date: &'a str, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.daily_calls.fetch_add(1, Ordering::SeqCst);
                Ok(self
                    .daily_data
                    .lock()
                    .unwrap()
                    .get(date)
                    .cloned()
                    .unwrap_or_default())
            })
        }
    }

    #[derive(Debug, Clone)]
    struct BlockingProvider {
        data: Vec<Value>,
        calls: Arc<AtomicUsize>,
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    impl BlockingProvider {
        fn new(data: Vec<Value>) -> Self {
            Self {
                data,
                calls: Arc::new(AtomicUsize::new(0)),
                started: Arc::new(Notify::new()),
                release: Arc::new(Notify::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl SourceProvider for BlockingProvider {
        fn load<'a>(&'a self, _view: View, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.notify_one();
                self.release.notified().await;
                Ok(self.data.clone())
            })
        }
    }

    #[derive(Debug, Clone)]
    struct GateProvider {
        calls: Arc<AtomicUsize>,
        open: Arc<AtomicBool>,
        release: Arc<Notify>,
    }

    impl GateProvider {
        fn new() -> Self {
            Self {
                calls: Arc::new(AtomicUsize::new(0)),
                open: Arc::new(AtomicBool::new(false)),
                release: Arc::new(Notify::new()),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }

        fn open(&self) {
            self.open.store(true, Ordering::SeqCst);
            self.release.notify_waiters();
        }
    }

    impl SourceProvider for GateProvider {
        fn load<'a>(&'a self, _view: View, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                loop {
                    let notified = self.release.notified();
                    if self.open.load(Ordering::SeqCst) {
                        break;
                    }
                    notified.await;
                }
                Ok(Vec::new())
            })
        }
    }

    #[derive(Debug, Clone, Default)]
    struct RecordingProvider {
        views: Arc<std::sync::Mutex<Vec<View>>>,
    }

    impl RecordingProvider {
        fn views(&self) -> Vec<View> {
            self.views.lock().unwrap().clone()
        }
    }

    impl SourceProvider for RecordingProvider {
        fn load<'a>(&'a self, view: View, _refresh: bool) -> ProviderFuture<'a> {
            Box::pin(async move {
                self.views.lock().unwrap().push(view);
                Ok(Vec::new())
            })
        }
    }

    fn test_state() -> AppState {
        Source::ALL
            .into_iter()
            .fold(AppState::new(), |state, source| {
                state.with_provider(source, StaticProvider::new(Vec::new()))
            })
    }

    #[tokio::test]
    async fn health_matches_node_shape() {
        let response = create_app(AppState::new())
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(value["status"], "ok");
        assert_eq!(value["cached"], json!(0));
        assert_eq!(value["expected"], json!(Source::ALL.len()));
        assert!(value["keys"].as_array().unwrap().is_empty());
        assert!(value["errors"].as_object().unwrap().is_empty());
        assert_eq!(value["warm"]["warming"], json!(false));
    }

    #[tokio::test]
    async fn sync_requires_repository_and_device_id() {
        let response = create_app(test_state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sync")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"repository":"","deviceId":""}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"], "error");
    }

    #[tokio::test]
    async fn sync_surfaces_git_repository_errors() {
        let root = temp_codex_home().join("sync-api");
        fs::create_dir_all(&root).unwrap();
        let state = test_state().with_ledger_path(root.join("usage-ledger.sqlite"));
        let body = json!({
            "repository": root.to_string_lossy(),
            "deviceId": "test-device",
        })
        .to_string();

        let response = create_app(state)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/sync")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert!(value["error"]
            .as_str()
            .is_some_and(|message| message.contains("not a git repository")));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn non_claude_blocks_return_empty_array() {
        let response = create_app(AppState::new())
            .oneshot(
                Request::builder()
                    .uri("/api/codex/blocks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value, json!([]));
    }

    #[tokio::test]
    async fn json_responses_support_gzip_compression() {
        let response = create_app(AppState::new())
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_ENCODING),
            Some(&header::HeaderValue::from_static("gzip"))
        );
    }

    #[tokio::test]
    async fn invalid_source_returns_not_found() {
        let response = create_app(AppState::new())
            .oneshot(
                Request::builder()
                    .uri("/api/unknown/daily")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn today_returns_aggregated_source_and_model_rows() {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let claude_provider = StaticProvider::new(vec![
            json!({
                "date": today,
                "inputTokens": 100,
                "outputTokens": 40,
                "cacheCreationTokens": 10,
                "cacheReadTokens": 5,
                "totalTokens": 155,
                "totalCost": 0.12,
                "modelsUsed": ["sonnet", "opus"],
                "modelBreakdowns": [
                    {
                        "modelName": "opus",
                        "inputTokens": 70,
                        "outputTokens": 30,
                        "cacheCreationTokens": 10,
                        "cacheReadTokens": 0,
                        "cost": 0.09
                    },
                    {
                        "modelName": "sonnet",
                        "inputTokens": 30,
                        "outputTokens": 10,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 5,
                        "cost": 0.03
                    }
                ]
            }),
            json!({
                "date": "2000-01-01",
                "inputTokens": 999,
                "outputTokens": 999,
                "cacheCreationTokens": 999,
                "cacheReadTokens": 999,
                "totalTokens": 3996,
                "totalCost": 9.99,
                "modelBreakdowns": []
            }),
        ]);
        let codex_provider = StaticProvider::new(vec![json!({
            "date": today,
            "inputTokens": 20,
            "outputTokens": 8,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 2,
            "totalTokens": 28,
            "totalCost": 0.04,
            "modelBreakdowns": [
                {
                    "modelName": "gpt-5-codex",
                    "inputTokens": 20,
                    "outputTokens": 8,
                    "cacheCreationTokens": 0,
                    "cacheReadTokens": 2,
                    "cost": 0.04
                }
            ]
        })]);

        let state = test_state()
            .with_provider(Source::Claude, claude_provider.clone())
            .with_provider(Source::Codex, codex_provider);
        let response = create_app(state)
            .oneshot(
                Request::builder()
                    .uri("/api/today")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(value["date"], Local::now().format("%Y-%m-%d").to_string());
        assert_eq!(value["inputTokens"], json!(120));
        assert_eq!(value["outputTokens"], json!(48));
        assert_eq!(value["cacheCreationTokens"], json!(10));
        assert_eq!(value["cacheReadTokens"], json!(7));
        assert_eq!(value["totalTokens"], json!(185));
        assert_eq!(value["totalCost"], json!(0.16));
        assert_eq!(value["activeSourceCount"], json!(2));
        assert_eq!(value["modelCount"], json!(3));
        assert_eq!(value["sourceRows"][0]["source"], "claude");
        assert_eq!(value["sourceRows"][0]["totalTokens"], json!(155));
        assert_eq!(value["sourceRows"][0]["modelCount"], json!(2));
        assert_eq!(value["sourceRows"][1]["source"], "codex");
        assert_eq!(value["sourceRows"][1]["totalTokens"], json!(30));
        assert_eq!(value["modelRows"][0]["source"], "claude");
        assert_eq!(value["modelRows"][0]["modelName"], "opus");
        assert_eq!(value["modelRows"][2]["source"], "codex");
        assert_eq!(value["modelRows"][2]["modelName"], "gpt-5-codex");
        assert_eq!(value["modelRows"][2]["totalTokens"], json!(30));
        assert_eq!(claude_provider.calls(), 1);
    }

    #[tokio::test]
    async fn today_reuses_cached_daily_rows() {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let provider = StaticProvider::new(vec![json!({
            "date": today,
            "inputTokens": 1,
            "outputTokens": 2,
            "cacheCreationTokens": 3,
            "cacheReadTokens": 4,
            "totalCost": 0.01,
            "modelsUsed": ["fallback-model"],
            "modelBreakdowns": []
        })]);
        let state = test_state().with_provider(Source::Claude, provider.clone());

        let first = state.today_summary(false).await;
        let second = state.today_summary(false).await;

        assert_eq!(first.total_tokens, 10);
        assert_eq!(second.total_tokens, 10);
        assert_eq!(first.model_count, 1);
        assert_eq!(first.model_rows[0].model_name, "fallback-model");
        assert_eq!(first.model_rows[0].total_tokens, 10);
        assert_eq!(provider.calls(), 1);
    }

    #[tokio::test]
    async fn today_remaps_codex_lossy_gpt5_fallback_to_unknown() {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let provider = StaticProvider::new(vec![
            json!({
                "date": today,
                "inputTokens": 10,
                "outputTokens": 2,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 1,
                "totalCost": 0.01,
                "modelsUsed": [],
                "modelBreakdowns": [
                    {
                        "modelName": "gpt-5",
                        "inputTokens": 10,
                        "outputTokens": 2,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 1,
                        "cost": 0.01
                    }
                ]
            }),
            json!({
                "date": today,
                "inputTokens": 3,
                "outputTokens": 1,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 0,
                "totalCost": 0.02,
                "modelsUsed": ["gpt-5"],
                "modelBreakdowns": [
                    {
                        "modelName": "gpt-5",
                        "inputTokens": 3,
                        "outputTokens": 1,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 0,
                        "cost": 0.02
                    }
                ]
            }),
        ]);
        let state = test_state().with_provider(Source::Codex, provider);

        let summary = state.today_summary(false).await;
        let codex_rows = summary
            .model_rows
            .iter()
            .filter(|row| row.source == "codex")
            .collect::<Vec<_>>();

        let unknown = codex_rows
            .iter()
            .find(|row| row.model_name == "unknown")
            .unwrap();
        let legitimate_gpt5 = codex_rows
            .iter()
            .find(|row| row.model_name == "gpt-5")
            .unwrap();

        assert_eq!(unknown.total_tokens, 13);
        assert_eq!(legitimate_gpt5.total_tokens, 4);
    }

    #[tokio::test]
    async fn concurrent_today_reuses_in_flight_daily_refresh() {
        let today = Local::now().format("%Y-%m-%d").to_string();
        let provider = BlockingProvider::new(vec![json!({
            "date": today,
            "inputTokens": 1,
            "outputTokens": 2,
            "cacheCreationTokens": 3,
            "cacheReadTokens": 4,
            "totalCost": 0.01,
            "modelsUsed": ["sonnet"],
            "modelBreakdowns": []
        })]);
        let state = test_state().with_provider(Source::Claude, provider.clone());

        let first_state = state.clone();
        let first = tokio::spawn(async move {
            first_state
                .refresh_single(Source::Claude, View::Daily, false, false, false)
                .await;
        });
        provider.started.notified().await;

        let second_state = state.clone();
        let second = tokio::spawn(async move { second_state.today_summary(false).await });
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        assert_eq!(provider.calls(), 1);
        provider.release.notify_one();

        first.await.unwrap();
        let summary = second.await.unwrap();
        assert_eq!(summary.total_tokens, 10);
        assert_eq!(provider.calls(), 1);

        let cached = state.today_summary(false).await;
        assert_eq!(cached.total_tokens, 10);
        assert_eq!(provider.calls(), 1);
    }

    #[tokio::test]
    async fn startup_warms_daily_views_only() {
        let provider = RecordingProvider::default();
        let state = test_state().with_provider(Source::Codex, provider.clone());

        state.refresh_startup().await;
        let health = state.cache.health(ALL_TASKS.len()).await;

        assert_eq!(provider.views(), vec![View::Daily]);
        assert!(health.keys.contains(&"codex:daily".to_owned()));
        assert!(!health.keys.contains(&"codex:monthly".to_owned()));
        assert!(!health.keys.contains(&"codex:sessions".to_owned()));
    }

    #[tokio::test]
    async fn refresh_all_uses_bounded_concurrency() {
        let provider = GateProvider::new();
        let state = Source::ALL.into_iter().fold(test_state(), |state, source| {
            state.with_provider(source, provider.clone())
        });

        let refresh_state = state.clone();
        let handle = tokio::spawn(async move {
            refresh_state.refresh_all(false).await;
        });

        for _ in 0..100 {
            if provider.calls() >= REFRESH_CONCURRENCY {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(provider.calls(), REFRESH_CONCURRENCY);

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert_eq!(provider.calls(), REFRESH_CONCURRENCY);

        provider.open();
        handle.await.unwrap();
        assert_eq!(provider.calls(), ALL_TASKS.len());
    }

    #[tokio::test]
    async fn refresh_startup_uses_bounded_concurrency() {
        let provider = GateProvider::new();
        let state = Source::ALL.into_iter().fold(test_state(), |state, source| {
            state.with_provider(source, provider.clone())
        });

        let refresh_state = state.clone();
        let handle = tokio::spawn(async move {
            refresh_state.refresh_startup().await;
        });

        for _ in 0..100 {
            if provider.calls() >= REFRESH_CONCURRENCY {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(provider.calls(), REFRESH_CONCURRENCY);

        provider.open();
        handle.await.unwrap();
        assert_eq!(provider.calls(), Source::ALL.len());
    }

    #[tokio::test]
    async fn forced_daily_range_refresh_replaces_cached_dates_incrementally() {
        let provider = IncrementalDailyProvider::new(
            vec![
                json!({ "date": "2026-01-01", "totalTokens": 10 }),
                json!({ "date": "2026-01-02", "totalTokens": 20 }),
            ],
            HashMap::from([(
                "2026-01-02".to_owned(),
                vec![json!({ "date": "2026-01-02", "totalTokens": 99 })],
            )]),
        );
        let state = test_state().with_provider(Source::Claude, provider.clone());

        let app = create_app(state);
        let first = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/claude/daily")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);

        let incremental = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/claude/daily?since=2026-01-02&until=2026-01-02&refresh=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(incremental.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value, json!([{ "date": "2026-01-02", "totalTokens": 99 }]));

        let cached = app
            .oneshot(
                Request::builder()
                    .uri("/api/claude/daily")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = to_bytes(cached.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            value,
            json!([
                { "date": "2026-01-01", "totalTokens": 10 },
                { "date": "2026-01-02", "totalTokens": 99 }
            ])
        );
        assert_eq!(provider.full_calls(), 1);
        assert_eq!(provider.daily_calls(), 1);
    }

    #[tokio::test]
    async fn local_provider_reads_persisted_ledger_after_source_files_disappear() {
        let ledger = UsageLedger::new(temp_ledger_path()).unwrap();
        let source = Source::Codex;

        let raw_sessions = vec![json!({
            "sessionId": "rollout-2026-06-01",
            "date": "2026-06-01",
            "time": "10:00",
            "inputTokens": 80,
            "outputTokens": 30,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 20,
            "totalTokens": 130,
            "totalCost": 0.12,
            "modelsUsed": ["gpt-5.5"],
            "modelBreakdowns": [{
                "modelName": "gpt-5.5",
                "inputTokens": 80,
                "outputTokens": 30,
                "cacheCreationTokens": 0,
                "cacheReadTokens": 20,
                "cost": 0.12
            }]
        })];

        ledger
            .upsert_view_rows(source, View::Sessions, &raw_sessions)
            .unwrap();
        let imported = ledger.load_view(source, View::Daily).unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(imported[0]["date"], "2026-06-01");
        assert_eq!(imported[0]["totalTokens"], 130);

        let persisted = ledger.load_view(source, View::Daily).unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0]["date"], "2026-06-01");
        assert_eq!(persisted[0]["totalTokens"], 130);
    }

    #[tokio::test]
    async fn local_codex_daily_uses_ledger_for_historical_persistence() {
        let _server_guard = ENV_LOCK.lock().unwrap();
        let _codex_guard = crate::sources::codex::tests::ENV_LOCK.lock().unwrap();
        let codex_home = temp_codex_home();
        let sessions_dir = codex_home.join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::write(
            sessions_dir.join("cross-day.jsonl"),
            [
                json!({
                    "timestamp": "2026-06-01T12:00:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            },
                            "last_token_usage": {
                                "input_tokens": 120,
                                "cached_input_tokens": 20,
                                "output_tokens": 50,
                                "total_tokens": 170
                            }
                        }
                    }
                })
                .to_string(),
                json!({
                    "timestamp": "2026-06-02T12:00:00.000Z",
                    "type": "event_msg",
                    "payload": {
                        "type": "token_count",
                        "info": {
                            "model_name": "gpt-5-codex",
                            "total_token_usage": {
                                "input_tokens": 200,
                                "cached_input_tokens": 35,
                                "output_tokens": 80,
                                "total_tokens": 280
                            }
                        }
                    }
                })
                .to_string(),
            ]
            .join("\n"),
        )
        .unwrap();

        let previous_codex_home = std::env::var_os("CODEX_HOME");
        let previous_codex_homes = std::env::var_os("TOKEN_USAGE_CODEX_HOMES");
        let previous_ledger_path = std::env::var_os("TOKEN_USAGE_LEDGER_PATH");
        let ledger_path = temp_ledger_path();
        std::env::set_var("CODEX_HOME", &codex_home);
        std::env::set_var("TOKEN_USAGE_LEDGER_PATH", &ledger_path);
        std::env::remove_var("TOKEN_USAGE_CODEX_HOMES");

        let provider = LocalSourceProvider {
            source: Source::Codex,
            ledger_path: Some(ledger_path.clone()),
        };
        let daily = provider.load(View::Daily, true).await.unwrap();

        // Codex daily now goes through the ledger with block-level dates.
        // Each token_count event becomes a block with its own event date,
        // so a cross-day session produces one daily row per day.
        assert_eq!(daily.len(), 2, "should have one row per day");
        assert_eq!(daily[0]["date"], "2026-06-01");
        assert_eq!(daily[0]["totalTokens"], 170);
        assert_eq!(daily[1]["date"], "2026-06-02");
        assert_eq!(daily[1]["totalTokens"], 110);

        // After removing the source file, the ledger still serves the
        // previously ingested data -- this is the key fix for data loss.
        let _ = fs::remove_file(sessions_dir.join("cross-day.jsonl"));
        let _ = fs::remove_dir_all(&codex_home);

        let provider_after = LocalSourceProvider {
            source: Source::Codex,
            ledger_path: Some(ledger_path),
        };
        // refresh=false must serve ledger rows without requiring source files.
        let daily_after = provider_after.load(View::Daily, false).await.unwrap();

        assert_eq!(daily_after.len(), 2, "ledger should persist after source removal");
        assert_eq!(daily_after[0]["date"], "2026-06-01");
        assert_eq!(daily_after[0]["totalTokens"], 170);
        assert_eq!(daily_after[1]["date"], "2026-06-02");
        assert_eq!(daily_after[1]["totalTokens"], 110);

        restore_env("CODEX_HOME", previous_codex_home);
        restore_env("TOKEN_USAGE_CODEX_HOMES", previous_codex_homes);
        restore_env("TOKEN_USAGE_LEDGER_PATH", previous_ledger_path);
    }

    #[tokio::test]
    async fn local_provider_skips_ingest_when_ledger_has_rows_and_not_refreshing() {
        let ledger_path = temp_ledger_path();
        let ledger = UsageLedger::new(ledger_path.clone()).unwrap();
        let source = Source::Grok;

        ledger
            .upsert_view_rows(
                source,
                View::Sessions,
                &[json!({
                    "sessionId": "grok-1",
                    "date": "2026-07-01",
                    "time": "10:00",
                    "inputTokens": 10,
                    "outputTokens": 5,
                    "cacheCreationTokens": 0,
                    "cacheReadTokens": 0,
                    "totalTokens": 15,
                    "totalCost": 0.01,
                    "modelsUsed": ["grok"],
                    "modelBreakdowns": [{
                        "modelName": "grok",
                        "inputTokens": 10,
                        "outputTokens": 5,
                        "cacheCreationTokens": 0,
                        "cacheReadTokens": 0,
                        "cost": 0.01
                    }]
                })],
            )
            .unwrap();

        let provider = LocalSourceProvider {
            source,
            ledger_path: Some(ledger_path),
        };
        let daily = provider.load(View::Daily, false).await.unwrap();
        assert_eq!(daily.len(), 1);
        assert_eq!(daily[0]["date"], "2026-07-01");
        assert_eq!(daily[0]["totalTokens"], 15);
    }

    #[test]
    fn filters_daily_and_monthly_ranges() {
        let data = vec![
            json!({ "date": "2026-01-01" }),
            json!({ "date": "2026-01-02" }),
            json!({ "month": "2026-02" }),
        ];

        assert_eq!(
            filter_by_date_range(&data, Some("20260102"), Some("20260201")),
            vec![
                json!({ "date": "2026-01-02" }),
                json!({ "month": "2026-02" })
            ]
        );
    }

    fn temp_ledger_path() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("token-usage-provider-ledger-{stamp}"))
            .join("usage-ledger.sqlite")
    }

    fn temp_codex_home() -> std::path::PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("token-usage-server-codex-{stamp}"))
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
