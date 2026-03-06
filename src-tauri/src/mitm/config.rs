// MITM 代理配置
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// MITM 代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MitmConfig {
    /// 是否启用 MITM 代理
    pub enabled: bool,
    /// 代理监听端口
    pub port: u16,
    /// Root CA 证书路径 (PEM 格式)
    pub root_ca_path: PathBuf,
    /// Root CA 私钥路径 (PEM 格式)
    pub root_ca_key_path: PathBuf,
    /// 目标域名列表 (支持通配符)
    pub target_domains: Vec<String>,
    /// 是否启用请求日志
    pub enable_logging: bool,
    /// 最大日志保留数量
    pub max_logs: usize,
}

impl Default for MitmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8081,
            root_ca_path: PathBuf::from("root_ca.pem"),
            root_ca_key_path: PathBuf::from("root_ca_key.pem"),
            target_domains: vec![
                "daily-cloudcode-pa.googleapis.com".to_string(),
                "cloudcode-pa.googleapis.com".to_string(),
                "daily-cloudcode-pa.sandbox.googleapis.com".to_string(),
            ],
            enable_logging: true,
            max_logs: 1000,
        }
    }
}

impl MitmConfig {
    #[allow(dead_code)]
    pub fn get_bind_address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    #[allow(dead_code)]
    pub fn is_target_domain(&self, domain: &str) -> bool {
        // 移除端口号
        let domain_without_port = domain.split(':').next().unwrap_or(domain);
        
        for target in &self.target_domains {
            // 精确匹配
            if domain_without_port == target {
                return true;
            }
            // 通配符匹配 (例如 *.googleapis.com)
            if target.starts_with("*.") {
                let suffix = &target[2..];
                if domain_without_port.ends_with(suffix) {
                    return true;
                }
            }
        }
        false
    }
}

/// MITM 代理服务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MitmStatus {
    pub running: bool,
    pub port: u16,
    pub proxy_url: String,
    pub requests_processed: u64,
    pub cert_cache_size: usize,
    pub enable_monitoring: bool,
    pub target_domains: Vec<String>,
}

impl Default for MitmStatus {
    fn default() -> Self {
        Self {
            running: false,
            port: 8081,
            proxy_url: "http://127.0.0.1:8081".to_string(),
            requests_processed: 0,
            cert_cache_size: 0,
            enable_monitoring: true,
            target_domains: Vec::new(),
        }
    }
}
