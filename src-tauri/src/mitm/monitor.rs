// MITM 监控模块
// 复用现有的 ProxyMonitor 和 token_stats 系统

use crate::mitm::parser::{parse_request, parse_response, ParsedResponse};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::collections::{HashMap, VecDeque};

/// MITM 单次拦截日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MitmRequestLog {
    pub id: String,
    pub timestamp: i64,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub duration: u64,
    pub req_headers: HashMap<String, String>,
    pub res_headers: HashMap<String, String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
}

/// MITM 监控器
pub struct MitmMonitor {
    logs: Arc<tokio::sync::RwLock<VecDeque<MitmRequestLog>>>,
    max_logs: usize,
    enabled: AtomicBool,
    speed_stats: Arc<tokio::sync::RwLock<SpeedStats>>,
}

/// 生成速度统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpeedStat {
    /// 模型名称
    pub model: String,
    /// 总输出 token 数
    pub total_output_tokens: u64,
    /// 总生成时间 (ms)
    pub total_duration_ms: u64,
    /// 请求次数
    pub request_count: u64,
    /// 首 token 延迟 (ms)
    pub first_token_latency_ms: Option<u64>,
}

impl SpeedStat {
    /// 计算平均生成速度 (tokens/s)
    pub fn avg_speed(&self) -> f64 {
        if self.total_duration_ms == 0 {
            return 0.0;
        }
        let seconds = self.total_duration_ms as f64 / 1000.0;
        self.total_output_tokens as f64 / seconds
    }

    #[allow(dead_code)]
    pub fn avg_first_token_latency(&self) -> Option<f64> {
        self.first_token_latency_ms
            .map(|l| l as f64)
    }
}

/// 全局速度统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpeedStats {
    /// 按模型分组的统计
    pub by_model: std::collections::HashMap<String, SpeedStat>,
    /// 总统计
    pub total: SpeedStat,
}

impl MitmMonitor {
    pub fn new(max_logs: usize) -> Self {
        Self {
            logs: Arc::new(tokio::sync::RwLock::new(VecDeque::with_capacity(max_logs))),
            max_logs,
            enabled: AtomicBool::new(true),
            speed_stats: Arc::new(tokio::sync::RwLock::new(SpeedStats::default())),
        }
    }

    /// 设置是否启用
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// 是否启用
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 记录请求
    pub async fn log_request(
        &self,
        method: &str,
        url: &str,
        req_headers: HashMap<String, String>,
        res_headers: HashMap<String, String>,
        request_body: Option<&str>,
        response_body: Option<&str>,
        duration_ms: u64,
        status: u16,
    ) {
        if !self.is_enabled() {
            return;
        }

        // 仅作统计用途去用 parser (若不打算在此处统计也可不调用)
        let parsed_req = parse_request(method, url, request_body);
        let parsed_resp = response_body
            .map(|b| parse_response(b))
            .unwrap_or_default();

        // 更新速度统计
        if let Some(model) = &parsed_req.model {
            self.update_speed_stats(model, &parsed_resp, duration_ms).await;
        }

        // 构建 MitmRequestLog 并记录入自己的队列
        let log = MitmRequestLog {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            method: method.to_string(),
            url: url.to_string(),
            status,
            duration: duration_ms,
            req_headers,
            res_headers,
            request_body: request_body.map(|s| s.to_string()),
            response_body: response_body.map(|s| s.to_string()),
        };

        let mut queue = self.logs.write().await;
        if queue.len() >= self.max_logs {
            queue.pop_back(); // 超出上限淘汰最老的数据
        }
        queue.push_front(log);
    }

    /// 获取所有日志
    pub async fn get_logs(&self) -> Vec<MitmRequestLog> {
        let queue = self.logs.read().await;
        queue.iter().cloned().collect()
    }

    /// 清空所有日志
    pub async fn clear_logs(&self) {
        let mut queue = self.logs.write().await;
        queue.clear();
    }

    /// 更新速度统计
    async fn update_speed_stats(
        &self,
        model: &str,
        parsed_resp: &ParsedResponse,
        duration_ms: u64,
    ) {
        let mut stats = self.speed_stats.write().await;
        
        // 更新模型统计
        let model_stat = stats.by_model
            .entry(model.to_string())
            .or_insert_with(|| SpeedStat {
                model: model.to_string(),
                ..Default::default()
            });

        if let Some(output_tokens) = parsed_resp.output_tokens {
            model_stat.total_output_tokens += output_tokens as u64;
            model_stat.total_duration_ms += duration_ms;
            model_stat.request_count += 1;

            // 更新总统计
            stats.total.total_output_tokens += output_tokens as u64;
            stats.total.total_duration_ms += duration_ms;
            stats.total.request_count += 1;
        }
    }

    /// 获取速度统计
    pub async fn get_speed_stats(&self) -> SpeedStats {
        self.speed_stats.read().await.clone()
    }

    /// 清空统计
    pub async fn clear_stats(&self) {
        let mut stats = self.speed_stats.write().await;
        *stats = SpeedStats::default();
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_speed_stat() {
        let mut stat = SpeedStat::default();
        stat.model = "gemini-2.0-flash".to_string();
        stat.total_output_tokens = 1000;
        stat.total_duration_ms = 5000; // 5 seconds
        stat.request_count = 10;

        assert_eq!(stat.avg_speed(), 200.0); // 200 tokens/s
    }
}
