use crate::mitm::config::{MitmConfig, MitmStatus};
use crate::mitm::server::{get_mitm_status, start_mitm_service, stop_mitm_service, validate_root_ca, MitmServiceState};
use std::path::PathBuf;
use tauri::State;

/// 启动 MITM 代理服务
#[tauri::command]
pub async fn start_mitm_proxy_service(
    config: MitmConfig,
    state: State<'_, MitmServiceState>,
) -> Result<MitmStatus, String> {
    start_mitm_proxy_service_internal(config, &state).await
}

/// 内部供程序使用的 MITM 代理服务启动逻辑
pub async fn start_mitm_proxy_service_internal(
    config: MitmConfig,
    state: &MitmServiceState,
) -> Result<MitmStatus, String> {
    let mut instance_lock = state.instance.write().await;

    if instance_lock.is_some() {
        return Err("MITM 服务已在运行中".to_string());
    }

    let service_instance = start_mitm_service(config.clone()).await?;

    let status = get_mitm_status(&Some(service_instance.clone())).await;
    *instance_lock = Some(service_instance);

    tracing::info!("[MITM] 服务已启动，端口: {}", config.port);

    Ok(status)
}

/// 停止 MITM 代理服务
#[tauri::command]
pub async fn stop_mitm_proxy_service(
    state: State<'_, MitmServiceState>,
) -> Result<(), String> {
    let mut instance_lock = state.instance.write().await;
    stop_mitm_service(&mut instance_lock).await;
    Ok(())
}

/// 获取 MITM 服务状态
#[tauri::command]
pub async fn get_mitm_proxy_status(
    state: State<'_, MitmServiceState>,
) -> Result<MitmStatus, String> {
    let instance_lock = state.instance.read().await;
    Ok(get_mitm_status(&instance_lock).await)
}

/// 获取生成速度统计
#[tauri::command]
pub async fn get_mitm_speed_stats(
    state: State<'_, MitmServiceState>,
) -> Result<serde_json::Value, String> {
    let instance_lock = state.instance.read().await;
    let raw_stats = match instance_lock.as_ref() {
        Some(inst) => inst.monitor.get_speed_stats().await,
        None => crate::mitm::monitor::SpeedStats::default(),
    };

    let mut stats_arr = Vec::new();
    for (_, stat) in raw_stats.by_model.iter() {
        stats_arr.push(serde_json::json!({
            "model": stat.model,
            "request_count": stat.request_count,
            "input_tokens": 0, // MITM 暂时无法区分 input/output，统一映射到 output
            "output_tokens": stat.total_output_tokens,
            "avg_speed": stat.avg_speed(),
        }));
    }

    Ok(serde_json::json!({
        "total_requests": raw_stats.total.request_count,
        "total_input_tokens": 0,
        "total_output_tokens": raw_stats.total.total_output_tokens,
        "avg_speed": raw_stats.total.avg_speed(),
        "stats": stats_arr,
    }))
}

/// 清空速度统计
#[tauri::command]
pub async fn clear_mitm_speed_stats(
    state: State<'_, MitmServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(inst) = instance_lock.as_ref() {
        inst.monitor.clear_stats().await;
    }
    Ok(())
}

/// 验证 Root CA 证书
#[tauri::command]
pub async fn validate_mitm_root_ca(
    cert_path: String,
    key_path: String,
) -> Result<bool, String> {
    match validate_root_ca(&PathBuf::from(cert_path), &PathBuf::from(key_path)) {
        Ok(()) => Ok(true),
        Err(e) => Err(e),
    }
}

/// 设置 MITM 监控开关
#[tauri::command]
pub async fn set_mitm_monitor_enabled(
    state: State<'_, MitmServiceState>,
    enabled: bool,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(inst) = instance_lock.as_ref() {
        inst.monitor.set_enabled(enabled);
    }
    Ok(())
}

/// 获取 MITM 日志
#[tauri::command]
pub async fn get_mitm_logs(
    state: State<'_, MitmServiceState>,
) -> Result<Vec<crate::mitm::monitor::MitmRequestLog>, String> {
    let instance_lock = state.instance.read().await;
    if let Some(inst) = instance_lock.as_ref() {
        Ok(inst.monitor.get_logs().await)
    } else {
        Ok(Vec::new())
    }
}

/// 清除 MITM 证书缓存
#[tauri::command]
pub async fn clear_mitm_cert_cache(
    state: State<'_, MitmServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(inst) = instance_lock.as_ref() {
        inst.ca.clear_cache();
    }
    Ok(())
}

/// 清空 MITM 日志
#[tauri::command]
pub async fn clear_mitm_logs(
    state: State<'_, MitmServiceState>,
) -> Result<(), String> {
    let instance_lock = state.instance.read().await;
    if let Some(inst) = instance_lock.as_ref() {
        inst.monitor.clear_logs().await;
    }
    Ok(())
}
