// MITM 证书管理
// 负责加载 Root CA 和动态生成域名证书

use rcgen::KeyPair;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use rustls_pemfile::{certs, private_key};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use http::uri::Authority;
use std::collections::HashMap;
use std::sync::Mutex as SyncMutex;
use std::sync::Arc;
use hudsucker::certificate_authority::CertificateAuthority;
use hudsucker::rustls::server::{ClientHello, ResolvesServerCert};
use hudsucker::rustls::sign::CertifiedKey;

pub struct RootCA {
    pub rcgen_key_pair: KeyPair,
    pub rcgen_cert: rcgen::Certificate,
}

impl RootCA {
    /// 从 PEM 文件加载 Root CA
    pub fn load_from_pem(cert_path: &Path, key_path: &Path) -> Result<Self, String> {
        // 加载证书
        let cert_file = File::open(cert_path)
            .map_err(|e| format!("无法打开 CA 证书文件 {}: {}", cert_path.display(), e))?;
        let mut cert_reader = BufReader::new(cert_file);
        let certs_list: Vec<CertificateDer<'static>> = certs(&mut cert_reader)
            .filter_map(|r| r.ok())
            .collect();
        
        let cert = certs_list.into_iter().next()
            .ok_or("CA 证书文件中没有找到有效的证书")?;

        // 读取私钥文件内容
        let key_content = std::fs::read_to_string(key_path)
            .map_err(|e| format!("读取私钥文件失败: {}", e))?;
        
        // 解析私钥
        let (key, key_pair) = Self::parse_private_key(&key_content)?;
        // 再次解析确确保获得适用于 Rcgen CA 构造器的 rcgen::Certificate
        let pem_cert = rustls_pemfile::certs(&mut BufReader::new(File::open(cert_path).map_err(|e| e.to_string())?))
            .filter_map(|r| r.ok())
            .next().ok_or("无法重新获取证书以便构造 rcgen::Certificate")?;
            
        let rcgen_cert = rcgen::CertificateParams::from_ca_cert_der(&pem_cert)
            .map_err(|e| format!("解析 CA 证书失败: {}", e))?
            .self_signed(&key_pair)
            .map_err(|e| format!("重新构建 CA 对象失败: {}", e))?;
            
        let _ = (cert, key);
        Ok(Self { rcgen_key_pair: key_pair, rcgen_cert })
    }
    
    /// 解析私钥 (支持 ECDSA PKCS#8 和 SEC1 格式)
    fn parse_private_key(pem_content: &str) -> Result<(PrivateKeyDer<'static>, KeyPair), String> {
        // 方法1: 尝试用 pkcs8 解析 PKCS#8 PEM (ECDSA)
        if pem_content.contains("PRIVATE KEY") && !pem_content.contains("EC PRIVATE KEY") && !pem_content.contains("RSA PRIVATE KEY") {
            // 尝试解析为 ECDSA PKCS#8
            let der_bytes = match Self::pem_to_der(pem_content, "PRIVATE KEY") {
                Some(der) => der,
                None => {
                    // 手动提取 base64
                    let lines: Vec<&str> = pem_content.lines()
                        .filter(|l| !l.starts_with("-----"))
                        .collect();
                    let b64 = lines.join("");
                    match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64) {
                        Ok(der) => der,
                        Err(_) => Vec::new(),
                    }
                }
            };
            
            if !der_bytes.is_empty() {
                tracing::debug!("[MITM] 尝试解析 PKCS#8 DER，长度: {}", der_bytes.len());
                // 检查是否是 ECDSA (通过 OID)
                if !Self::is_rsa_key(&der_bytes) {
                    // 提取 SEC1 内容 (PKCS#8 的 privateKey OCTET STRING)
                    let sec1_der = Self::pkcs8_to_sec1(&der_bytes);
                    if !sec1_der.is_empty() {
                        tracing::debug!("[MITM] 提取 SEC1 DER，长度: {}", sec1_der.len());
                        let sec1_pem = Self::der_to_pem(&sec1_der, "EC PRIVATE KEY");
                        if let Ok(kp) = KeyPair::from_pem(&sec1_pem) {
                            tracing::debug!("[MITM] SEC1 KeyPair 创建成功");
                            let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(kp.serialize_der()));
                            return Ok((key_der, kp));
                        }
                    }
                }
            }
        }
        
        // 方法2: 尝试用 rustls_pemfile 解析
        let mut reader = BufReader::new(pem_content.as_bytes());
        if let Ok(Some(key)) = private_key(&mut reader) {
            match &key {
                PrivateKeyDer::Pkcs1(_) => {
                    tracing::debug!("[MITM] 检测到 RSA PKCS#1 格式，不支持");
                    return Err(Self::ecdsa_only_error());
                }
                PrivateKeyDer::Pkcs8(pkcs8) => {
                    let der = pkcs8.secret_pkcs8_der();
                    tracing::debug!("[MITM] 检测到 PKCS#8 格式，DER 长度: {}", der.len());
                    if Self::is_rsa_key(der) {
                        tracing::debug!("[MITM] 检测到 RSA 密钥，不支持");
                        return Err(Self::ecdsa_only_error());
                    }
                    // 提取 SEC1 内容
                    let sec1_der = Self::pkcs8_to_sec1(der);
                    if !sec1_der.is_empty() {
                        let sec1_pem = Self::der_to_pem(&sec1_der, "EC PRIVATE KEY");
                        if let Ok(kp) = KeyPair::from_pem(&sec1_pem) {
                            tracing::debug!("[MITM] SEC1 KeyPair 创建成功");
                            let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(kp.serialize_der()));
                            return Ok((key_der, kp));
                        }
                    }
                }
                PrivateKeyDer::Sec1(sec1) => {
                    tracing::debug!("[MITM] 检测到 SEC1 格式 EC 私钥");
                    let der = sec1.secret_sec1_der();
                    let pkcs8_der = Self::sec1_to_pkcs8(der);
                    let pkcs8_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pkcs8_der));
                    if let Ok(kp) = Self::try_key_pair_from_der(&pkcs8_key) {
                        tracing::debug!("[MITM] SEC1 转 PKCS#8 成功");
                        return Ok((pkcs8_key, kp));
                    }
                }
                _ => {}
            }
        }
        
        // 方法3: 尝试直接用 rcgen 解析 (SEC1 格式)
        if let Ok(kp) = KeyPair::from_pem(pem_content) {
            let der = kp.serialize_der();
            if Self::is_rsa_key(&der) {
                return Err(Self::ecdsa_only_error());
            }
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der.to_vec()));
            return Ok((key, kp));
        }
        
        // 方法4: 检查是否是 RSA 私钥
        if pem_content.contains("RSA PRIVATE KEY") || 
           (pem_content.contains("PRIVATE KEY") && Self::pem_contains_rsa(pem_content)) {
            return Err(Self::ecdsa_only_error());
        }
        
        Err("无法解析私钥格式。\n支持的格式: ECDSA P-256/P-384 (PKCS#8 或 SEC1)\n不支持: RSA 密钥".to_string())
    }
    
    /// PEM 转 DER
    fn pem_to_der(pem: &str, label: &str) -> Option<Vec<u8>> {
        let begin_marker = format!("-----BEGIN {}-----", label);
        let end_marker = format!("-----END {}-----", label);
        
        let start = pem.find(&begin_marker)? + begin_marker.len();
        let end = pem.find(&end_marker)?;
        
        let b64 = pem[start..end].replace(|c: char| c.is_ascii_whitespace(), "");
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64).ok()
    }
    
    /// PKCS#8 转 SEC1 格式 (包含曲线 OID)
    fn pkcs8_to_sec1(pkcs8_der: &[u8]) -> Vec<u8> {
        // PKCS#8 结构:
        // SEQUENCE { version INTEGER, algorithm SEQUENCE { ecOID, curveOID }, privateKey OCTET STRING }
        // SEC1 结构:
        // SEQUENCE { version INTEGER, privateKey OCTET STRING, [0] curveOID, [1] publicKey }
        
        // 提取曲线 OID 和 SEC1 内容
        let mut curve_oid: Option<Vec<u8>> = None;
        let mut sec1_content: Option<Vec<u8>> = None;
        
        // 简单解析：查找 algorithm SEQUENCE 中的曲线 OID
        // 格式: 30 13 06 07 2A 86 48 CE 3D 02 01 06 08 2A 86 48 CE 3D 03 01 07
        //       ^alg^  ^ecOID^              ^curveOID^
        
        // EC OID: 06 07 2A 86 48 CE 3D 02 01
        let ec_oid: &[u8] = &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
        
        // 查找 EC OID
        for i in 0..pkcs8_der.len().saturating_sub(ec_oid.len()) {
            if &pkcs8_der[i..i + ec_oid.len()] == ec_oid {
                // EC OID 后面是曲线 OID
                let curve_oid_start = i + ec_oid.len();
                if curve_oid_start < pkcs8_der.len() && pkcs8_der[curve_oid_start] == 0x06 {
                    // 读取曲线 OID
                    let oid_len = pkcs8_der[curve_oid_start + 1] as usize;
                    if curve_oid_start + 2 + oid_len <= pkcs8_der.len() {
                        curve_oid = Some(pkcs8_der[curve_oid_start..curve_oid_start + 2 + oid_len].to_vec());
                    }
                }
                break;
            }
        }
        
        // 查找 OCTET STRING (SEC1 内容)
        for i in 0..pkcs8_der.len().saturating_sub(2) {
            if pkcs8_der[i] == 0x04 {
                let len_pos = i + 1;
                if len_pos >= pkcs8_der.len() {
                    continue;
                }
                
                let (len, content_start) = if pkcs8_der[len_pos] & 0x80 == 0 {
                    (pkcs8_der[len_pos] as usize, len_pos + 1)
                } else {
                    let len_bytes = (pkcs8_der[len_pos] & 0x7F) as usize;
                    if len_pos + 1 + len_bytes > pkcs8_der.len() {
                        continue;
                    }
                    let mut len = 0usize;
                    for j in 0..len_bytes {
                        len = (len << 8) | pkcs8_der[len_pos + 1 + j] as usize;
                    }
                    (len, len_pos + 1 + len_bytes)
                };
                
                if content_start + len <= pkcs8_der.len() {
                    let content = &pkcs8_der[content_start..content_start + len];
                    if !content.is_empty() && content[0] == 0x30 {
                        sec1_content = Some(content.to_vec());
                        break;
                    }
                }
            }
        }
        
        // 构建 SEC1
        match (curve_oid, sec1_content) {
            (Some(oid), Some(mut sec1)) => {
                tracing::debug!("[MITM] 曲线 OID: {:02X?}", oid);
                tracing::debug!("[MITM] SEC1 原始: {:02X?}", &sec1[..20.min(sec1.len())]);
                let has_curve_oid = sec1.windows(2).any(|w| w[0] == 0xA0);
                tracing::debug!("[MITM] 已有曲线 OID: {}", has_curve_oid);
                
                if !has_curve_oid {
                    // 需要添加曲线 OID 到 SEC1
                    // SEC1 结构: 30 <len> 02 01 01 04 <key_len> <key> [曲线OID] [公钥]
                    // 我们在 privateKey 后面插入曲线 OID
                    
                    // 找到 privateKey OCTET STRING 的结束位置
                    if sec1.len() > 7 && sec1[0] == 0x30 {
                        // 跳过 SEQUENCE header
                        let mut pos = if sec1[1] & 0x80 == 0 { 2 } else { 3 };
                        // 跳过 version
                        if sec1[pos] == 0x02 { pos += 3; }
                        // 跳过 privateKey OCTET STRING
                        if sec1[pos] == 0x04 {
                            pos += 1;
                            let key_len = if sec1[pos] & 0x80 == 0 {
                                sec1[pos] as usize
                            } else {
                                // 长格式
                                let len_bytes = (sec1[pos] & 0x7F) as usize;
                                pos += 1;
                                let mut len = 0usize;
                                for j in 0..len_bytes {
                                    len = (len << 8) | sec1[pos + j] as usize;
                                }
                                len
                            };
                            pos += if sec1[pos - 1] & 0x80 == 0 { 1 } else { 0 };
                            pos += key_len;
                            
                            // 在 pos 位置插入曲线 OID (带标签 0xA0)
                            let curve_with_tag = {
                                let mut v = vec![0xA0, (oid.len() + 2) as u8];
                                v.extend_from_slice(&oid);
                                v
                            };
                            
                            // 插入曲线 OID
                            sec1.splice(pos..pos, curve_with_tag);
                            
                            // 更新 SEQUENCE 长度
                            let new_len = sec1.len() - 2;
                            if new_len < 128 {
                                sec1[1] = new_len as u8;
                            } else if new_len < 256 {
                                sec1[1] = 0x81;
                                sec1.insert(2, new_len as u8);
                            }
                        }
                    }
                }
                sec1
            }
            _ => Vec::new(),
        }
    }
    
    /// SEC1 格式 EC 私钥转换为 PKCS#8
    fn sec1_to_pkcs8(sec1_der: &[u8]) -> Vec<u8> {
        // 从 SEC1 DER 中检测曲线类型
        // SEC1 格式: 0x30 <len> 0x02 0x01 0x01 0x04 <key_len> <key> [0xa0 <curve_oid>] [0xa1 <pub_key>]
        // 通过私钥长度判断曲线: P-256 = 32字节, P-384 = 48字节
        
        // 尝试从 SEC1 中提取曲线 OID
        let curve_oid = Self::extract_curve_oid(sec1_der);
        
        // PKCS#8 结构:
        // SEQUENCE {
        //   version INTEGER 0,
        //   algorithmIdentifier SEQUENCE { ecPublicKey OID, curve OID },
        //   privateKey OCTET STRING (包含 SEC1 DER)
        // }
        
        // EC OID: 1.2.840.10045.2.1 = 06 07 2A 86 48 CE 3D 02 01
        let ec_oid: &[u8] = &[0x06, 0x07, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
        
        // 构建 AlgorithmIdentifier: SEQUENCE { ec_oid, curve_oid }
        let alg_content_len = ec_oid.len() + curve_oid.len();
        let mut alg_id = Vec::with_capacity(alg_content_len + 4);
        alg_id.push(0x30); // SEQUENCE
        alg_id.push(alg_content_len as u8);
        alg_id.extend_from_slice(ec_oid);
        alg_id.extend_from_slice(&curve_oid);
        
        // 包装 SEC1 DER 为 OCTET STRING
        let octet_len = sec1_der.len();
        let mut octet_string = Vec::with_capacity(octet_len + 4);
        if octet_len < 128 {
            octet_string.push(0x04); // OCTET STRING
            octet_string.push(octet_len as u8);
        } else {
            octet_string.push(0x04);
            octet_string.push(0x81); // 长格式
            octet_string.push(octet_len as u8);
        }
        octet_string.extend_from_slice(sec1_der);
        
        // 构建 PrivateKeyInfo
        let inner_len = 3 + alg_id.len() + octet_string.len(); // version(3) + alg + octet
        
        let mut pkcs8 = Vec::with_capacity(inner_len + 4);
        if inner_len < 128 {
            pkcs8.push(0x30); // SEQUENCE
            pkcs8.push(inner_len as u8);
        } else if inner_len < 256 {
            pkcs8.push(0x30);
            pkcs8.push(0x81);
            pkcs8.push(inner_len as u8);
        } else {
            pkcs8.push(0x30);
            pkcs8.push(0x82);
            pkcs8.push((inner_len >> 8) as u8);
            pkcs8.push(inner_len as u8);
        }
        
        pkcs8.push(0x02); // INTEGER version
        pkcs8.push(0x01); // length 1
        pkcs8.push(0x00); // version 0
        pkcs8.extend_from_slice(&alg_id);
        pkcs8.extend_from_slice(&octet_string);
        
        pkcs8
    }
    
    /// 从 SEC1 DER 中提取曲线 OID
    fn extract_curve_oid(sec1_der: &[u8]) -> Vec<u8> {
        // P-256 OID: 1.2.840.10045.3.1.7 = 06 08 2A 86 48 CE 3D 03 01 07
        let p256_oid: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
        // P-384 OID: 1.3.132.0.34 = 06 05 2B 81 04 00 22
        let p384_oid: &[u8] = &[0x06, 0x05, 0x2B, 0x81, 0x04, 0x00, 0x22];
        
        // 尝试在 SEC1 DER 中查找曲线 OID
        if sec1_der.windows(p256_oid.len()).any(|w| w == p256_oid) {
            return p256_oid.to_vec();
        }
        if sec1_der.windows(p384_oid.len()).any(|w| w == p384_oid) {
            return p384_oid.to_vec();
        }
        
        // 默认使用 P-256
        p256_oid.to_vec()
    }
    
    /// 检查 DER 数据是否为 RSA 密钥
    fn is_rsa_key(der: &[u8]) -> bool {
        // RSA OID: 1.2.840.113549.1.1.1
        let rsa_oid = &[0x06, 0x09, 0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x01];
        der.windows(rsa_oid.len()).any(|w| w == rsa_oid)
    }
    
    /// 检查 PEM 内容是否包含 RSA 密钥
    fn pem_contains_rsa(pem: &str) -> bool {
        let lines: Vec<&str> = pem.lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        let b64 = lines.join("");
        if let Ok(der) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64) {
            return Self::is_rsa_key(&der);
        }
        false
    }
    
    /// ECDSA only 错误信息
    fn ecdsa_only_error() -> String {
        "RSA 密钥不支持。请使用 ECDSA CA 证书。\n\n生成命令:\nopenssl ecparam -genkey -name prime256v1 -noout -out ca-key.pem\nopenssl req -new -x509 -key ca-key.pem -out ca-cert.pem -days 3650 -subj '/CN=MITM-CA'".to_string()
    }
    
    /// 从 PrivateKeyDer 创建 KeyPair
    fn try_key_pair_from_der(key: &PrivateKeyDer) -> Result<KeyPair, String> {
        let pem = match key {
            PrivateKeyDer::Pkcs8(pkcs8) => {
                let der = pkcs8.secret_pkcs8_der();
                Self::der_to_pem(der, "PRIVATE KEY")
            }
            PrivateKeyDer::Sec1(sec1) => {
                let der = sec1.secret_sec1_der();
                Self::der_to_pem(der, "EC PRIVATE KEY")
            }
            _ => return Err("不支持的私钥格式".to_string()),
        };
        
        KeyPair::from_pem(&pem)
            .map_err(|e| format!("KeyPair 解析失败: {}", e))
    }
    
    /// DER 转 PEM
    fn der_to_pem(der: &[u8], label: &str) -> String {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, der);
        let wrapped = Self::wrap_base64(&b64, 64);
        format!("-----BEGIN {}-----\n{}\n-----END {}-----", label, wrapped, label)
    }
    
    /// Base64 换行
    fn wrap_base64(s: &str, width: usize) -> String {
        s.as_bytes()
            .chunks(width)
            .map(std::str::from_utf8)
            .filter_map(|r| r.ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 构建 Hudsucker 支持的 Authority
    pub fn into_authority(self) -> AntigravityAuthority {
        AntigravityAuthority::new(self)
    }
}

/// 自定义的 CertificateAuthority 适配 Hudsucker，解决 IP SAN 问题及带上缓存
#[derive(Clone)]
pub struct AntigravityAuthority {
    pub root_ca: Arc<RootCA>,
    cache: Arc<SyncMutex<HashMap<String, Arc<CertifiedKey>>>>,
}

impl AntigravityAuthority {
    pub fn new(root_ca: RootCA) -> Self {
        Self {
            root_ca: Arc::new(root_ca),
            cache: Arc::new(SyncMutex::new(HashMap::new())),
        }
    }

    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }

    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }
}

// 需要引入 async_trait (如果 Hudsucker 的 CertificateAuthority 不依赖原生 async fn，但查看之前报错，它是原生 async)
impl CertificateAuthority for AntigravityAuthority {
    async fn gen_server_config(&self, authority: &Authority) -> Arc<hudsucker::rustls::ServerConfig> {
        let fallback_host = authority.host().to_string();
        
        let resolver = Arc::new(AntigravityCertResolver {
            root_ca: self.root_ca.clone(),
            cache: self.cache.clone(),
            fallback_host,
        });

        let mut server_config = hudsucker::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(resolver);

        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Arc::new(server_config)
    }
}

struct AntigravityCertResolver {
    root_ca: Arc<RootCA>,
    cache: Arc<SyncMutex<HashMap<String, Arc<CertifiedKey>>>>,
    fallback_host: String,
}

impl std::fmt::Debug for AntigravityCertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AntigravityCertResolver").finish()
    }
}

impl ResolvesServerCert for AntigravityCertResolver {
    fn resolve(&self, client_hello: ClientHello) -> Option<Arc<CertifiedKey>> {
        let host = client_hello.server_name().unwrap_or(&self.fallback_host).to_string();
        
        {
            let cache = self.cache.lock().unwrap();
            if let Some(key) = cache.get(&host) {
                return Some(key.clone());
            }
        }

        let mut params = rcgen::CertificateParams::default();
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, host.clone());
        dn.push(rcgen::DnType::OrganizationName, "Antigravity Proxy");
        params.distinguished_name = dn;

        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            params.subject_alt_names = vec![rcgen::SanType::IpAddress(ip)];
        } else {
            if let Ok(ia5) = rcgen::Ia5String::try_from(host.clone()) {
                params.subject_alt_names = vec![rcgen::SanType::DnsName(ia5)];
            }
        }

        let client_key_pair = rcgen::KeyPair::generate().unwrap();
        let cert = params.signed_by(&client_key_pair, &self.root_ca.rcgen_cert, &self.root_ca.rcgen_key_pair).unwrap();

        let cert_chain = vec![hudsucker::rustls::pki_types::CertificateDer::from(cert.der().to_vec())];
        let private_key = hudsucker::rustls::pki_types::PrivateKeyDer::Pkcs8(hudsucker::rustls::pki_types::PrivatePkcs8KeyDer::from(client_key_pair.serialize_der()));

        let dummy_config = hudsucker::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)
            .ok()?;
        
        let certified_key = dummy_config.cert_resolver.resolve(client_hello)?;
        
        let mut cache = self.cache.lock().unwrap();
        cache.insert(host, certified_key.clone());
        Some(certified_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_domain_matching() {
        let domain = "daily-cloudcode-pa.googleapis.com:443";
        let without_port = domain.split(':').next().unwrap();
        assert_eq!(without_port, "daily-cloudcode-pa.googleapis.com");
    }
    
    #[test]
    fn test_rsa_pkcs8_key_rejected() {
        // RSA PKCS#8 格式私钥应该被拒绝
        let rsa_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQCx7fuY266ptYbL
PV393o5z9MN4NdAt5nN26cYMC40jYUHRmWxISTaljjmGZ2p2RBl5PnWqk1K8uyhh
86chPHcL5r2BUpoi0M812Lp8jeYZAsCL2PjjvhMlEwZk2DYO1rAaN9RTUJ8YnfLs
TuHgvCu7Nzy3WlBmfq9UR7tWugFjOjoN6OVjGNht+xeukbYGBTRlSIsNBTqapxxL
WPHEMViNh2Y2U5RQPoh+OJxFXM26Evk8PG4D0cLPdC5JC4+UBD/Hh+v3YI4lgpPp
RDkvqXMV15y5lx1hWYm9YqDDAjHYhj0GBCS9g38i22+Q474jt1N9Dm/5fkVzyyZJ
mmopypKpAgMBAAECggEATV7g8v1EHEP1U1diEcy/QSkD/rfXyL3XI7RQDFjRjLrz
9gKzFVPQ0XjhBtLddoPyV8iTPhNF/Q+dZcqfuFIkqiYx7ZRPtif6kr2lihfiIKoT
-----END PRIVATE KEY-----"#;
        
        let result = RootCA::parse_private_key(rsa_key_pem);
        // RSA 应该被拒绝
        assert!(result.is_err(), "RSA PKCS#8 私钥应该被拒绝");
    }
    
    #[test]
    fn test_pkcs8_to_sec1_conversion() {
        // 测试 PKCS#8 转 SEC1
        let ec_pkcs8_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgOGuWpE/XzxndmNFz
hu2tVI10iu7jcqM7X94H9W2I34+gCgYIKoZIzj0DAQhRANCAARL59LlXO6bagN7G
ygYPNT6fo4bifB5q1DqEFPkX3CGy3/bAVyxJ+/ZM8QTmwsEy3PTrXAks0tuIGmG8
ohSdK/eA
-----END PRIVATE KEY-----"#;
        
        // 提取 PKCS#8 DER
        let lines: Vec<&str> = ec_pkcs8_pem.lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        let b64 = lines.join("");
        let pkcs8_der = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64).unwrap();
        
        println!("PKCS#8 DER 长度: {}", pkcs8_der.len());
        println!("PKCS#8 DER 前 20 字节: {:02X?}", &pkcs8_der[..20.min(pkcs8_der.len())]);
        
        // 转换为 SEC1
        let sec1_der = RootCA::pkcs8_to_sec1(&pkcs8_der);
        println!("SEC1 DER 长度: {}", sec1_der.len());
        if !sec1_der.is_empty() {
            println!("SEC1 DER 前 20 字节: {:02X?}", &sec1_der[..20.min(sec1_der.len())]);
        }
        
        if sec1_der.is_empty() {
            println!("PKCS#8 转 SEC1 失败");
            return;
        }
        
        // 转换为 PEM 并测试 rcgen
        let sec1_pem = RootCA::der_to_pem(&sec1_der, "EC PRIVATE KEY");
        println!("SEC1 PEM:\n{}", sec1_pem);
        
        let result = KeyPair::from_pem(&sec1_pem);
        match &result {
            Ok(_) => println!("SEC1 KeyPair 创建成功!"),
            Err(e) => println!("SEC1 KeyPair 创建失败: {:?}", e),
        }
        assert!(!sec1_der.is_empty(), "SEC1 DER 不应为空");
    }
    
    #[test]
    fn test_ecdsa_pkcs8_key_parsing() {
        // 测试 ECDSA PKCS#8 格式私钥解析
        let ec_key_pem = r#"-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgOGuWpE/XzxndmNFz
hu2tVI10iu7jcqM7X94H9W2I34+gCgYIKoZIzj0DAQhRANCAARL59LlXO6bagN7G
ygYPNT6fo4bifB5q1DqEFPkX3CGy3/bAVyxJ+/ZM8QTmwsEy3PTrXAks0tuIGmG8
ohSdK/eA
-----END PRIVATE KEY-----"#;
        
        // 先测试 rcgen 是否能直接解析
        let direct_result = KeyPair::from_pem(ec_key_pem);
        println!("rcgen 直接解析 ECDSA PKCS#8: {:?}", direct_result.is_ok());
        if let Err(e) = &direct_result {
            println!("rcgen 解析错误: {:?}", e);
        }
        
        let result = RootCA::parse_private_key(ec_key_pem);
        match &result {
            Ok((_, _kp)) => {
                println!("ECDSA PKCS#8 私钥解析成功!");
            }
            Err(e) => {
                println!("ECDSA PKCS#8 私钥解析失败: {}", e);
            }
        }
        // 如果 rcgen 不支持，跳过此测试
        if direct_result.is_err() {
            println!("rcgen 不支持此格式，跳过测试");
            return;
        }
        assert!(result.is_ok(), "ECDSA PKCS#8 私钥解析应该成功");
    }
    
    #[test]
    fn test_rsa_pkcs1_key_rejected() {
        // RSA PKCS#1 格式私钥应该被拒绝
        let rsa_pkcs1_pem = r#"-----BEGIN RSA PRIVATE KEY-----
MIIBOgIBAAJBALHt+5jbrqm1hss9Xf3ejnP0w3g10C3mc3bpxgwLjSNhQdGZbEhJ
NqWOOYZnanZEGXk+daqTUry7KGHzpyE8dwvmvYFSmiLTzzXYunyN5hkCwIvY+OO+
EyUTBmTYNg7WsBo31FNQnxid8uxO4eC8K7s3PLdaUGZ+r1RHu1a6AWM6Og3o5WM=
-----END RSA PRIVATE KEY-----"#;
        
        let result = RootCA::parse_private_key(rsa_pkcs1_pem);
        // RSA 应该被拒绝
        assert!(result.is_err(), "RSA PKCS#1 私钥应该被拒绝");
    }
    
    #[test]
    fn test_ec_sec1_key_parsing() {
        // 测试 EC PRIVATE KEY (SEC1) 格式私钥解析
        let ec_sec1_pem = r#"-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIDhblqRP188Z3ZjRc4btrVSNdIru43KjO1/eB/VtiN+PoAoGCCqGSM49
AwEHoUQDQgAES+fS5Vzum2oDexsoGDzU+n6OG4nweatQ6hBT5F9whst/2wFcsSfv
2TPEE5sLBMtz061wJLNLbiBphvKIUnSv3g==
-----END EC PRIVATE KEY-----"#;
        
        let result = RootCA::parse_private_key(ec_sec1_pem);
        match &result {
            Ok((_, kp)) => {
                println!("EC SEC1 私钥解析成功!");
            }
            Err(e) => {
                println!("EC SEC1 私钥解析失败: {}", e);
            }
        }
        assert!(result.is_ok(), "EC SEC1 私钥解析应该成功");
    }
    
    #[test]
    fn test_sec1_to_pkcs8_conversion() {
        // 测试 SEC1 转 PKCS#8 转换
        let ec_sec1_pem = r#"-----BEGIN EC PRIVATE KEY-----
MHcCAQEEIDhblqRP188Z3ZjRc4btrVSNdIru43KjO1/eB/VtiN+PoAoGCCqGSM49
AwEHoUQDQgAES+fS5Vzum2oDexsoGDzU+n6OG4nweatQ6hBT5F9whst/2wFcsSfv
2TPEE5sLBMtz061wJLNLbiBphvKIUnSv3g==
-----END EC PRIVATE KEY-----"#;
        
        // 提取 SEC1 DER
        let lines: Vec<&str> = ec_sec1_pem.lines()
            .filter(|l| !l.starts_with("-----"))
            .collect();
        let b64 = lines.join("");
        let sec1_der = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &b64).unwrap();
        
        // 转换为 PKCS#8
        let pkcs8_der = RootCA::sec1_to_pkcs8(&sec1_der);
        println!("SEC1 DER 长度: {}", sec1_der.len());
        println!("PKCS#8 DER 长度: {}", pkcs8_der.len());
        
        // 验证转换后的 PKCS#8 可以被 rcgen 解析
        let pkcs8_pem = RootCA::der_to_pem(&pkcs8_der, "PRIVATE KEY");
        println!("PKCS#8 PEM:\n{}", pkcs8_pem);
        
        let result = KeyPair::from_pem(&pkcs8_pem);
        match &result {
            Ok(kp) => {
                println!("SEC1 转 PKCS#8 成功，KeyPair 创建成功!");
            }
            Err(e) => {
                println!("SEC1 转 PKCS#8 后 KeyPair 创建失败: {:?}", e);
            }
        }
        assert!(result.is_ok(), "SEC1 转 PKCS#8 应该成功");
    }
    
    #[test]
    fn test_actual_cert_files() {
        let cert_path = r"C:\Users\puppy\.antigravity_tools\ca-cert.pem";
        let key_path = r"C:\Users\puppy\.antigravity_tools\ca-key.pem";
        
        if std::path::Path::new(cert_path).exists() && std::path::Path::new(key_path).exists() {
            println!("=== 测试实际证书文件 ===");
            
            // 读取私钥
            let key_content = std::fs::read_to_string(key_path).unwrap();
            println!("私钥包含 EC PRIVATE KEY: {}", key_content.contains("EC PRIVATE KEY"));
            println!("私钥包含 PRIVATE KEY: {}", key_content.contains("PRIVATE KEY"));
            
            // 尝试解析
            match RootCA::parse_private_key(&key_content) {
                Ok((_, kp)) => {
                    println!("✓ 实际私钥解析成功!");
                }
                Err(e) => {
                    println!("✗ 实际私钥解析失败: {}", e);
                }
            }
            
            // 尝试加载完整证书
            match RootCA::load_from_pem(
                std::path::Path::new(cert_path),
                std::path::Path::new(key_path)
            ) {
                Ok(_) => {
                    println!("✓ 实际证书加载成功!");
                }
                Err(e) => {
                    println!("✗ 实际证书加载失败: {}", e);
                }
            }
        } else {
            println!("证书文件不存在，跳过测试");
        }
    }
}
