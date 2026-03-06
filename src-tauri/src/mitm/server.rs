use crate::mitm::cert::RootCA;
use crate::mitm::config::{MitmConfig, MitmStatus};
use crate::mitm::monitor::MitmMonitor;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MitmServiceInstance {
    pub config: MitmConfig,
    pub monitor: Arc<MitmMonitor>,
    pub ca: crate::mitm::cert::AntigravityAuthority,
    pub proxy_tx: Arc<tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
    pub requests_processed: Arc<std::sync::atomic::AtomicUsize>,
    pub resolver: Arc<crate::mitm::resolver::DomainResolver>,
}

pub struct MitmServiceState {
    pub instance: Arc<RwLock<Option<Arc<MitmServiceInstance>>>>,
}

impl MitmServiceState {
    pub fn new() -> Self {
        Self {
            instance: Arc::new(RwLock::new(None)),
        }
    }
}

impl Default for MitmServiceState {
    fn default() -> Self {
        Self::new()
    }
}

fn make_handler(
    monitor: &Arc<MitmMonitor>,
    config: &MitmConfig,
    counter: &Arc<std::sync::atomic::AtomicUsize>,
    resolver: &Arc<crate::mitm::resolver::DomainResolver>,
) -> crate::mitm::proxy::AntigravityHttpHandler {
    crate::mitm::proxy::AntigravityHttpHandler::new(
        monitor.clone(),
        config.clone(),
        counter.clone(),
        resolver.clone(),
    )
}

pub async fn start_mitm_service(
    config: MitmConfig,
) -> Result<Arc<MitmServiceInstance>, String> {
    if !config.root_ca_path.exists() {
        return Err(format!(
            "Root CA 证书文件不存在: {}",
            config.root_ca_path.display()
        ));
    }

    if !config.root_ca_key_path.exists() {
        return Err(format!(
            "Root CA 私钥文件不存在: {}",
            config.root_ca_key_path.display()
        ));
    }

    tracing::info!(
        "[MITM] 加载 Root CA: {}",
        config.root_ca_path.display()
    );

    let root_ca = RootCA::load_from_pem(&config.root_ca_path, &config.root_ca_key_path)?;
    let ca = root_ca.into_authority();
    let ca_for_instance = ca.clone();

    let mitm_monitor = Arc::new(MitmMonitor::new(config.max_logs));
    mitm_monitor.set_enabled(config.enable_logging);

    use std::net::SocketAddr;
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

    let listener = std::net::TcpListener::bind(addr).map_err(|e| {
        format!("端口 {} 已被占用或无法绑定: {}", config.port, e)
    })?;
    drop(listener);

    let resolver = Arc::new(crate::mitm::resolver::DomainResolver::new(
        config.target_domains.clone(),
    ));
    resolver.resolve_all().await;
    resolver.start_refresh_task();

    let processed_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handler = make_handler(&mitm_monitor, &config, &processed_counter, &resolver);

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();

    let upstream = get_upstream_proxy();

    if let Some(ref proxy_url) = upstream {
        if proxy_url.starts_with("socks5://") || proxy_url.starts_with("socks5h://") {
            tracing::info!("[MITM] 上游代理 (SOCKS5): {}", proxy_url);

            let socks = crate::mitm::connector::Socks5Connector::new(proxy_url);
            let https = hyper_rustls_mitm::HttpsConnectorBuilder::new()
                .with_webpki_roots()
                .https_or_http()
                .enable_all_versions()
                .wrap_connector(socks);

            let client = hyper_util::client::legacy::Client::builder(
                hyper_util::rt::TokioExecutor::new(),
            )
            .build(https);

            let proxy = hudsucker::Proxy::builder()
                .with_addr(addr)
                .with_client(client)
                .with_ca(ca)
                .with_http_handler(handler)
                .build();

            tokio::spawn(async move {
                tracing::info!("[MITM] 启动 Hudsucker (SOCKS5), 监听 {}", addr);
                tokio::select! {
                    result = proxy.start() => {
                        if let Err(e) = result {
                            tracing::error!("[MITM] Hudsucker 代理异常终止: {:?}", e);
                        }
                    }
                    _ = rx => {
                        tracing::info!("[MITM] 收到关闭信号，Hudsucker 代理将停止");
                    }
                }
            });
        } else {
            tracing::warn!("[MITM] 不支持的上游代理协议: {}，回退为直连", proxy_url);
            spawn_direct_proxy(addr, ca, handler, rx);
        }
    } else {
        tracing::info!("[MITM] 上游代理: 无 (直连)");
        spawn_direct_proxy(addr, ca, handler, rx);
    }

    let instance = Arc::new(MitmServiceInstance {
        config,
        monitor: mitm_monitor,
        ca: ca_for_instance,
        proxy_tx: Arc::new(tokio::sync::Mutex::new(Some(tx))),
        requests_processed: processed_counter,
        resolver,
    });

    Ok(instance)
}

fn spawn_direct_proxy(
    addr: std::net::SocketAddr,
    ca: crate::mitm::cert::AntigravityAuthority,
    handler: crate::mitm::proxy::AntigravityHttpHandler,
    rx: tokio::sync::oneshot::Receiver<()>,
) {
    let proxy = hudsucker::Proxy::builder()
        .with_addr(addr)
        .with_rustls_client()
        .with_ca(ca)
        .with_http_handler(handler)
        .build();

    tokio::spawn(async move {
        tracing::info!("[MITM] 启动 Hudsucker (直连), 监听 {}", addr);
        tokio::select! {
            result = proxy.start() => {
                if let Err(e) = result {
                    tracing::error!("[MITM] Hudsucker 代理异常终止: {:?}", e);
                }
            }
            _ = rx => {
                tracing::info!("[MITM] 收到关闭信号，Hudsucker 代理将停止");
            }
        }
    });
}

fn get_upstream_proxy() -> Option<String> {
    if let Ok(config) = crate::modules::config::load_app_config() {
        let proxy = &config.proxy.upstream_proxy;
        if proxy.enabled && !proxy.url.is_empty() {
            return Some(proxy.url.clone());
        }
    }
    None
}

/// 获取 MITM 服务状态
pub async fn get_mitm_status(instance: &Option<Arc<MitmServiceInstance>>) -> MitmStatus {
    match instance {
        Some(inst) => {
            let requests_processed = inst.requests_processed.load(std::sync::atomic::Ordering::Relaxed) as u64;
            let cert_cache_size = inst.ca.cache_size();

            MitmStatus {
                running: true,
                port: inst.config.port,
                proxy_url: format!("http://127.0.0.1:{}", inst.config.port),
                requests_processed,
                cert_cache_size,
                enable_monitoring: inst.monitor.is_enabled(),
                target_domains: inst.config.target_domains.clone(),
            }
        }
        None => MitmStatus::default(),
    }
}

pub async fn stop_mitm_service(instance: &mut Option<Arc<MitmServiceInstance>>) {
    if let Some(inst) = instance.take() {
        if let Some(tx) = inst.proxy_tx.lock().await.take() {
            let _ = tx.send(());
        }
        tracing::info!("[MITM] 服务已停止");
    }
}

/// 验证 Root CA 证书
pub fn validate_root_ca(cert_path: &PathBuf, key_path: &PathBuf) -> Result<(), String> {
    if !cert_path.exists() {
        return Err(format!("证书文件不存在: {}", cert_path.display()));
    }

    if !key_path.exists() {
        return Err(format!("私钥文件不存在: {}", key_path.display()));
    }

    // 尝试加载证书
    RootCA::load_from_pem(cert_path, key_path)?;
    
    Ok(())
}

/// 生成自签名 Root CA (用于测试)
#[cfg(test)]
pub fn generate_test_root_ca() -> Result<(Vec<u8>, Vec<u8>), String> {
    use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, IsCa, BasicConstraints};

    let mut params = CertificateParams::new(vec!["Antigravity MITM CA".to_string()])
        .map_err(|e| format!("创建 CA 参数失败: {}", e))?;

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Antigravity MITM CA");
    dn.push(DnType::OrganizationName, "Antigravity");
    params.distinguished_name = dn;

    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        rcgen::KeyUsagePurpose::KeyCertSign,
        rcgen::KeyUsagePurpose::CrlSign,
    ];

    let key_pair = KeyPair::generate()
        .map_err(|e| format!("生成密钥对失败: {}", e))?;

    let cert = params.self_signed(&key_pair)
        .map_err(|e| format!("签发证书失败: {}", e))?;

    Ok((cert.der().to_vec(), key_pair.serialize_der()))
}
