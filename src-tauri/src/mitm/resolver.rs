use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};

/// Pre-resolves target domains to IPs so `should_intercept` can decide
/// whether to MITM a CONNECT-to-IP request without waiting for the inner
/// HTTP request's Host header.
#[derive(Clone)]
pub struct DomainResolver {
    ip_to_domain: Arc<RwLock<HashMap<IpAddr, String>>>,
    target_domains: Vec<String>,
}

impl DomainResolver {
    pub fn new(target_domains: Vec<String>) -> Self {
        Self {
            ip_to_domain: Arc::new(RwLock::new(HashMap::new())),
            target_domains,
        }
    }

    pub async fn resolve_all(&self) {
        let mut map = HashMap::new();
        for domain in &self.target_domains {
            if domain.starts_with("*.") {
                continue;
            }
            match tokio::net::lookup_host(format!("{}:443", domain)).await {
                Ok(addrs) => {
                    for addr in addrs {
                        tracing::info!("[MITM-DNS] {} → {}", domain, addr.ip());
                        map.insert(addr.ip(), domain.clone());
                    }
                }
                Err(e) => {
                    tracing::warn!("[MITM-DNS] 解析失败 {}: {}", domain, e);
                }
            }
        }
        let count = map.len();
        *self.ip_to_domain.write().unwrap() = map;
        tracing::info!(
            "[MITM-DNS] 解析完成: {} 个目标域名 → {} 个 IP 映射",
            self.target_domains.len(),
            count
        );
    }

    pub fn lookup_domain(&self, ip: &IpAddr) -> Option<String> {
        self.ip_to_domain.read().unwrap().get(ip).cloned()
    }

    pub fn is_target_ip(&self, ip: &IpAddr) -> bool {
        self.ip_to_domain.read().unwrap().contains_key(ip)
    }

    pub fn start_refresh_task(self: &Arc<Self>) {
        let resolver = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                tracing::debug!("[MITM-DNS] 定时刷新 IP 映射...");
                resolver.resolve_all().await;
            }
        });
    }
}
