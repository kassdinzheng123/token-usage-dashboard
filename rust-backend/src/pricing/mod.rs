use crate::sources;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock, RwLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const LLM_PRICES_URL: &str = "https://www.llm-prices.com/current-v1.json";
const PI_DEV_MODELS_URL: &str = "https://pi.dev/models";
const MILLION: f64 = 1_000_000.0;
const PRICING_HTTP_TIMEOUT_SECS: u64 = 15;
const PRICING_CACHE_FRESH_SECS: u64 = 24 * 60 * 60;
// 2026-09-01T00:00:00Z — Anthropic standard Sonnet 5 pricing starts here.
const CLAUDE_SONNET_5_STANDARD_PRICING_START_SECS: u64 = 1_788_220_800;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ModelPricing {
    input_cost_per_token: f64,
    output_cost_per_token: f64,
    cache_creation_input_token_cost: f64,
    cache_read_input_token_cost: f64,
    input_cost_per_token_above_200k_tokens: f64,
    output_cost_per_token_above_200k_tokens: f64,
    cache_creation_input_token_cost_above_200k_tokens: f64,
    cache_read_input_token_cost_above_200k_tokens: f64,
    input_cost_per_token_above_272k_tokens: f64,
    output_cost_per_token_above_272k_tokens: f64,
    cache_creation_input_token_cost_above_272k_tokens: f64,
    cache_read_input_token_cost_above_272k_tokens: f64,
    input_cost_per_token_above_128k_tokens: f64,
    output_cost_per_token_above_128k_tokens: f64,
    cache_creation_input_token_cost_above_128k_tokens: f64,
    cache_read_input_token_cost_above_128k_tokens: f64,
}

#[derive(Debug, Clone)]
struct PiDevModel {
    provider: String,
    model_id: String,
    model_name: String,
    pricing: ModelPricing,
}

#[derive(Debug, Default)]
struct PricingDataset {
    pidev: Vec<PiDevModel>,
    primary: HashMap<String, ModelPricing>,
    secondary: HashMap<String, ModelPricing>,
    unresolved_models: HashSet<String>,
}

static PRICING: OnceLock<RwLock<PricingDataset>> = OnceLock::new();
static PRICING_REFRESH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn model_cost_usd(model: &str, usage: TokenUsage) -> f64 {
    if is_free_model(model) {
        return 0.0;
    }
    if let Some(cost) = builtin_model_cost_usd(model, usage) {
        return sanitize_cost(cost);
    }

    let Some(pricing) = find_cached_pricing(model).or_else(|| {
        refresh_pricing_for_model(model);
        find_cached_pricing(model)
    }) else {
        return 0.0;
    };

    sanitize_cost(cost_for_usage(usage, pricing))
}

fn builtin_model_cost_usd(model: &str, usage: TokenUsage) -> Option<f64> {
    let candidates = candidates_for(model);
    let is_composer_fast = candidates.iter().any(|candidate| {
        matches!(
            candidate.as_str(),
            "composer-2.5-fast"
                | "composer-2-5-fast"
                | "grok-composer-2.5-fast"
                | "grok-composer-2-5-fast"
        )
    });
    if is_composer_fast {
        return Some(composer_fast_cost_usd(usage));
    }
    let is_composer = candidates.iter().any(|candidate| {
        matches!(
            candidate.as_str(),
            "composer-2.5" | "composer-2-5" | "cursor-composer-2.5" | "cursor-composer-2-5"
        )
    });
    if is_composer {
        return Some(composer_standard_cost_usd(usage));
    }
    let is_sonnet_5 = candidates.iter().any(|candidate| {
        matches!(
            candidate.as_str(),
            "claude-sonnet-5" | "anthropic/claude-sonnet-5"
        ) || candidate.contains("claude-sonnet-5")
    });
    if is_sonnet_5 {
        return Some(claude_sonnet_5_cost_usd(usage));
    }
    None
}

fn composer_fast_cost_usd(usage: TokenUsage) -> f64 {
    (usage.input_tokens.max(0) as f64 * 3.0
        + usage.output_tokens.max(0) as f64 * 15.0
        + usage.cache_creation_tokens.max(0) as f64 * 3.0
        + usage.cache_read_tokens.max(0) as f64 * 0.08)
        / MILLION
}

fn composer_standard_cost_usd(usage: TokenUsage) -> f64 {
    (usage.input_tokens.max(0) as f64 * 0.5
        + usage.output_tokens.max(0) as f64 * 2.5
        + usage.cache_creation_tokens.max(0) as f64 * 0.5
        + usage.cache_read_tokens.max(0) as f64 * (0.5 / 37.5))
        / MILLION
}

fn claude_sonnet_5_uses_intro_pricing() -> bool {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() < CLAUDE_SONNET_5_STANDARD_PRICING_START_SECS)
        .unwrap_or(true)
}

fn claude_sonnet_5_cost_usd(usage: TokenUsage) -> f64 {
    let (input_rate, output_rate, cache_write_rate, cache_read_rate) =
        if claude_sonnet_5_uses_intro_pricing() {
            (2.0, 10.0, 2.5, 0.20)
        } else {
            (3.0, 15.0, 3.75, 0.30)
        };
    (usage.input_tokens.max(0) as f64 * input_rate
        + usage.output_tokens.max(0) as f64 * output_rate
        + usage.cache_creation_tokens.max(0) as f64 * cache_write_rate
        + usage.cache_read_tokens.max(0) as f64 * cache_read_rate)
        / MILLION
}

pub fn model_breakdown(model: &str, usage: TokenUsage, cost: Option<f64>) -> Value {
    let model_name = if model.trim().is_empty() {
        "unknown"
    } else {
        model
    };
    let cost = sanitize_cost(cost.unwrap_or_else(|| model_cost_usd(model, usage)));

    json!({
        "modelName": model_name,
        "inputTokens": usage.input_tokens,
        "outputTokens": usage.output_tokens,
        "cacheCreationTokens": usage.cache_creation_tokens,
        "cacheReadTokens": usage.cache_read_tokens,
        "cost": cost,
    })
}

fn load_pricing_dataset() -> PricingDataset {
    PricingDataset {
        pidev: load_cached_pidev_models().unwrap_or_default(),
        primary: load_cached_pricing_map(LITELLM_PRICES_URL).unwrap_or_default(),
        secondary: load_cached_pricing_map(LLM_PRICES_URL).unwrap_or_default(),
        unresolved_models: HashSet::new(),
    }
}

fn fetch_pricing_map(url: &str) -> Result<HashMap<String, ModelPricing>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(PRICING_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|err| err.to_string())?;
    let text = client
        .get(url)
        .send()
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())?;

    let map = parse_pricing_map(&text)?;
    if !map.is_empty() {
        persist_pricing_map(url, &text);
    }

    Ok(map)
}

fn load_cached_pricing_map(url: &str) -> Result<HashMap<String, ModelPricing>, String> {
    let Some(path) = pricing_cache_path(url) else {
        return Ok(HashMap::new());
    };
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    parse_pricing_map(&text)
}

fn parse_pricing_map(text: &str) -> Result<HashMap<String, ModelPricing>, String> {
    let value = serde_json::from_str::<Value>(text).map_err(|err| err.to_string())?;
    if value.get("prices").and_then(Value::as_array).is_some() {
        return Ok(load_llm_prices(&value));
    }
    Ok(load_litellm_prices(&value))
}

fn persist_pricing_map(url: &str, text: &str) {
    let Some(path) = pricing_cache_path(url) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let _ = fs::write(path, text);
}

fn load_litellm_prices(value: &Value) -> HashMap<String, ModelPricing> {
    let Some(entries) = value.as_object() else {
        return HashMap::new();
    };

    entries
        .iter()
        .filter_map(|(model_name, raw)| {
            Some((normalize_key(model_name), pricing_from_litellm(raw)?))
        })
        .collect()
}

fn load_llm_prices(value: &Value) -> HashMap<String, ModelPricing> {
    let Some(prices) = value.get("prices").and_then(Value::as_array) else {
        return HashMap::new();
    };

    let mut map = HashMap::new();
    for raw in prices {
        let Some(id) = raw.get("id").and_then(Value::as_str) else {
            continue;
        };
        let vendor = raw.get("vendor").and_then(Value::as_str);
        let input = num(raw.get("input")) / MILLION;
        let pricing = ModelPricing {
            input_cost_per_token: input,
            output_cost_per_token: num(raw.get("output")) / MILLION,
            cache_creation_input_token_cost: input,
            cache_read_input_token_cost: raw
                .get("input_cached")
                .map(|value| num(Some(value)))
                .filter(|value| *value > 0.0)
                .unwrap_or_default()
                / MILLION,
            ..ModelPricing::default()
        };
        map.insert(normalize_key(id), pricing);
        if let Some(vendor) = vendor {
            map.insert(normalize_key(&format!("{vendor}/{id}")), pricing);
        }
    }
    map
}

fn pricing_from_litellm(raw: &Value) -> Option<ModelPricing> {
    if !raw.is_object() {
        return None;
    }

    Some(ModelPricing {
        input_cost_per_token: num(raw.get("input_cost_per_token")),
        output_cost_per_token: num(raw.get("output_cost_per_token")),
        cache_creation_input_token_cost: num(raw.get("cache_creation_input_token_cost")),
        cache_read_input_token_cost: num(raw.get("cache_read_input_token_cost")),
        input_cost_per_token_above_200k_tokens: num(
            raw.get("input_cost_per_token_above_200k_tokens")
        ),
        output_cost_per_token_above_200k_tokens: num(
            raw.get("output_cost_per_token_above_200k_tokens")
        ),
        cache_creation_input_token_cost_above_200k_tokens: num(
            raw.get("cache_creation_input_token_cost_above_200k_tokens")
        ),
        cache_read_input_token_cost_above_200k_tokens: num(
            raw.get("cache_read_input_token_cost_above_200k_tokens")
        ),
        input_cost_per_token_above_272k_tokens: num(
            raw.get("input_cost_per_token_above_272k_tokens")
        ),
        output_cost_per_token_above_272k_tokens: num(
            raw.get("output_cost_per_token_above_272k_tokens")
        ),
        cache_creation_input_token_cost_above_272k_tokens: num(
            raw.get("cache_creation_input_token_cost_above_272k_tokens")
        ),
        cache_read_input_token_cost_above_272k_tokens: num(
            raw.get("cache_read_input_token_cost_above_272k_tokens")
        ),
        input_cost_per_token_above_128k_tokens: num(
            raw.get("input_cost_per_token_above_128k_tokens")
        ),
        output_cost_per_token_above_128k_tokens: num(
            raw.get("output_cost_per_token_above_128k_tokens")
        ),
        cache_creation_input_token_cost_above_128k_tokens: num(
            raw.get("cache_creation_input_token_cost_above_128k_tokens")
        ),
        cache_read_input_token_cost_above_128k_tokens: num(
            raw.get("cache_read_input_token_cost_above_128k_tokens")
        ),
    })
}

fn fetch_pidev_models() -> Result<Vec<PiDevModel>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(PRICING_HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|err| err.to_string())?;
    let text = client
        .get(PI_DEV_MODELS_URL)
        .send()
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())?;

    let models = parse_pidev_models(&text)?;
    if !models.is_empty() {
        persist_pricing_map(PI_DEV_MODELS_URL, &text);
    }
    Ok(models)
}

fn load_cached_pidev_models() -> Result<Vec<PiDevModel>, String> {
    let Some(path) = pricing_cache_path(PI_DEV_MODELS_URL) else {
        return Ok(Vec::new());
    };
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    parse_pidev_models(&text)
}

fn parse_pidev_models(text: &str) -> Result<Vec<PiDevModel>, String> {
    let mut models = Vec::new();
    let marker = r#"data-model-row="true""#;
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find(marker) {
        let row_start = search_from + offset;
        let row_end = text[row_start..]
            .find("</tr>")
            .map(|end| row_start + end)
            .unwrap_or(text.len());
        let row = &text[row_start..row_end];
        search_from = if row_end < text.len() {
            row_end + "</tr>".len()
        } else {
            text.len()
        };

        let Some(model_id) = extract_attr(row, "data-model-id") else {
            continue;
        };
        let model_name = extract_attr(row, "data-model-name").unwrap_or_default();
        let Some(provider) = extract_attr(row, "data-model-provider") else {
            continue;
        };

        let cells = extract_td_prices(row);
        if cells.len() < 5 {
            continue;
        }
        let pricing = ModelPricing {
            input_cost_per_token: clamp_price(cells[1]) / MILLION,
            output_cost_per_token: clamp_price(cells[2]) / MILLION,
            cache_read_input_token_cost: clamp_price(cells[3]) / MILLION,
            cache_creation_input_token_cost: clamp_price(cells[4]) / MILLION,
            ..ModelPricing::default()
        };
        models.push(PiDevModel {
            provider: normalize_key(&provider),
            model_id: normalize_key(&model_id),
            model_name: normalize_key(&model_name),
            pricing,
        });
    }
    Ok(models)
}

fn extract_attr(row: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let start = row.find(&needle)? + needle.len();
    let rest = &row[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn extract_td_prices(row: &str) -> Vec<f64> {
    let mut prices = Vec::new();
    let mut search = 0;
    while let Some(offset) = row[search..].find("<td") {
        let tag_start = search + offset;
        let Some(close) = row[tag_start..].find('>') else {
            break;
        };
        let content_start = tag_start + close + 1;
        let Some(end) = row[content_start..].find("</td>") else {
            break;
        };
        let content = &row[content_start..content_start + end];
        if let Some(price) = parse_price_cell(content) {
            prices.push(price);
        }
        search = content_start + end + "</td>".len();
    }
    prices
}

fn parse_price_cell(content: &str) -> Option<f64> {
    let cleaned: String = content
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    cleaned.parse::<f64>().ok().filter(|value| value.is_finite())
}

fn clamp_price(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn find_pidev_pricing(models: &[PiDevModel], model: &str) -> Option<ModelPricing> {
    if models.is_empty() {
        return None;
    }
    let candidates = candidates_for(model);
    if candidates.is_empty() {
        return None;
    }

    if let Some(pricing) = find_pidev_in_provider(models, "openrouter", &candidates) {
        return Some(pricing);
    }
    if let Some(provider) = official_pidev_provider(model) {
        if let Some(pricing) = find_pidev_in_provider(models, provider, &candidates) {
            return Some(pricing);
        }
    }
    let all: Vec<&PiDevModel> = models.iter().collect();
    find_pidev_best(&all, &candidates)
}

fn find_pidev_in_provider(
    models: &[PiDevModel],
    provider: &str,
    candidates: &[String],
) -> Option<ModelPricing> {
    let filtered: Vec<&PiDevModel> = models
        .iter()
        .filter(|model| model.provider == provider)
        .collect();
    find_pidev_best(&filtered, candidates)
}

fn find_pidev_best(models: &[&PiDevModel], candidates: &[String]) -> Option<ModelPricing> {
    let viable: Vec<&PiDevModel> = models
        .iter()
        .copied()
        .filter(|model| {
            model.pricing.input_cost_per_token > 0.0 || model.pricing.output_cost_per_token > 0.0
        })
        .collect();
    if viable.is_empty() {
        return None;
    }

    let mut best: Option<(usize, &PiDevModel)> = None;
    for model in viable.iter().copied() {
        let Some(score) = score_pidev_match(model, candidates) else {
            continue;
        };
        match best {
            None => best = Some((score, model)),
            Some((best_score, _)) if score < best_score => best = Some((score, model)),
            _ => {}
        }
    }
    best.map(|(_, model)| model.pricing)
}

fn score_pidev_match(model: &PiDevModel, candidates: &[String]) -> Option<usize> {
    let id = &model.model_id;
    let name = &model.model_name;

    if candidates.iter().any(|candidate| candidate == id) {
        return Some(0);
    }
    if !name.is_empty() && candidates.iter().any(|candidate| candidate == name) {
        return Some(1);
    }

    let mut best: Option<usize> = None;
    for candidate in candidates {
        if id.contains(candidate.as_str()) {
            let score = 2 + id.len();
            best = Some(match best {
                None => score,
                Some(existing) => existing.min(score),
            });
        }
    }
    best
}

fn official_pidev_provider(model: &str) -> Option<&'static str> {
    let normalized = normalize_key(model);
    if normalized.contains("deepseek") {
        Some("deepseek")
    } else if normalized.contains("claude") {
        Some("anthropic")
    } else if normalized.contains("gemini") {
        Some("google")
    } else if normalized.contains("gpt") {
        Some("openai")
    } else if normalized.contains("glm") || normalized.contains("ark-code") {
        Some("zai")
    } else if normalized.contains("grok") {
        Some("xai")
    } else if normalized.contains("mimo") {
        Some("xiaomi")
    } else {
        None
    }
}

fn find_pricing(dataset: &PricingDataset, model: &str) -> Option<ModelPricing> {
    if let Some(pricing) = find_pidev_pricing(&dataset.pidev, model) {
        return Some(pricing);
    }
    let candidates = candidates_for(model);
    for candidate in &candidates {
        if let Some(pricing) = dataset.primary.get(candidate).copied() {
            if !pricing_has_no_rates(pricing) {
                return Some(pricing);
            }
        }
    }
    for candidate in &candidates {
        if let Some(pricing) = dataset.secondary.get(candidate).copied() {
            if !pricing_has_no_rates(pricing) {
                return Some(pricing);
            }
        }
    }
    None
}

fn find_cached_pricing(model: &str) -> Option<ModelPricing> {
    let pricing = PRICING.get_or_init(|| RwLock::new(load_pricing_dataset()));
    let dataset = pricing.read().ok()?;
    find_pricing(&dataset, model)
}

fn refresh_pricing_for_model(model: &str) {
    let normalized_model = normalize_key(model);
    if normalized_model.is_empty() {
        return;
    }

    let pricing = PRICING.get_or_init(|| RwLock::new(load_pricing_dataset()));
    {
        let Ok(dataset) = pricing.read() else {
            return;
        };
        if find_pricing(&dataset, model).is_some()
            || dataset.unresolved_models.contains(&normalized_model)
        {
            return;
        }
    }

    let refresh_lock = PRICING_REFRESH_LOCK.get_or_init(|| Mutex::new(()));
    let Ok(_guard) = refresh_lock.lock() else {
        return;
    };

    {
        let Ok(dataset) = pricing.read() else {
            return;
        };
        if find_pricing(&dataset, model).is_some()
            || dataset.unresolved_models.contains(&normalized_model)
        {
            return;
        }
    }

    if pricing_cache_is_fresh() {
        if let Ok(mut dataset) = pricing.write() {
            dataset.unresolved_models.insert(normalized_model);
        }
        return;
    }

    let pidev = fetch_pidev_models().ok();
    let primary = fetch_pricing_map(LITELLM_PRICES_URL).ok();
    let secondary = fetch_pricing_map(LLM_PRICES_URL).ok();

    let Ok(mut dataset) = pricing.write() else {
        return;
    };

    if let Some(pidev) = pidev.filter(|models| !models.is_empty()) {
        dataset.pidev = pidev;
    }
    if let Some(primary) = primary.filter(|map| !map.is_empty()) {
        dataset.primary = primary;
    }
    if let Some(secondary) = secondary.filter(|map| !map.is_empty()) {
        dataset.secondary = secondary;
    }

    if find_pricing(&dataset, model).is_none() {
        dataset.unresolved_models.insert(normalized_model);
    }
}

fn pricing_cache_path(url: &str) -> Option<PathBuf> {
    let base = std::env::var_os("TOKEN_USAGE_PRICING_CACHE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".token-usage").join("pricing-cache"))
        })?;

    let file_name = match url {
        LITELLM_PRICES_URL => "litellm-model-prices.json",
        LLM_PRICES_URL => "llm-prices-current-v1.json",
        PI_DEV_MODELS_URL => "pidev-models.html",
        _ => return None,
    };
    Some(base.join(file_name))
}

fn pricing_cache_is_fresh() -> bool {
    [LITELLM_PRICES_URL, LLM_PRICES_URL, PI_DEV_MODELS_URL].into_iter().any(|url| {
        let Some(path) = pricing_cache_path(url) else {
            return false;
        };
        let Ok(modified) = fs::metadata(path).and_then(|metadata| metadata.modified()) else {
            return false;
        };
        SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age < Duration::from_secs(PRICING_CACHE_FRESH_SECS))
    })
}

fn candidates_for(model: &str) -> Vec<String> {
    let mut candidates = HashSet::new();
    add_candidate(&mut candidates, model);

    let resolved = resolve_alias(model);
    if resolved != model {
        add_candidate(&mut candidates, &resolved);
    }

    let custom_stripped = custom_stripped_model(model);
    let custom_resolved = custom_stripped
        .as_deref()
        .map(resolve_alias)
        .filter(|resolved| Some(resolved.as_str()) != custom_stripped.as_deref());
    if let Some(stripped) = custom_stripped.as_deref() {
        add_candidate(&mut candidates, stripped);
    }
    if let Some(resolved) = custom_resolved.as_deref() {
        add_candidate(&mut candidates, resolved);
    }

    if let Some((_, suffix)) = model.rsplit_once('/') {
        add_candidate(&mut candidates, suffix);
        let resolved_suffix = resolve_alias(suffix);
        if resolved_suffix != suffix {
            add_candidate(&mut candidates, &resolved_suffix);
        }
    }

    let prefixes = [
        "anthropic/",
        "github_copilot/",
        "claude-",
        "chatgpt/",
        "openai/",
        "azure/",
        "fireworks_ai/",
        "openrouter/openai/",
        "openrouter/anthropic/",
        "minimax/",
        "openrouter/minimax/",
        "moonshot/",
        "openrouter/moonshotai/",
        "xiaomi/",
        "openrouter/xiaomi/",
        "xiaomi-token-plan-sgp/",
        "xiaomi-token-plan-ams/",
        "xiaomi-token-plan-cn/",
    ];
    for prefix in prefixes {
        add_candidate(&mut candidates, &format!("{prefix}{model}"));
        if resolved != model {
            add_candidate(&mut candidates, &format!("{prefix}{resolved}"));
        }
        if let Some(stripped) = custom_stripped.as_deref() {
            add_candidate(&mut candidates, &format!("{prefix}{stripped}"));
        }
        if let Some(resolved) = custom_resolved.as_deref() {
            add_candidate(&mut candidates, &format!("{prefix}{resolved}"));
        }
        if let Some((_, suffix)) = model.rsplit_once('/') {
            add_candidate(&mut candidates, &format!("{prefix}{suffix}"));
            let resolved_suffix = resolve_alias(suffix);
            if resolved_suffix != suffix {
                add_candidate(&mut candidates, &format!("{prefix}{resolved_suffix}"));
            }
        }
    }

    add_provider_variants(&mut candidates, model);
    if let Some(stripped) = custom_stripped.as_deref() {
        add_provider_variants(&mut candidates, stripped);
    }
    add_fireworks_router_variants(&mut candidates, model);
    if let Some(stripped) = custom_stripped.as_deref() {
        add_fireworks_router_variants(&mut candidates, stripped);
    }

    let mut candidates = candidates.into_iter().collect::<Vec<_>>();
    candidates.sort();
    candidates
}

fn custom_stripped_model(model: &str) -> Option<String> {
    normalize_key(model)
        .strip_prefix("custom:")
        .filter(|stripped| !stripped.is_empty())
        .map(ToOwned::to_owned)
}

fn add_candidate(candidates: &mut HashSet<String>, value: &str) {
    let normalized = normalize_key(value);
    if normalized.is_empty() {
        return;
    }
    candidates.insert(normalized.clone());
    candidates.insert(normalized.replace(':', "-"));
    candidates.insert(normalized.replace('.', "-"));
    candidates.insert(normalized.replace(['.', ':'], "-"));
}

fn add_provider_variants(candidates: &mut HashSet<String>, model: &str) {
    let Some((provider, suffix)) = model.split_once('/') else {
        return;
    };

    let provider = normalize_key(provider);
    let suffix = suffix.trim();
    if provider.is_empty() || suffix.is_empty() {
        return;
    }

    let provider_variants = [
        provider.clone(),
        provider.replace('-', "_"),
        provider.replace('_', "-"),
    ];
    let resolved_suffix = resolve_alias(suffix);
    for provider in provider_variants {
        add_candidate(candidates, &format!("{provider}/{suffix}"));
        if resolved_suffix != suffix {
            add_candidate(candidates, &format!("{provider}/{resolved_suffix}"));
        }
    }
}

fn add_fireworks_router_variants(candidates: &mut HashSet<String>, model: &str) {
    let normalized = normalize_key(model);
    let Some((provider, route)) = normalized.split_once("/accounts/fireworks/routers/") else {
        return;
    };

    let base = route.strip_suffix("-turbo").unwrap_or(route);
    for provider in [provider, "fireworks_ai"] {
        add_candidate(
            candidates,
            &format!("{provider}/accounts/fireworks/models/{base}"),
        );
        add_candidate(candidates, &format!("{provider}/{base}"));
    }
}

fn resolve_alias(model: &str) -> String {
    let normalized = normalize_key(model);
    if normalized == "kiro-claude-opus-4.7" || normalized == "kiro-claude-opus-4-7" {
        return "claude-opus-4-7".to_string();
    }
    if is_kiro_model(&normalized) {
        return sources::cluster_model_name(&normalized);
    }
    if normalized.contains("claude-opus-4.7") || normalized.contains("claude-opus-4-7") {
        return "claude-opus-4-7".to_string();
    }
    if normalized.contains("ark-code-latest") || normalized == "ark-code" {
        return "glm-5.2".to_string();
    }
    if normalized.contains("glm-5.2") || normalized.contains("glm-5-2") {
        return "glm-5.2".to_string();
    }
    if normalized.contains("step-3.7-flash") {
        return "step-3.7-flash".to_string();
    }
    if normalized.contains("composer-2.5-fast") || normalized.contains("composer-2-5-fast") {
        return "composer-2.5-fast".to_string();
    }
    if normalized.contains("composer-2.5") || normalized.contains("composer-2-5") {
        return "composer-2.5".to_string();
    }
    if normalized.contains("claude-sonnet-5") {
        return "claude-sonnet-5".to_string();
    }
    if normalized.contains("gpt-5.5")
        && !normalized.contains("gpt-5.5-mini")
        && !normalized.contains("gpt-5.5-nano")
        && !normalized.contains("gpt-5.5-pro")
    {
        return "gpt-5.5".to_string();
    }
    if normalized.contains("gpt-5.4")
        && !normalized.contains("gpt-5.4-mini")
        && !normalized.contains("gpt-5.4-nano")
        && !normalized.contains("gpt-5.4-pro")
    {
        return "gpt-5.4".to_string();
    }
    match normalized.as_str() {
        "claude-opus-4-6-thinking" => "claude-opus-4-6".to_string(),
        "claude-sonnet-4.5" | "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929".to_string(),
        "claude-haiku-4.5" | "claude-haiku-4-5" => "claude-haiku-4-5-20251001".to_string(),
        "claude-opus-4.8" | "claude-opus-4-8" => "claude-opus-4-8".to_string(),
        "gpt-5-codex" => "gpt-5".to_string(),
        "gpt-5.3-codex-spark" => "gpt-5.3-codex".to_string(),
        "gpt-5.3-codex" => "gpt-5.2-codex".to_string(),
        _ => sources::cluster_model_name(&normalized),
    }
}

fn cost_for_usage(usage: TokenUsage, pricing: ModelPricing) -> f64 {
    calculate_tiered_cost(
        usage.input_tokens,
        pricing.input_cost_per_token,
        tiered_prices(
            pricing.input_cost_per_token_above_128k_tokens,
            pricing.input_cost_per_token_above_200k_tokens,
            pricing.input_cost_per_token_above_272k_tokens,
        ),
    ) + calculate_tiered_cost(
        usage.output_tokens,
        pricing.output_cost_per_token,
        tiered_prices(
            pricing.output_cost_per_token_above_128k_tokens,
            pricing.output_cost_per_token_above_200k_tokens,
            pricing.output_cost_per_token_above_272k_tokens,
        ),
    ) + calculate_tiered_cost(
        usage.cache_creation_tokens,
        nonzero_or(
            pricing.cache_creation_input_token_cost,
            pricing.input_cost_per_token,
        ),
        tiered_prices(
            pricing.cache_creation_input_token_cost_above_128k_tokens,
            pricing.cache_creation_input_token_cost_above_200k_tokens,
            pricing.cache_creation_input_token_cost_above_272k_tokens,
        ),
    ) + calculate_tiered_cost(
        usage.cache_read_tokens,
        pricing.cache_read_input_token_cost,
        tiered_prices(
            pricing.cache_read_input_token_cost_above_128k_tokens,
            pricing.cache_read_input_token_cost_above_200k_tokens,
            pricing.cache_read_input_token_cost_above_272k_tokens,
        ),
    )
}

fn tiered_prices(
    price_above_128k: f64,
    price_above_200k: f64,
    price_above_272k: f64,
) -> [(f64, f64); 3] {
    [
        (128_000.0, price_above_128k),
        (200_000.0, price_above_200k),
        (272_000.0, price_above_272k),
    ]
}

fn calculate_tiered_cost(tokens: i64, base_price: f64, tiered_prices: [(f64, f64); 3]) -> f64 {
    let tokens = tokens.max(0) as f64;
    if tokens == 0.0 {
        return 0.0;
    }
    if let Some((threshold, tiered_price)) = tiered_prices
        .into_iter()
        .find(|(threshold, price)| tokens > *threshold && *price > 0.0)
    {
        let below = tokens.min(threshold);
        let above = (tokens - threshold).max(0.0);
        return below * base_price + above * tiered_price;
    }
    tokens * base_price
}

fn pricing_has_no_rates(pricing: ModelPricing) -> bool {
    pricing.input_cost_per_token == 0.0
        && pricing.output_cost_per_token == 0.0
        && pricing.cache_creation_input_token_cost == 0.0
        && pricing.cache_read_input_token_cost == 0.0
        && pricing.input_cost_per_token_above_200k_tokens == 0.0
        && pricing.output_cost_per_token_above_200k_tokens == 0.0
        && pricing.cache_creation_input_token_cost_above_200k_tokens == 0.0
        && pricing.cache_read_input_token_cost_above_200k_tokens == 0.0
        && pricing.input_cost_per_token_above_272k_tokens == 0.0
        && pricing.output_cost_per_token_above_272k_tokens == 0.0
        && pricing.cache_creation_input_token_cost_above_272k_tokens == 0.0
        && pricing.cache_read_input_token_cost_above_272k_tokens == 0.0
        && pricing.input_cost_per_token_above_128k_tokens == 0.0
        && pricing.output_cost_per_token_above_128k_tokens == 0.0
        && pricing.cache_creation_input_token_cost_above_128k_tokens == 0.0
        && pricing.cache_read_input_token_cost_above_128k_tokens == 0.0
}

fn normalize_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn is_free_model(model: &str) -> bool {
    let normalized = normalize_key(model);
    normalized == "openrouter/free"
        || normalized.ends_with(":free")
        || normalized.ends_with("-free")
        || normalized.contains("/free/")
}

fn is_kiro_model(model: &str) -> bool {
    let normalized = normalize_key(model);
    normalized == "kiro"
        || normalized.starts_with("kiro-")
        || normalized.starts_with("kiro/")
        || normalized.contains("-kiro-")
        || normalized.contains("/kiro/")
}

fn nonzero_or(value: f64, fallback: f64) -> f64 {
    if value > 0.0 {
        value
    } else {
        fallback
    }
}

fn num(value: Option<&Value>) -> f64 {
    value
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.parse::<f64>().ok()))
        })
        .filter(|value| value.is_finite())
        .unwrap_or_default()
}

fn sanitize_cost(cost: f64) -> f64 {
    if cost.is_finite() && cost >= 0.0 {
        cost
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: i64, output: i64, cache_creation: i64, cache_read: i64) -> TokenUsage {
        TokenUsage {
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: cache_creation,
            cache_read_tokens: cache_read,
        }
    }

    #[test]
    fn calculates_gpt_5_cost_with_cache_tokens() {
        let usage = usage(1_000, 500, 200, 300);
        let pricing = ModelPricing {
            input_cost_per_token: 1.25 / MILLION,
            output_cost_per_token: 10.0 / MILLION,
            cache_creation_input_token_cost: 1.25 / MILLION,
            cache_read_input_token_cost: 0.125 / MILLION,
            ..ModelPricing::default()
        };
        let cost = cost_for_usage(usage, pricing);
        let expected = (1_000.0 * 1.25 + 500.0 * 10.0 + 200.0 * 1.25 + 300.0 * 0.125) / MILLION;

        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn calculates_builtin_grok_composer_25_fast_cost() {
        let usage = usage(1_000_000, 1_000_000, 250_000, 500_000);
        let cost = model_cost_usd("grok-composer-2.5-fast", usage);
        let expected = (1_250_000.0 * 3.0 + 1_000_000.0 * 15.0 + 500_000.0 * 0.08) / MILLION;

        assert!((cost - expected).abs() < 1e-12);
        assert_eq!(model_cost_usd("composer-2.5-fast", usage), cost);
    }

    #[test]
    fn calculates_builtin_claude_sonnet_5_intro_cost() {
        let usage = usage(1_000_000, 1_000_000, 250_000, 500_000);
        let cost = model_cost_usd("claude-sonnet-5", usage);
        let expected = if claude_sonnet_5_uses_intro_pricing() {
            (1_000_000.0 * 2.0 + 1_000_000.0 * 10.0 + 250_000.0 * 2.5 + 500_000.0 * 0.20)
                / MILLION
        } else {
            (1_000_000.0 * 3.0 + 1_000_000.0 * 15.0 + 250_000.0 * 3.75 + 500_000.0 * 0.30)
                / MILLION
        };

        assert!((cost - expected).abs() < 1e-12);
        assert_eq!(model_cost_usd("claude-sonnet-5-thinking-high", usage), cost);
        assert_eq!(model_cost_usd("anthropic/claude-sonnet-5", usage), cost);
    }

    #[test]
    fn resolves_claude_sonnet_5_alias() {
        let candidates = candidates_for("claude-sonnet-5-thinking-high");
        assert!(candidates.contains(&"claude-sonnet-5".to_string()));
    }

    #[test]
    fn calculates_builtin_composer_25_standard_cost() {
        let usage = usage(1_000_000, 1_000_000, 250_000, 500_000);
        let cost = model_cost_usd("composer-2.5", usage);
        let expected =
            (1_250_000.0 * 0.5 + 1_000_000.0 * 2.5 + 500_000.0 * (0.5 / 37.5)) / MILLION;

        assert!((cost - expected).abs() < 1e-12);
        assert_eq!(model_cost_usd("cursor-composer-2-5", usage), cost);
    }

    #[test]
    fn includes_xiaomi_pricing_candidates() {
        let candidates = candidates_for("mimo-v2.5-pro");

        assert!(candidates.contains(&"xiaomi/mimo-v2.5-pro".to_string()));
        assert!(candidates.contains(&"openrouter/xiaomi/mimo-v2.5-pro".to_string()));
    }

    #[test]
    fn resolves_codex_aliases() {
        let candidates = candidates_for("gpt-5-codex");
        assert!(candidates.contains(&"gpt-5".to_string()));
    }

    #[test]
    fn resolves_newer_codex_alias() {
        let candidates = candidates_for("gpt-5.3-codex");
        assert!(candidates.contains(&"gpt-5.2-codex".to_string()));
    }

    #[test]
    fn resolves_codex_spark_alias() {
        let candidates = candidates_for("poe/openai/gpt-5.3-codex-spark");
        assert!(candidates.contains(&"gpt-5.3-codex".to_string()));
        assert!(candidates.contains(&"chatgpt/gpt-5.3-codex".to_string()));
    }

    #[test]
    fn keeps_mini_models_from_resolving_to_full_model_pricing() {
        let candidates = candidates_for("gpt-5.4-mini");
        assert!(candidates.contains(&"gpt-5.4-mini".to_string()));
        assert!(candidates.contains(&"openai/gpt-5.4-mini".to_string()));
        assert!(!candidates.contains(&"gpt-5.4".to_string()));
        assert!(!candidates.contains(&"openai/gpt-5.4".to_string()));

        let prices = r#"{
            "gpt-5.4": {
                "input_cost_per_token": 0.0000025,
                "output_cost_per_token": 0.000015,
                "cache_read_input_token_cost": 0.00000025
            },
            "gpt-5.4-mini": {
                "input_cost_per_token": 0.00000075,
                "output_cost_per_token": 0.0000045,
                "cache_read_input_token_cost": 0.000000075
            }
        }"#;
        let dataset = PricingDataset {
            pidev: Vec::new(),
            primary: parse_pricing_map(prices).expect("pricing parses"),
            secondary: HashMap::new(),
            unresolved_models: HashSet::new(),
        };
        let pricing = find_pricing(&dataset, "gpt-5.4-mini").expect("pricing exists");

        assert_eq!(pricing.input_cost_per_token, 0.00000075);
        assert_eq!(pricing.output_cost_per_token, 0.0000045);
        assert_eq!(pricing.cache_read_input_token_cost, 0.000000075);
    }

    #[test]
    fn resolves_custom_prefixed_models() {
        let codex = candidates_for("custom:gpt-5-codex");
        assert!(codex.contains(&"gpt-5-codex".to_string()));
        assert!(codex.contains(&"gpt-5".to_string()));
        assert!(codex.contains(&"openai/gpt-5".to_string()));

        let factory = candidates_for("custom:OmniMind-GPT-5.5-High-0");
        assert!(factory.contains(&"gpt-5.5".to_string()));
        assert!(factory.contains(&"openai/gpt-5.5".to_string()));

        let kiro = candidates_for("custom:pi-mono-kiro-claude-opus-4.7-3");
        assert!(!kiro.contains(&"claude-opus-4-7".to_string()));
        assert!(!kiro.contains(&"anthropic/claude-opus-4-7".to_string()));

        let pi_kiro = candidates_for("kiro-claude-opus-4.7");
        assert!(pi_kiro.contains(&"claude-opus-4-7".to_string()));
        assert!(pi_kiro.contains(&"anthropic/claude-opus-4-7".to_string()));
    }

    #[test]
    fn resolves_ark_code_latest_to_glm_52() {
        let candidates = candidates_for("CPA/ark-code-latest");
        assert!(candidates.contains(&"glm-5.2".to_string()));
        assert_eq!(official_pidev_provider("CPA/ark-code-latest"), Some("zai"));
    }

    #[test]
    fn resolves_glm_52_variants_to_glm_52() {
        for model in ["glm-5.2-max", "glm-5.2", "z-ai/glm-5.2-preview", "glm-5-2-max"] {
            let candidates = candidates_for(model);
            assert!(
                candidates.contains(&"glm-5.2".to_string()),
                "expected glm-5.2 candidate for {model}"
            );
        }
    }

    #[test]
    fn pricing_http_timeout_is_not_too_aggressive() {
        assert!(PRICING_HTTP_TIMEOUT_SECS >= 15);
    }

    #[test]
    fn pricing_cache_freshness_cools_unknown_models_for_at_least_a_day() {
        assert!(PRICING_CACHE_FRESH_SECS >= 24 * 60 * 60);
    }

    #[test]
    fn parses_and_finds_litellm_pricing() {
        let prices = r#"{
            "gpt-5.5": {
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.00003,
                "cache_read_input_token_cost": 0.0000005
            }
        }"#;
        let map = parse_pricing_map(prices).expect("pricing parses");
        let dataset = PricingDataset {
            pidev: Vec::new(),
            primary: map,
            secondary: HashMap::new(),
            unresolved_models: HashSet::new(),
        };
        let pricing = find_pricing(&dataset, "openai/gpt-5.5").expect("pricing exists");

        assert_eq!(pricing.input_cost_per_token, 0.000005);
        assert_eq!(pricing.output_cost_per_token, 0.00003);
        assert_eq!(pricing.cache_read_input_token_cost, 0.0000005);
    }

    #[test]
    fn finds_litellm_pricing_for_custom_prefixed_model() {
        let prices = r#"{
            "gpt-5.5": {
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.00003
            }
        }"#;
        let map = parse_pricing_map(prices).expect("pricing parses");
        let dataset = PricingDataset {
            pidev: Vec::new(),
            primary: map,
            secondary: HashMap::new(),
            unresolved_models: HashSet::new(),
        };
        let pricing = find_pricing(&dataset, "custom:gpt-5.5").expect("pricing exists");

        assert_eq!(pricing.input_cost_per_token, 0.000005);
        assert_eq!(pricing.output_cost_per_token, 0.00003);
    }

    #[test]
    fn does_not_price_kiro_models_through_broad_aliases() {
        let prices = r#"{
            "claude-opus-4-7": {
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.000025
            }
        }"#;
        let map = parse_pricing_map(prices).expect("pricing parses");
        let dataset = PricingDataset {
            pidev: Vec::new(),
            primary: map,
            secondary: HashMap::new(),
            unresolved_models: HashSet::new(),
        };

        assert!(find_pricing(&dataset, "custom:pi-mono-kiro-claude-opus-4.7-3").is_none());
        assert!(find_pricing(&dataset, "kiro-claude-opus-4.7").is_some());
    }

    #[test]
    fn parses_and_finds_llm_prices_pricing() {
        let prices = r#"{
            "prices": [
                {
                    "id": "deepseek-v4-flash",
                    "vendor": "deepseek",
                    "input": 0.14,
                    "output": 0.28
                }
            ]
        }"#;
        let map = parse_pricing_map(prices).expect("pricing parses");
        let dataset = PricingDataset {
            pidev: Vec::new(),
            primary: HashMap::new(),
            secondary: map,
            unresolved_models: HashSet::new(),
        };
        let pricing = find_pricing(&dataset, "deepseek/deepseek-v4-flash").expect("pricing exists");

        assert_eq!(pricing.input_cost_per_token, 0.14 / MILLION);
        assert_eq!(pricing.output_cost_per_token, 0.28 / MILLION);
    }

    #[test]
    fn includes_provider_and_route_variants() {
        let github = candidates_for("github-copilot/gpt-5-mini");
        assert!(github.contains(&"github_copilot/gpt-5-mini".to_string()));

        let chatgpt = candidates_for("openai/gpt-5.3-codex-spark");
        assert!(chatgpt.contains(&"chatgpt/openai/gpt-5.3-codex-spark".to_string()));
        assert!(chatgpt.contains(&"chatgpt/gpt-5.3-codex-spark".to_string()));

        let fireworks = candidates_for("fireworks-ai/accounts/fireworks/routers/kimi-k2p5-turbo");
        assert!(fireworks.contains(&"fireworks_ai/accounts/fireworks/models/kimi-k2p5".to_string()));
        assert!(fireworks.contains(&"fireworks_ai/kimi-k2p5".to_string()));
    }

    #[test]
    fn applies_272k_tiered_pricing() {
        let pricing = ModelPricing {
            input_cost_per_token: 2.5 / MILLION,
            input_cost_per_token_above_272k_tokens: 5.0 / MILLION,
            ..ModelPricing::default()
        };
        let cost = cost_for_usage(usage(300_000, 0, 0, 0), pricing);
        let expected = (272_000.0 * 2.5 + 28_000.0 * 5.0) / MILLION;
        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn returns_zero_for_free_models() {
        let usage = usage(1_000, 500, 200, 300);

        assert_eq!(model_cost_usd("openrouter/free", usage), 0.0);
        assert_eq!(model_cost_usd("openrouter/openai/gpt-5:free", usage), 0.0);
        assert_eq!(model_cost_usd("kimi-k2.5-free", usage), 0.0);
    }

    #[test]
    fn builds_breakdown_with_explicit_cost_or_computed_cost() {
        let usage = usage(10, 20, 30, 40);
        let explicit = model_breakdown("gpt-5-codex", usage, Some(12.5));
        assert_eq!(
            explicit,
            json!({
                "modelName": "gpt-5-codex",
                "inputTokens": 10,
                "outputTokens": 20,
                "cacheCreationTokens": 30,
                "cacheReadTokens": 40,
                "cost": 12.5,
            })
        );

        let computed = model_breakdown("openrouter/free", usage, None);
        assert_eq!(computed["modelName"], "openrouter/free");
        assert!(computed["cost"].as_f64().unwrap_or_default() >= 0.0);
    }

    const PIDEV_HTML_SAMPLE: &str = r#"
<table>
<tbody>
<tr class="models-provider-group-row" data-model-group="true" data-model-group-provider="openrouter"></tr>
<tr data-model-row="true" data-model-name="openai gpt-5.5" data-model-id="openai/gpt-5.5" data-model-provider="openrouter"><th class="models-model-col" scope="row"><a href="/models/openrouter/openai-gpt-5-5" class="models-model-link" data-model-link="true" data-model-path="/models/openrouter/openai-gpt-5-5">OpenAI: GPT-5.5</a><code>openai/gpt-5.5</code></th><td class="data-table-col-num" data-label="Context">1,050,000</td><td class="data-table-col-num" data-label="Input /M">$5</td><td class="data-table-col-num" data-label="Output /M">$30</td><td class="data-table-col-num" data-label="Cache read /M">$0.5</td><td class="data-table-col-num" data-label="Cache write /M">$0</td></tr>
<tr data-model-row="true" data-model-name="anthropic claude opus 4.6" data-model-id="anthropic/claude-opus-4.6" data-model-provider="openrouter"><th><a href="/models/openrouter/anthropic-claude-opus-4-6">Anthropic: Claude Opus 4.6</a><code>anthropic/claude-opus-4.6</code></th><td>1,000,000</td><td>$5</td><td>$25</td><td>$0.5</td><td>$6.25</td></tr>
<tr data-model-row="true" data-model-name="stepfun step 3.7 flash" data-model-id="stepfun/step-3.7-flash" data-model-provider="openrouter"><th><a href="/models/openrouter/stepfun-step-3-7-flash">StepFun: Step 3.7 Flash</a><code>stepfun/step-3.7-flash</code></th><td>256,000</td><td>$0.2</td><td>$1.15</td><td>$0.04</td><td>$0</td></tr>
<tr data-model-row="true" data-model-name="xiaomi mimo v2.5 pro" data-model-id="xiaomi/mimo-v2.5-pro" data-model-provider="openrouter"><th><a href="/models/openrouter/xiaomi-mimo-v2-5-pro">Xiaomi: MiMo-V2.5-Pro</a><code>xiaomi/mimo-v2.5-pro</code></th><td>1,048,576</td><td>$0.435</td><td>$0.87</td><td>$0.0036</td><td>$0</td></tr>
</tbody>
<tbody>
<tr class="models-provider-group-row" data-model-group="true" data-model-group-provider="anthropic"></tr>
<tr data-model-row="true" data-model-name="claude opus 4.6" data-model-id="claude-opus-4-6" data-model-provider="anthropic"><th><a href="/models/anthropic/claude-opus-4-6">Claude Opus 4.6</a><code>claude-opus-4-6</code></th><td>1,000,000</td><td>$5</td><td>$25</td><td>$0.5</td><td>$6.25</td></tr>
</tbody>
<tbody>
<tr class="models-provider-group-row" data-model-group="true" data-model-group-provider="xai"></tr>
<tr data-model-row="true" data-model-name="grok build 0.1" data-model-id="grok-build-0.1" data-model-provider="xai"><th><a href="/models/xai/grok-build-0-1">Grok Build 0.1</a><code>grok-build-0.1</code></th><td>256,000</td><td>$1</td><td>$2</td><td>$0.2</td><td>$0</td></tr>
</tbody>
<tbody>
<tr class="models-provider-group-row" data-model-group="true" data-model-group-provider="openrouter"></tr>
<tr data-model-row="true" data-model-name="openai gpt-5.5" data-model-id="gpt-5.5" data-model-provider="openrouter"><th><a href="/models/openrouter/gpt-5-5">OpenAI: GPT-5.5</a><code>gpt-5.5</code></th><td>1,050,000</td><td>$0</td><td>$0</td><td>$0</td><td>$0</td></tr>
</tbody>
</table>
"#;

    #[test]
    fn parses_pidev_html_rows_into_pricing_entries() {
        let models = parse_pidev_models(PIDEV_HTML_SAMPLE).expect("parses");

        let gpt55 = models
            .iter()
            .find(|model| model.model_id == "openai/gpt-5.5")
            .expect("openai/gpt-5.5 present");
        assert_eq!(gpt55.provider, "openrouter");
        assert_eq!(gpt55.pricing.input_cost_per_token, 5.0 / MILLION);
        assert_eq!(gpt55.pricing.output_cost_per_token, 30.0 / MILLION);
        assert_eq!(gpt55.pricing.cache_read_input_token_cost, 0.5 / MILLION);
        assert_eq!(gpt55.pricing.cache_creation_input_token_cost, 0.0);

        assert!(models.iter().any(|model| model.model_id == "claude-opus-4-6"));
    }

    #[test]
    fn pidev_prefers_openrouter_then_official_provider_then_any() {
        let models = parse_pidev_models(PIDEV_HTML_SAMPLE).expect("parses");

        // gpt-5.5 -> openrouter provider has a non-zero priced entry.
        let pricing = find_pidev_pricing(&models, "gpt-5.5").expect("found");
        assert_eq!(pricing.input_cost_per_token, 5.0 / MILLION);
        assert_eq!(pricing.output_cost_per_token, 30.0 / MILLION);

        // claude-opus-4.6 -> only anthropic provider exposes it here.
        let pricing = find_pidev_pricing(&models, "claude-opus-4.6").expect("found");
        assert_eq!(pricing.input_cost_per_token, 5.0 / MILLION);
        assert_eq!(pricing.output_cost_per_token, 25.0 / MILLION);

        // grok-build-0.1 -> only xai provider exposes it.
        let pricing = find_pidev_pricing(&models, "grok-build-0.1").expect("found");
        assert_eq!(pricing.input_cost_per_token, 1.0 / MILLION);
    }

    #[test]
    fn pidev_skips_zero_input_output_entries() {
        let models = parse_pidev_models(PIDEV_HTML_SAMPLE).expect("parses");

        // The bare `gpt-5.5` id row under openrouter is all-zero; the
        // `openai/gpt-5.5` row should win instead.
        let pricing = find_pidev_pricing(&models, "gpt-5.5").expect("found");
        assert_eq!(pricing.input_cost_per_token, 5.0 / MILLION);
    }

    #[test]
    fn pidev_matches_step_and_mimo_via_openrouter() {
        let models = parse_pidev_models(PIDEV_HTML_SAMPLE).expect("parses");

        let step = find_pidev_pricing(&models, "step-3.7-flash").expect("found");
        assert_eq!(step.input_cost_per_token, 0.2 / MILLION);
        assert_eq!(step.output_cost_per_token, 1.15 / MILLION);

        let mimo = find_pidev_pricing(&models, "mimo-v2.5-pro").expect("found");
        assert_eq!(mimo.input_cost_per_token, 0.435 / MILLION);
        assert_eq!(mimo.output_cost_per_token, 0.87 / MILLION);
    }

    #[test]
    fn builtin_composer_pricing_still_applies_without_pidev_entry() {
        let usage = usage(1_000_000, 1_000_000, 250_000, 500_000);
        let cost = model_cost_usd("composer-2.5-fast", usage);
        let expected = (1_250_000.0 * 3.0 + 1_000_000.0 * 15.0 + 500_000.0 * 0.08) / MILLION;

        assert!((cost - expected).abs() < 1e-12);
    }

    #[test]
    fn ark_code_latest_maps_to_glm_52_for_pricing_and_provider() {
        // Provider rule: ark-code routes to the zai official provider, and
        // resolve_alias maps it to glm-5.2 so pi.dev lookup works end-to-end.
        assert_eq!(official_pidev_provider("ark-code-latest"), Some("zai"));
        assert!(candidates_for("CPA/ark-code-latest").contains(&"glm-5.2".to_string()));

        let models = vec![
            PiDevModel {
                provider: "zai".to_string(),
                model_id: "glm-5.2".to_string(),
                model_name: "glm 5.2".to_string(),
                pricing: ModelPricing {
                    input_cost_per_token: 1.4 / MILLION,
                    output_cost_per_token: 4.4 / MILLION,
                    cache_read_input_token_cost: 0.26 / MILLION,
                    ..ModelPricing::default()
                },
            },
            PiDevModel {
                provider: "openrouter".to_string(),
                model_id: "z-ai/glm-5.2".to_string(),
                model_name: "glm 5.2".to_string(),
                pricing: ModelPricing {
                    input_cost_per_token: 0.0,
                    output_cost_per_token: 0.0,
                    ..ModelPricing::default()
                },
            },
        ];

        // openrouter entry is all-zero and skipped, so the zai official entry wins.
        let pricing = find_pidev_pricing(&models, "CPA/ark-code-latest").expect("found");
        assert_eq!(pricing.input_cost_per_token, 1.4 / MILLION);
        assert_eq!(pricing.output_cost_per_token, 4.4 / MILLION);
    }
}
