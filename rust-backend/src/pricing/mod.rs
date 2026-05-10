use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock, RwLock},
    time::{Duration, SystemTime},
};

const LITELLM_PRICES_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const LLM_PRICES_URL: &str = "https://www.llm-prices.com/current-v1.json";
const MILLION: f64 = 1_000_000.0;
const PRICING_HTTP_TIMEOUT_SECS: u64 = 15;
const PRICING_CACHE_FRESH_SECS: u64 = 24 * 60 * 60;

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

#[derive(Debug, Default)]
struct PricingDataset {
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
    match normalize_key(model).as_str() {
        "mimo-v2.5-pro"
        | "mimo-v2-5-pro"
        | "xiaomi/mimo-v2.5-pro"
        | "xiaomi/mimo-v2-5-pro"
        | "xiaomi-token-plan-sgp/mimo-v2.5-pro"
        | "xiaomi-token-plan-sgp/mimo-v2-5-pro"
        | "xiaomi-token-plan-ams/mimo-v2.5-pro"
        | "xiaomi-token-plan-ams/mimo-v2-5-pro"
        | "xiaomi-token-plan-cn/mimo-v2.5-pro"
        | "xiaomi-token-plan-cn/mimo-v2-5-pro"
        | "openrouter/xiaomi/mimo-v2.5-pro"
        | "openrouter/xiaomi/mimo-v2-5-pro" => Some(
            (usage.input_tokens.max(0) as f64 * 1.0
                + usage.output_tokens.max(0) as f64 * 3.0
                + usage.cache_read_tokens.max(0) as f64 * 0.2)
                / MILLION,
        ),
        _ => None,
    }
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

fn find_pricing(dataset: &PricingDataset, model: &str) -> Option<ModelPricing> {
    for candidate in candidates_for(model) {
        if let Some(pricing) = dataset.primary.get(&candidate).copied() {
            if !pricing_has_no_rates(pricing) {
                return Some(pricing);
            }
        }
    }
    for candidate in candidates_for(model) {
        if let Some(pricing) = dataset.secondary.get(&candidate).copied() {
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

    let primary = fetch_pricing_map(LITELLM_PRICES_URL).ok();
    let secondary = fetch_pricing_map(LLM_PRICES_URL).ok();

    let Ok(mut dataset) = pricing.write() else {
        return;
    };

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
        _ => return None,
    };
    Some(base.join(file_name))
}

fn pricing_cache_is_fresh() -> bool {
    [LITELLM_PRICES_URL, LLM_PRICES_URL].into_iter().any(|url| {
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

    candidates.into_iter().collect()
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
    if normalized == "kiro-claude-opus-4.7" {
        return "claude-opus-4.7".to_string();
    }
    if is_kiro_model(&normalized) {
        return normalized;
    }
    if normalized.contains("claude-opus-4.7") {
        return "claude-opus-4.7".to_string();
    }
    if normalized.contains("gpt-5.5") {
        return "gpt-5.5".to_string();
    }
    if normalized.contains("gpt-5.4") {
        return "gpt-5.4".to_string();
    }
    match normalized.as_str() {
        "claude-opus-4-6-thinking" => "claude-opus-4-6".to_string(),
        "claude-sonnet-4.5" | "claude-sonnet-4-5" => "claude-sonnet-4-5-20250929".to_string(),
        "claude-haiku-4.5" | "claude-haiku-4-5" => "claude-haiku-4-5-20251001".to_string(),
        "gpt-5-codex" => "gpt-5".to_string(),
        "gpt-5.3-codex-spark" => "gpt-5.3-codex".to_string(),
        "gpt-5.3-codex" => "gpt-5.2-codex".to_string(),
        other => other.to_string(),
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
    fn calculates_builtin_mimo_v25_pro_cost() {
        let usage = usage(6_200, 33, 100, 50);
        let cost = model_cost_usd("mimo-v2.5-pro", usage);
        let expected = (6_200.0 * 1.0 + 33.0 * 3.0 + 50.0 * 0.2) / MILLION;

        assert!((cost - expected).abs() < 1e-12);
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
    fn resolves_custom_prefixed_models() {
        let codex = candidates_for("custom:gpt-5-codex");
        assert!(codex.contains(&"gpt-5-codex".to_string()));
        assert!(codex.contains(&"gpt-5".to_string()));
        assert!(codex.contains(&"openai/gpt-5".to_string()));

        let factory = candidates_for("custom:OmniMind-GPT-5.5-High-0");
        assert!(factory.contains(&"gpt-5.5".to_string()));
        assert!(factory.contains(&"openai/gpt-5.5".to_string()));

        let kiro = candidates_for("custom:pi-mono-kiro-claude-opus-4.7-3");
        assert!(!kiro.contains(&"claude-opus-4.7".to_string()));
        assert!(!kiro.contains(&"anthropic/claude-opus-4.7".to_string()));

        let pi_kiro = candidates_for("kiro-claude-opus-4.7");
        assert!(pi_kiro.contains(&"claude-opus-4.7".to_string()));
        assert!(pi_kiro.contains(&"anthropic/claude-opus-4.7".to_string()));
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
}
