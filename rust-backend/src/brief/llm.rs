use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SectionSummary {
    pub headline: String,
    pub bullets: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

pub fn summarize_source(
    config: &LlmConfig,
    source_label: &str,
    extract_payload: &Value,
) -> Result<SectionSummary, String> {
    summarize_project(config, source_label, "General", extract_payload)
}

pub fn summarize_project(
    config: &LlmConfig,
    source_label: &str,
    project_label: &str,
    extract_payload: &Value,
) -> Result<SectionSummary, String> {
    let system = format!(
        "你是 Token Usage Dashboard 的 Daily Brief 助手。根据给定 CLI（{source_label}）在项目「{project_label}」当日会话的标题与用户原文，用中文写一份短卡片叙事。\
只依据提供的 userTexts/title，禁止编造未出现的事实。输出严格 JSON：{{\"headline\":\"...\",\"bullets\":[\"...\"]}}。\
headline 一句概括该项目今天做了什么；bullets 3 到 5 条，每条短句。"
    );
    let user = serde_json::to_string_pretty(extract_payload)
        .map_err(|err| format!("failed to serialize extract payload: {err}"))?;
    let content = chat_completion_content(config, &system, &user)?;
    parse_section_summary(&content)
}

/// Summarizes one hour of activity into a single headline. When the hour
/// spans multiple projects, the model is asked to distinguish them inline,
/// e.g. "在 summer 项目上，…；在 token-usage 项目上，…".
pub fn summarize_hour(
    config: &LlmConfig,
    hour: i64,
    extract_payload: &Value,
) -> Result<String, String> {
    let system = format!(
        "你是 Token Usage Dashboard 的 Daily Brief 助手。根据用户在 {}（{}:00–{}:59）各 CLI 会话的标题与用户原文，用中文概括这一小时用户在做什么。\
只依据提供的 titles/userTexts，禁止编造未出现的事实。这些 userTexts 已按本小时过滤，不要把其他时段的工作写进来。\
输出严格 JSON：{{\"headline\":\"...\"}}。\
headline 为一段不超过 60 字的短句。若该小时涉及多个项目，按项目分段，每段以「在 <项目名> 项目上，…」开头，段间用「；」连接；只有一个项目时不必显式点名。",
        hour_label(hour),
        hour,
        hour
    );
    let user = serde_json::to_string_pretty(extract_payload)
        .map_err(|err| format!("failed to serialize extract payload: {err}"))?;
    let content = chat_completion_content(config, &system, &user)?;
    parse_hour_summary(&content)
}

fn hour_label(hour: i64) -> String {
    let part = match hour {
        0..=5 => "凌晨",
        6..=8 => "早晨",
        9..=11 => "上午",
        12..=13 => "中午",
        14..=17 => "下午",
        _ => "晚上",
    };
    format!("{part} {hour} 点时段")
}

fn parse_hour_summary(content: &str) -> Result<String, String> {
    let trimmed = content.trim();
    let json_text = extract_json_object(trimmed).unwrap_or(trimmed);
    let parsed: Value = serde_json::from_str(json_text).map_err(|err| {
        format!(
            "failed to parse hour JSON: {err}; body={}",
            truncate(trimmed, 300)
        )
    })?;
    parsed
        .get("headline")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|headline| !headline.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "LLM returned empty hour headline".to_string())
}

fn chat_completion_content(
    config: &LlmConfig,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let body = json!({
        "model": config.model_id,
        "temperature": 0.2,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ]
    });

    let url = chat_completions_url(&config.base_url);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|err| format!("failed to build HTTP client: {err}"))?;

    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .map_err(|err| format!("LLM request failed: {err}"))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|err| format!("failed to read LLM response: {err}"))?;
    if !status.is_success() {
        return Err(format!("LLM HTTP {status}: {}", truncate(&text, 400)));
    }

    let completion: ChatCompletionResponse = serde_json::from_str(&text)
        .map_err(|err| format!("failed to parse LLM response: {err}"))?;
    completion
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_deref())
        .map(ToString::to_string)
        .ok_or_else(|| "LLM response missing content".to_string())
}

fn parse_section_summary(content: &str) -> Result<SectionSummary, String> {
    let trimmed = content.trim();
    let json_text = extract_json_object(trimmed).unwrap_or(trimmed);
    let mut summary: SectionSummary = serde_json::from_str(json_text)
        .map_err(|err| format!("failed to parse section JSON: {err}; body={}", truncate(trimmed, 300)))?;
    summary.headline = summary.headline.trim().to_string();
    summary.bullets = summary
        .bullets
        .into_iter()
        .map(|bullet| bullet.trim().to_string())
        .filter(|bullet| !bullet.is_empty())
        .collect();
    if summary.headline.is_empty() {
        return Err("LLM returned empty headline".to_string());
    }
    if summary.bullets.is_empty() {
        summary.bullets.push("今日有活动，但未能提炼出要点。".to_string());
    }
    Ok(summary)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let mut depth = 0i32;
    for (offset, character) in text[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..start + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

fn chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    for (index, character) in text.chars().enumerate() {
        if index >= max_chars {
            result.push('…');
            break;
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_object_from_fenced_content() {
        let content = "```json\n{\"headline\":\"你好\",\"bullets\":[\"一\",\"二\"]}\n```";
        let summary = parse_section_summary(content).unwrap();
        assert_eq!(summary.headline, "你好");
        assert_eq!(summary.bullets, vec!["一", "二"]);
    }

    #[test]
    fn parses_hour_headline_tolerantly() {
        let content = "结果：```json\n{\"headline\":\"调试后端服务\"}\n```";
        assert_eq!(parse_hour_summary(content).unwrap(), "调试后端服务");
        assert!(parse_hour_summary("{\"headline\":\"  \"}").is_err());
    }

    #[test]
    fn builds_chat_completions_url() {
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8317/v1"),
            "http://127.0.0.1:8317/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("http://127.0.0.1:8317/v1/chat/completions"),
            "http://127.0.0.1:8317/v1/chat/completions"
        );
    }
}
