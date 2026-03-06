// MITM 请求解析器
// 解析 Gemini/OpenAI 格式的请求和响应，提取 token 使用信息

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

/// 解析后的请求信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedRequest {
    /// 请求方法
    pub method: String,
    /// 请求 URL
    pub url: String,
    /// 模型名称
    pub model: Option<String>,
    /// 请求体 (格式化后)
    pub request_body: Option<String>,
    /// 协议类型
    pub protocol: Option<String>,
}

/// 解析后的响应信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ParsedResponse {
    /// HTTP 状态码
    pub status: u16,
    /// 输入 token 数
    pub input_tokens: Option<u32>,
    /// 输出 token 数
    pub output_tokens: Option<u32>,
    /// 响应内容摘要
    pub content_summary: Option<String>,
    /// 思维链内容
    pub thinking_content: Option<String>,
    /// 错误信息
    pub error: Option<String>,
}

/// 解析 HTTP 请求
pub fn parse_request(method: &str, url: &str, body: Option<&str>) -> ParsedRequest {
    // 从 URL 提取模型名
    let model_from_url = extract_model_from_url(url);
    
    // 从请求体提取模型名
    let model_from_body = body.and_then(|b| extract_model_from_body(b));
    
    // 模型名优先级: body > url
    let model = model_from_body.or(model_from_url);
    
    // 确定协议类型
    let protocol = determine_protocol(url);
    
    // 格式化请求体
    let request_body = body.and_then(|b| {
        serde_json::from_str::<Value>(b).ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
    });

    ParsedRequest {
        method: method.to_string(),
        url: url.to_string(),
        model,
        request_body,
        protocol,
    }
}

/// 解析 HTTP 响应
pub fn parse_response(response_str: &str) -> ParsedResponse {
    // 分离响应头和响应体
    let (status, body) = split_response(response_str);
    
    let mut result = ParsedResponse {
        status,
        input_tokens: None,
        output_tokens: None,
        content_summary: None,
        thinking_content: None,
        error: None,
    };

    if status >= 400 {
        result.error = extract_error_message(body);
        return result;
    }

    // 检查是否是 SSE 流
    if body.contains("data: ") {
        parse_sse_response(body, &mut result);
    } else if let Ok(json) = serde_json::from_str::<Value>(body) {
        parse_json_response(&json, &mut result);
    }

    result
}

/// 从 URL 提取模型名
fn extract_model_from_url(url: &str) -> Option<String> {
    // Gemini 格式: /v1beta/models/gemini-2.0-flash:generateContent
    if url.contains("/v1beta/models/") {
        return url
            .split("/v1beta/models/")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .map(|s| s.to_string());
    }
    
    // OpenAI 格式: /v1/chat/completions (模型在 body 中)
    None
}

/// 从请求体提取模型名
fn extract_model_from_body(body: &str) -> Option<String> {
    let json = serde_json::from_str::<Value>(body).ok()?;
    json.get("model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}

/// 确定协议类型
fn determine_protocol(url: &str) -> Option<String> {
    if url.contains("/v1/messages") {
        Some("anthropic".to_string())
    } else if url.contains("/v1beta/models") || url.contains("v1internal") {
        Some("gemini".to_string())
    } else if url.starts_with("/v1/") {
        Some("openai".to_string())
    } else {
        None
    }
}

/// 分离响应头和响应体
pub fn split_response(response: &str) -> (u16, &str) {
    // 查找 \r\n\r\n 分隔符
    let header_end = response.find("\r\n\r\n").unwrap_or(0);
    
    let headers = &response[..header_end];
    let body = if header_end > 0 && response.len() >= header_end + 4 {
        &response[header_end + 4..]
    } else {
        response
    };

    let status = extract_status_code(headers).unwrap_or(200);
    (status, body)
}

#[allow(dead_code)]
pub fn extract_headers(content: &str) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    let header_end = content.find("\r\n\r\n").unwrap_or(content.len());
    let header_block = &content[..header_end];
    for line in header_block.lines() {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    headers
}

/// 解压 HTTP 响应体
pub fn decode_response_body(body_bytes: &[u8], headers: &HashMap<String, String>) -> String {
    let encoding = headers.keys().find(|k| k.eq_ignore_ascii_case("content-encoding"))
        .and_then(|k| headers.get(k))
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    let mut decoded = Vec::new();

    if encoding == "gzip" {
        let mut decoder = flate2::read::GzDecoder::new(body_bytes);
        if decoder.read_to_end(&mut decoded).is_ok() {
            return String::from_utf8_lossy(&decoded).to_string();
        }
    } else if encoding == "br" {
        let mut decoder = brotli::Decompressor::new(body_bytes, 4096);
        if decoder.read_to_end(&mut decoded).is_ok() {
            return String::from_utf8_lossy(&decoded).to_string();
        }
    } else if encoding == "deflate" {
        let mut decoder = flate2::read::ZlibDecoder::new(body_bytes);
        if decoder.read_to_end(&mut decoded).is_ok() {
            return String::from_utf8_lossy(&decoded).to_string();
        }
        let mut decoded2 = Vec::new();
        let mut decoder2 = flate2::read::DeflateDecoder::new(body_bytes);
        if decoder2.read_to_end(&mut decoded2).is_ok() {
            return String::from_utf8_lossy(&decoded2).to_string();
        }
    }

    // fallback
    String::from_utf8_lossy(body_bytes).to_string()
}

/// 从响应头中提取状态码
fn extract_status_code(headers: &str) -> Option<u16> {
    headers
        .lines()
        .next()
        .and_then(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            parts.get(1).and_then(|s| s.parse::<u16>().ok())
        })
}

/// 提取错误信息
fn extract_error_message(body: &str) -> Option<String> {
    if let Ok(json) = serde_json::from_str::<Value>(body) {
        if let Some(error) = json.get("error") {
            if let Some(msg) = error.get("message").and_then(|m| m.as_str()) {
                return Some(msg.to_string());
            }
            return Some(error.to_string());
        }
    }
    // 取前 200 字符作为错误信息 (需要注意 UTF-8 字符边界)
    if body.chars().count() > 200 {
        let truncated: String = body.chars().take(200).collect();
        Some(format!("{}...", truncated))
    } else {
        Some(body.to_string())
    }
}

/// 解析 SSE 流式响应
fn parse_sse_response(body: &str, result: &mut ParsedResponse) {
    let mut content = String::new();
    let mut thinking = String::new();
    let mut last_usage: Option<Value> = None;

    for line in body.lines() {
        if !line.starts_with("data: ") {
            continue;
        }

        let json_str = line.trim_start_matches("data: ").trim();
        if json_str == "[DONE]" {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<Value>(json_str) {
            // OpenAI 格式
            if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
                for choice in choices {
                    if let Some(delta) = choice.get("delta") {
                        // 思维链内容
                        if let Some(t) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                            thinking.push_str(t);
                        }
                        // 主要内容
                        if let Some(c) = delta.get("content").and_then(|v| v.as_str()) {
                            content.push_str(c);
                        }
                    }
                }
            }

            // Anthropic 格式
            if let Some(delta) = json.get("delta") {
                if let Some(t) = delta.get("thinking").and_then(|v| v.as_str()) {
                    thinking.push_str(t);
                }
                if let Some(c) = delta.get("text").and_then(|v| v.as_str()) {
                    content.push_str(c);
                }
            }

            // Gemini 格式
            if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
                for candidate in candidates {
                    if let Some(content_obj) = candidate.get("content") {
                        if let Some(parts) = content_obj.get("parts").and_then(|p| p.as_array()) {
                            for part in parts {
                                if let Some(t) = part.get("thought").and_then(|v| v.as_str()) {
                                    thinking.push_str(t);
                                }
                                if let Some(c) = part.get("text").and_then(|v| v.as_str()) {
                                    content.push_str(c);
                                }
                            }
                        }
                    }
                }
            }

            // 提取 usage (可能在流的最后)
            if let Some(usage) = json.get("usage")
                .or(json.get("usageMetadata"))
                .or(json.get("response").and_then(|r| r.get("usage")))
            {
                last_usage = Some(usage.clone());
            }
        }
    }

    // 解析 usage
    if let Some(usage) = last_usage {
        extract_tokens(&usage, result);
    }

    // 设置内容摘要
    if !content.is_empty() {
        result.content_summary = Some(truncate_content(&content, 500));
    }
    
    if !thinking.is_empty() {
        result.thinking_content = Some(truncate_content(&thinking, 500));
    }
}

/// 解析 JSON 响应
fn parse_json_response(json: &Value, result: &mut ParsedResponse) {
    // 提取 usage
    if let Some(usage) = json.get("usage").or(json.get("usageMetadata")) {
        extract_tokens(usage, result);
    }

    // 提取内容
    if let Some(choices) = json.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(message) = choice.get("message") {
                if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
                    result.content_summary = Some(truncate_content(content, 500));
                }
            }
        }
    }

    // Gemini 格式
    if let Some(candidates) = json.get("candidates").and_then(|c| c.as_array()) {
        for candidate in candidates {
            if let Some(content) = candidate.get("content") {
                if let Some(parts) = content.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                            result.content_summary = Some(truncate_content(text, 500));
                        }
                    }
                }
            }
        }
    }
}

/// 从 usage 对象提取 token 数
fn extract_tokens(usage: &Value, result: &mut ParsedResponse) {
    // OpenAI 格式
    result.input_tokens = usage
        .get("prompt_tokens")
        .or(usage.get("input_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    result.output_tokens = usage
        .get("completion_tokens")
        .or(usage.get("output_tokens"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    // Gemini 格式
    if result.input_tokens.is_none() {
        result.input_tokens = usage
            .get("promptTokenCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
    }

    if result.output_tokens.is_none() {
        result.output_tokens = usage
            .get("candidatesTokenCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
    }

    // 如果只有 total，则设置为 output
    if result.input_tokens.is_none() && result.output_tokens.is_none() {
        result.output_tokens = usage
            .get("total_tokens")
            .or(usage.get("totalTokenCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
    }
}

/// 截断内容
fn truncate_content(content: &str, max_len: usize) -> String {
    if content.chars().count() <= max_len {
        content.to_string()
    } else {
        let truncated: String = content.chars().take(max_len).collect();
        format!("{}... [{} 字符]", truncated, content.chars().count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let url = "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse";
        let body = r#"{"model": "gemini-2.0-flash", "messages": []}"#;
        
        let parsed = parse_request("POST", url, Some(body));
        assert_eq!(parsed.model, Some("gemini-2.0-flash".to_string()));
        assert_eq!(parsed.protocol, Some("gemini".to_string()));
    }

    #[test]
    fn test_parse_sse_response() {
        let response = "HTTP/1.1 200 OK\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\n";
        let parsed = parse_response(response);
        
        assert_eq!(parsed.status, 200);
        assert_eq!(parsed.input_tokens, Some(10));
        assert_eq!(parsed.output_tokens, Some(5));
    }
}
