use crate::models::{Account, AppConfig, Instance, QuotaData, TokenData};
use crate::modules;
use tauri::{Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

// 导出 proxy 命令
pub mod proxy;
// 导出 autostart 命令
pub mod autostart;
// 导出 mitm 命令
pub mod mitm;

use uuid::Uuid;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 列出所有账号
#[tauri::command]
pub async fn list_accounts() -> Result<Vec<Account>, String> {
    modules::list_accounts()
}

/// 添加账号
#[tauri::command]
pub async fn add_account(
    app: tauri::AppHandle,
    _email: String,
    refresh_token: String,
) -> Result<Account, String> {
    // 1. 使用 refresh_token 获取 access_token
    // 注意：这里我们忽略传入的 _email，而是直接去 Google 获取真实的邮箱
    let token_res = modules::oauth::refresh_access_token(&refresh_token).await?;

    // 2. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;

    // 3. 构造 TokenData
    let token = TokenData::new(
        token_res.access_token,
        refresh_token, // 继续使用用户传入的 refresh_token
        token_res.expires_in,
        Some(user_info.email.clone()),
        None, // project_id 将在需要时获取
        None, // session_id
    );

    // 4. 使用真实的 email 添加或更新账号
    let account =
        modules::upsert_account(user_info.email.clone(), user_info.get_display_name(), token)?;

    modules::logger::log_info(&format!("添加账号成功: {}", account.email));

    // 5. 自动触发刷新额度
    let mut account = account;
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 6. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 删除账号
#[tauri::command]
pub async fn delete_account(app: tauri::AppHandle, account_id: String) -> Result<(), String> {
    modules::logger::log_info(&format!("收到删除账号请求: {}", account_id));
    modules::delete_account(&account_id).map_err(|e| {
        modules::logger::log_error(&format!("删除账号失败: {}", e));
        e
    })?;
    modules::logger::log_info(&format!("账号删除成功: {}", account_id));

    // 强制同步托盘
    crate::modules::tray::update_tray_menus(&app);
    Ok(())
}

/// 批量删除账号
#[tauri::command]
pub async fn delete_accounts(
    app: tauri::AppHandle,
    account_ids: Vec<String>,
) -> Result<(), String> {
    modules::logger::log_info(&format!(
        "收到批量删除请求，共 {} 个账号",
        account_ids.len()
    ));
    modules::account::delete_accounts(&account_ids).map_err(|e| {
        modules::logger::log_error(&format!("批量删除失败: {}", e));
        e
    })?;

    // 强制同步托盘
    crate::modules::tray::update_tray_menus(&app);
    Ok(())
}

/// 重新排序账号列表
/// 根据传入的账号ID数组顺序更新账号排列
#[tauri::command]
pub async fn reorder_accounts(account_ids: Vec<String>) -> Result<(), String> {
    modules::logger::log_info(&format!(
        "收到账号重排序请求，共 {} 个账号",
        account_ids.len()
    ));
    modules::account::reorder_accounts(&account_ids).map_err(|e| {
        modules::logger::log_error(&format!("账号重排序失败: {}", e));
        e
    })
}

/// 切换账号
#[tauri::command]
pub async fn switch_account(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
) -> Result<(), String> {
    let res = modules::switch_account(&account_id).await;
    if res.is_ok() {
        crate::modules::tray::update_tray_menus(&app);

        // [FIX #820] Notify proxy to clear stale session bindings and reload accounts
        // This prevents API requests from routing to the wrong account after switching
        let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;
    }
    res
}

/// 免重启切换账号 (Hot Switch)
/// 使用 antigravity:// 协议直接传递 Access Token 给 IDE
#[tauri::command]
pub async fn switch_account_hot(
    app: tauri::AppHandle,
    account_id: String,
) -> Result<serde_json::Value, String> {
    modules::logger::log_info(&format!("Hot switching account: {}", account_id));

    // 1. 加载账号
    let account = modules::load_account(&account_id).map_err(|e| e.to_string())?;

    // 2. 刷新 Token (确保 Access Token 有效)
    let fresh_token = modules::oauth::ensure_fresh_token(&account.token).await?;

    // 如果 token 更新了，保存回账号
    if fresh_token.access_token != account.token.access_token {
        let mut updated_account = account.clone();
        updated_account.token = fresh_token.clone();
        modules::account::save_account(&updated_account).map_err(|e| e.to_string())?;
    }

    // 3. 构造回调 URL
    // Format: antigravity://codeium.antigravity?access_token=<TOKEN>&state=<UUID>&token_type=Bearer
    let state = Uuid::new_v4().to_string();
    let params = [
        ("access_token", fresh_token.access_token.as_str()),
        ("state", &state),
        ("token_type", "Bearer"),
    ];

    let fragment = serde_urlencoded::to_string(&params)
        .map_err(|e| format!("Failed to encode URL parameters: {}", e))?;

    // 注意：这里使用 # 还是 ? 取决于协议实现，参考 windsurf 是 #
    // 但通常 OAuth 回调是 #. 咱们先试 #
    let callback_url = format!("antigravity://google.antigravity#{}", fragment);

    modules::logger::log_info(&format!("Triggering hot switch callback: {}", callback_url));

    // 4. 打开 URL
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        // 使用 PowerShell Start-Process 以获得更好的 URL 处理兼容性
        Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-Command",
                &format!("Start-Process '{}'", callback_url),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        Command::new("open")
            .arg(&callback_url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        Command::new("xdg-open")
            .arg(&callback_url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    // 热切换不需要重启实例，只需要更新 Manager 中的当前账号状态
    // 不调用 switch_account，因为那会关闭并重启实例
    // 只更新索引中的当前账号 ID
    let mut index = modules::account::load_account_index().map_err(|e| e.to_string())?;
    index.current_account_id = Some(account_id.clone());
    modules::account::save_account_index(&index).map_err(|e| e.to_string())?;

    // 更新最后使用时间
    let mut updated_account = account.clone();
    updated_account.last_used = chrono::Utc::now().timestamp();
    updated_account.token = fresh_token.clone();
    modules::account::save_account(&updated_account).map_err(|e| e.to_string())?;

    crate::modules::tray::update_tray_menus(&app);

    modules::logger::log_info("Hot switch triggered successfully (no restart)");

    Ok(serde_json::json!({
        "success": true,
        "message": "已触发免重启切换，请等待 IDE 处理回调",
        "url": callback_url // for debug
    }))
}

/// 获取当前账号
#[tauri::command]
pub async fn get_current_account() -> Result<Option<Account>, String> {
    // println!("🚀 Backend Command: get_current_account called"); // Commented out to reduce noise for frequent calls, relies on frontend log for frequency
    // Actually user WANTS to see it.
    modules::logger::log_info("Backend Command: get_current_account called");

    let account_id = modules::get_current_account_id()?;

    if let Some(id) = account_id {
        // modules::logger::log_info(&format!("   Found current account ID: {}", id));
        modules::load_account(&id).map(Some)
    } else {
        modules::logger::log_info("   No current account set");
        Ok(None)
    }
}

/// 内部辅助功能：在添加或导入账号后自动刷新一次额度
async fn internal_refresh_account_quota(
    app: &tauri::AppHandle,
    account: &mut Account,
) -> Result<QuotaData, String> {
    modules::logger::log_info(&format!("自动触发刷新配额: {}", account.email));

    // 使用带重试的查询 (Shared logic)
    match modules::account::fetch_quota_with_retry(account).await {
        Ok(quota) => {
            // 更新账号配额
            let _ = modules::update_account_quota(&account.id, quota.clone());
            // 更新托盘菜单
            crate::modules::tray::update_tray_menus(app);
            Ok(quota)
        }
        Err(e) => {
            modules::logger::log_warn(&format!("自动刷新配额失败 ({}): {}", account.email, e));
            Err(e.to_string())
        }
    }
}

/// 查询账号配额
#[tauri::command]
pub async fn fetch_account_quota(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
) -> crate::error::AppResult<QuotaData> {
    modules::logger::log_info(&format!("手动刷新配额请求: {}", account_id));
    let mut account =
        modules::load_account(&account_id).map_err(crate::error::AppError::Account)?;

    // 使用带重试的查询 (Shared logic)
    let quota = modules::account::fetch_quota_with_retry(&mut account).await?;

    // 4. 更新账号配额
    modules::update_account_quota(&account_id, quota.clone())
        .map_err(crate::error::AppError::Account)?;

    crate::modules::tray::update_tray_menus(&app);

    // 5. 同步到运行中的反代服务（如果已启动）
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.reload_account(&account_id).await;
    }

    Ok(quota)
}

pub use modules::account::RefreshStats;

/// 刷新所有账号配额
#[tauri::command]
pub async fn refresh_all_quotas(
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
) -> Result<RefreshStats, String> {
    let stats = modules::account::refresh_all_quotas_logic().await?;

    // 同步到运行中的反代服务（如果已启动）
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        let _ = instance.token_manager.reload_all_accounts().await;
    }

    Ok(stats)
}
/// 获取设备指纹（当前 storage.json + 账号绑定）
#[tauri::command]
pub async fn get_device_profiles(
    account_id: String,
) -> Result<modules::account::DeviceProfiles, String> {
    modules::get_device_profiles(&account_id)
}

/// 绑定设备指纹（capture: 采集当前；generate: 生成新指纹），并写入 storage.json
#[tauri::command]
pub async fn bind_device_profile(
    account_id: String,
    mode: String,
) -> Result<crate::models::DeviceProfile, String> {
    modules::bind_device_profile(&account_id, &mode)
}

/// 预览生成一个指纹（不落盘）
#[tauri::command]
pub async fn preview_generate_profile() -> Result<crate::models::DeviceProfile, String> {
    Ok(crate::modules::device::generate_profile())
}

/// 使用给定指纹直接绑定
#[tauri::command]
pub async fn bind_device_profile_with_profile(
    account_id: String,
    profile: crate::models::DeviceProfile,
) -> Result<crate::models::DeviceProfile, String> {
    modules::bind_device_profile_with_profile(&account_id, profile, Some("generated".to_string()))
}

/// 将账号已绑定的指纹应用到 storage.json
#[tauri::command]
pub async fn apply_device_profile(
    account_id: String,
) -> Result<crate::models::DeviceProfile, String> {
    modules::apply_device_profile(&account_id)
}

/// 恢复最早的 storage.json 备份（近似“原始”状态）
#[tauri::command]
pub async fn restore_original_device() -> Result<String, String> {
    modules::restore_original_device()
}

/// 列出指纹版本
#[tauri::command]
pub async fn list_device_versions(
    account_id: String,
) -> Result<modules::account::DeviceProfiles, String> {
    modules::list_device_versions(&account_id)
}

/// 按版本恢复指纹
#[tauri::command]
pub async fn restore_device_version(
    account_id: String,
    version_id: String,
) -> Result<crate::models::DeviceProfile, String> {
    modules::restore_device_version(&account_id, &version_id)
}

/// 删除历史指纹（baseline 不可删）
#[tauri::command]
pub async fn delete_device_version(account_id: String, version_id: String) -> Result<(), String> {
    modules::delete_device_version(&account_id, &version_id)
}

/// 打开设备存储目录
#[tauri::command]
pub async fn open_device_folder(app: tauri::AppHandle) -> Result<(), String> {
    let dir = modules::device::get_storage_dir()?;
    let dir_str = dir
        .to_str()
        .ok_or("无法解析存储目录路径为字符串")?
        .to_string();
    app.opener()
        .open_path(dir_str, None::<&str>)
        .map_err(|e| format!("打开目录失败: {}", e))
}

/// 加载配置
#[tauri::command]
pub async fn load_config() -> Result<AppConfig, String> {
    modules::load_app_config()
}

/// 保存配置
#[tauri::command]
pub async fn save_config(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    config: AppConfig,
) -> Result<(), String> {
    modules::save_app_config(&config)?;

    // 通知托盘配置已更新
    let _ = app.emit("config://updated", ());

    // 热更新正在运行的服务
    let instance_lock = proxy_state.instance.read().await;
    if let Some(instance) = instance_lock.as_ref() {
        // 更新模型映射
        instance.axum_server.update_mapping(&config.proxy).await;
        // 更新上游代理
        instance
            .axum_server
            .update_proxy(config.proxy.upstream_proxy.clone())
            .await;
        // 更新安全策略 (auth)
        instance.axum_server.update_security(&config.proxy).await;
        // 更新 z.ai 配置
        instance.axum_server.update_zai(&config.proxy).await;
        // 更新实验性配置
        instance
            .axum_server
            .update_experimental(&config.proxy)
            .await;
        tracing::debug!("已同步热更新反代服务配置");
    }

    Ok(())
}

// --- OAuth 命令 ---

#[tauri::command]
pub async fn start_oauth_login(app_handle: tauri::AppHandle) -> Result<Account, String> {
    modules::logger::log_info("开始 OAuth 授权流程...");

    // 1. 启动 OAuth 流程获取 Token
    let token_res = modules::oauth_server::start_oauth_flow(app_handle.clone()).await?;

    // 2. 检查 refresh_token
    let refresh_token = token_res.refresh_token.ok_or_else(|| {
        "未获取到 Refresh Token。\n\n\
         可能原因:\n\
         1. 您之前已授权过此应用,Google 不会再次返回 refresh_token\n\n\
         解决方案:\n\
         1. 访问 https://myaccount.google.com/permissions\n\
         2. 撤销 'Antigravity Tools' 的访问权限\n\
         3. 重新进行 OAuth 授权\n\n\
         或者使用 'Refresh Token' 标签页手动添加账号"
            .to_string()
    })?;

    // 3. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;
    modules::logger::log_info(&format!("获取用户信息成功: {}", user_info.email));

    // 4. 尝试获取项目ID
    let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
        .await
        .ok();

    if let Some(ref pid) = project_id {
        modules::logger::log_info(&format!("获取项目ID成功: {}", pid));
    } else {
        modules::logger::log_warn("未能获取项目ID,将在后续懒加载");
    }

    // 5. 构造 TokenData
    let token_data = TokenData::new(
        token_res.access_token,
        refresh_token,
        token_res.expires_in,
        Some(user_info.email.clone()),
        project_id,
        None,
    );

    // 6. 添加或更新到账号列表
    modules::logger::log_info("正在保存账号信息...");
    let mut account = modules::upsert_account(
        user_info.email.clone(),
        user_info.get_display_name(),
        token_data,
    )?;

    // 7. 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // 8. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 完成 OAuth 授权（不自动打开浏览器）
#[tauri::command]
pub async fn complete_oauth_login(app_handle: tauri::AppHandle) -> Result<Account, String> {
    modules::logger::log_info("完成 OAuth 授权流程 (manual)...");

    // 1. 等待回调并交换 Token（不 open browser）
    let token_res = modules::oauth_server::complete_oauth_flow(app_handle.clone()).await?;

    // 2. 检查 refresh_token
    let refresh_token = token_res.refresh_token.ok_or_else(|| {
        "未获取到 Refresh Token。\n\n\
         可能原因:\n\
         1. 您之前已授权过此应用,Google 不会再次返回 refresh_token\n\n\
         解决方案:\n\
         1. 访问 https://myaccount.google.com/permissions\n\
         2. 撤销 'Antigravity Tools' 的访问权限\n\
         3. 重新进行 OAuth 授权\n\n\
         或者使用 'Refresh Token' 标签页手动添加账号"
            .to_string()
    })?;

    // 3. 获取用户信息
    let user_info = modules::oauth::get_user_info(&token_res.access_token).await?;
    modules::logger::log_info(&format!("获取用户信息成功: {}", user_info.email));

    // 4. 尝试获取项目ID
    let project_id = crate::proxy::project_resolver::fetch_project_id(&token_res.access_token)
        .await
        .ok();

    if let Some(ref pid) = project_id {
        modules::logger::log_info(&format!("获取项目ID成功: {}", pid));
    } else {
        modules::logger::log_warn("未能获取项目ID,将在后续懒加载");
    }

    // 5. 构造 TokenData
    let token_data = TokenData::new(
        token_res.access_token,
        refresh_token,
        token_res.expires_in,
        Some(user_info.email.clone()),
        project_id,
        None,
    );

    // 6. 添加或更新到账号列表
    modules::logger::log_info("正在保存账号信息...");
    let mut account = modules::upsert_account(
        user_info.email.clone(),
        user_info.get_display_name(),
        token_data,
    )?;

    // 7. 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app_handle, &mut account).await;

    // 8. If proxy is running, reload token pool so changes take effect immediately.
    let _ = crate::commands::proxy::reload_proxy_accounts(
        app_handle.state::<crate::commands::proxy::ProxyServiceState>(),
    )
    .await;

    Ok(account)
}

/// 预生成 OAuth 授权链接 (不打开浏览器)
#[tauri::command]
pub async fn prepare_oauth_url(app_handle: tauri::AppHandle) -> Result<String, String> {
    crate::modules::oauth_server::prepare_oauth_url(app_handle).await
}

#[tauri::command]
pub async fn cancel_oauth_login() -> Result<(), String> {
    modules::oauth_server::cancel_oauth_flow();
    Ok(())
}

// --- 导入命令 ---

#[tauri::command]
pub async fn import_v1_accounts(app: tauri::AppHandle) -> Result<Vec<Account>, String> {
    let accounts = modules::migration::import_from_v1().await?;

    // 对导入的账号尝试刷新一波
    for mut account in accounts.clone() {
        let _ = internal_refresh_account_quota(&app, &mut account).await;
    }

    Ok(accounts)
}

#[tauri::command]
pub async fn import_from_db(app: tauri::AppHandle) -> Result<Account, String> {
    // 同步函数包装为 async
    let mut account = modules::migration::import_from_db().await?;

    // 既然是从数据库导入（即 IDE 当前账号），自动将其设为 Manager 的当前账号
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)?;

    // 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 刷新托盘图标展示
    crate::modules::tray::update_tray_menus(&app);

    Ok(account)
}

#[tauri::command]
#[allow(dead_code)]
pub async fn import_custom_db(app: tauri::AppHandle, path: String) -> Result<Account, String> {
    // 调用重构后的自定义导入函数
    let mut account = modules::migration::import_from_custom_db_path(path).await?;

    // 自动设为当前账号
    let account_id = account.id.clone();
    modules::account::set_current_account_id(&account_id)?;

    // 自动触发刷新额度
    let _ = internal_refresh_account_quota(&app, &mut account).await;

    // 刷新托盘图标展示
    crate::modules::tray::update_tray_menus(&app);

    Ok(account)
}

#[tauri::command]
pub async fn sync_account_from_db(app: tauri::AppHandle) -> Result<Option<Account>, String> {
    // 1. 获取 DB 中的 Refresh Token
    let db_refresh_token = match modules::migration::get_refresh_token_from_db() {
        Ok(token) => token,
        Err(e) => {
            modules::logger::log_info(&format!("自动同步跳过: {}", e));
            return Ok(None);
        }
    };

    // 2. 获取 Manager 当前账号
    let curr_account = modules::account::get_current_account()?;

    // 3. 对比：如果 Refresh Token 相同，说明账号没变，无需导入
    if let Some(acc) = curr_account {
        if acc.token.refresh_token == db_refresh_token {
            // 账号未变，由于已经是周期性任务，我们可以选择性刷新一下配额，或者直接返回
            // 这里为了节省 API 流量，直接返回
            return Ok(None);
        }
        modules::logger::log_info(&format!(
            "检测到账号切换 ({} -> DB新账号)，正在同步...",
            acc.email
        ));
    } else {
        modules::logger::log_info("检测到新登录账号，正在自动同步...");
    }

    // 4. 执行完整导入
    let account = import_from_db(app).await?;
    Ok(Some(account))
}

/// 保存文本文件 (绕过前端 Scope 限制)
#[tauri::command]
pub async fn save_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {}", e))
}

/// 读取文本文件 (绕过前端 Scope 限制)
#[tauri::command]
pub async fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("读取文件失败: {}", e))
}

/// 清理日志缓存
#[tauri::command]
pub async fn clear_log_cache() -> Result<(), String> {
    modules::logger::clear_logs()
}

/// 打开数据目录
#[tauri::command]
pub async fn open_data_folder() -> Result<(), String> {
    let path = modules::account::get_data_dir()?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| format!("打开文件夹失败: {}", e))?;
    }

    Ok(())
}

/// 获取数据目录绝对路径
#[tauri::command]
pub async fn get_data_dir_path() -> Result<String, String> {
    let path = modules::account::get_data_dir()?;
    Ok(path.to_string_lossy().to_string())
}

/// 显示主窗口
#[tauri::command]
pub async fn show_main_window(window: tauri::Window) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())
}

/// 获取 Antigravity 可执行文件路径
#[tauri::command]
pub async fn get_antigravity_path(bypass_config: Option<bool>) -> Result<String, String> {
    // 1. 优先从配置查询 (除非明确要求绕过)
    if bypass_config != Some(true) {
        if let Ok(config) = crate::modules::config::load_app_config() {
            if let Some(path) = config.antigravity_executable {
                if std::path::Path::new(&path).exists() {
                    return Ok(path);
                }
            }
        }
    }

    // 2. 执行实时探测
    match crate::modules::process::get_antigravity_executable_path() {
        Some(path) => Ok(path.to_string_lossy().to_string()),
        None => Err("未找到 Antigravity 安装路径".to_string()),
    }
}

/// 获取 Antigravity 启动参数
#[tauri::command]
pub async fn get_antigravity_args() -> Result<Vec<String>, String> {
    match crate::modules::process::get_args_from_running_process() {
        Some(args) => Ok(args),
        None => Err("未找到正在运行的 Antigravity 进程".to_string()),
    }
}

/// 检测更新响应结构
pub use crate::modules::update_checker::UpdateInfo;

/// 检测 GitHub releases 更新
#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateInfo, String> {
    modules::logger::log_info("收到前端触发的更新检查请求");
    crate::modules::update_checker::check_for_updates().await
}

#[tauri::command]
pub async fn should_check_updates() -> Result<bool, String> {
    let settings = crate::modules::update_checker::load_update_settings()?;
    Ok(crate::modules::update_checker::should_check_for_updates(
        &settings,
    ))
}

#[tauri::command]
pub async fn update_last_check_time() -> Result<(), String> {
    crate::modules::update_checker::update_last_check_time()
}

/// 获取更新设置
#[tauri::command]
pub async fn get_update_settings() -> Result<crate::modules::update_checker::UpdateSettings, String>
{
    crate::modules::update_checker::load_update_settings()
}

/// 保存更新设置
#[tauri::command]
pub async fn save_update_settings(
    settings: crate::modules::update_checker::UpdateSettings,
) -> Result<(), String> {
    crate::modules::update_checker::save_update_settings(&settings)
}

/// 切换账号的反代禁用状态
#[tauri::command]
pub async fn toggle_proxy_status(
    app: tauri::AppHandle,
    proxy_state: tauri::State<'_, crate::commands::proxy::ProxyServiceState>,
    account_id: String,
    enable: bool,
    reason: Option<String>,
) -> Result<(), String> {
    modules::logger::log_info(&format!(
        "切换账号反代状态: {} -> {}",
        account_id,
        if enable { "启用" } else { "禁用" }
    ));

    // 1. 读取账号文件
    let data_dir = modules::account::get_data_dir()?;
    let account_path = data_dir
        .join("accounts")
        .join(format!("{}.json", account_id));

    if !account_path.exists() {
        return Err(format!("账号文件不存在: {}", account_id));
    }

    let content =
        std::fs::read_to_string(&account_path).map_err(|e| format!("读取账号文件失败: {}", e))?;

    let mut account_json: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("解析账号文件失败: {}", e))?;

    // 2. 更新 proxy_disabled 字段
    if enable {
        // 启用反代
        account_json["proxy_disabled"] = serde_json::Value::Bool(false);
        account_json["proxy_disabled_reason"] = serde_json::Value::Null;
        account_json["proxy_disabled_at"] = serde_json::Value::Null;
    } else {
        // 禁用反代
        let now = chrono::Utc::now().timestamp();
        account_json["proxy_disabled"] = serde_json::Value::Bool(true);
        account_json["proxy_disabled_at"] = serde_json::Value::Number(now.into());
        account_json["proxy_disabled_reason"] =
            serde_json::Value::String(reason.unwrap_or_else(|| "用户手动禁用".to_string()));
    }

    // 3. 保存到磁盘
    std::fs::write(
        &account_path,
        serde_json::to_string_pretty(&account_json).unwrap(),
    )
    .map_err(|e| format!("写入账号文件失败: {}", e))?;

    modules::logger::log_info(&format!(
        "账号反代状态已更新: {} ({})",
        account_id,
        if enable { "已启用" } else { "已禁用" }
    ));

    // 4. 如果反代服务正在运行,重新加载账号池
    let _ = crate::commands::proxy::reload_proxy_accounts(proxy_state).await;

    // 5. 更新托盘菜单
    crate::modules::tray::update_tray_menus(&app);

    Ok(())
}

/// 预热所有可用账号
#[tauri::command]
pub async fn warm_up_all_accounts() -> Result<String, String> {
    modules::quota::warm_up_all_accounts().await
}

/// 预热指定账号
#[tauri::command]
pub async fn warm_up_account(account_id: String) -> Result<String, String> {
    modules::quota::warm_up_account(&account_id).await
}

// ============================================================================
// HTTP API 设置命令
// ============================================================================

/// 获取 HTTP API 设置
#[tauri::command]
pub async fn get_http_api_settings() -> Result<crate::modules::http_api::HttpApiSettings, String> {
    crate::modules::http_api::load_settings()
}

/// 保存 HTTP API 设置
#[tauri::command]
pub async fn save_http_api_settings(
    settings: crate::modules::http_api::HttpApiSettings,
) -> Result<(), String> {
    crate::modules::http_api::save_settings(&settings)
}

// ============================================================================
// Token Statistics Commands
// ============================================================================

pub use crate::modules::token_stats::{AccountTokenStats, TokenStatsAggregated, TokenStatsSummary};

#[tauri::command]
pub async fn get_token_stats_hourly(hours: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    crate::modules::token_stats::get_hourly_stats(hours)
}

#[tauri::command]
pub async fn get_token_stats_daily(days: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    crate::modules::token_stats::get_daily_stats(days)
}

#[tauri::command]
pub async fn get_token_stats_weekly(weeks: i64) -> Result<Vec<TokenStatsAggregated>, String> {
    crate::modules::token_stats::get_weekly_stats(weeks)
}

#[tauri::command]
pub async fn get_token_stats_by_account(hours: i64) -> Result<Vec<AccountTokenStats>, String> {
    crate::modules::token_stats::get_account_stats(hours)
}

#[tauri::command]
pub async fn get_token_stats_summary(hours: i64) -> Result<TokenStatsSummary, String> {
    crate::modules::token_stats::get_summary_stats(hours)
}

#[tauri::command]
pub async fn get_token_stats_by_model(
    hours: i64,
) -> Result<Vec<crate::modules::token_stats::ModelTokenStats>, String> {
    crate::modules::token_stats::get_model_stats(hours)
}

#[tauri::command]
pub async fn get_token_stats_model_trend_hourly(
    hours: i64,
) -> Result<Vec<crate::modules::token_stats::ModelTrendPoint>, String> {
    crate::modules::token_stats::get_model_trend_hourly(hours)
}

#[tauri::command]
pub async fn get_token_stats_model_trend_daily(
    days: i64,
) -> Result<Vec<crate::modules::token_stats::ModelTrendPoint>, String> {
    crate::modules::token_stats::get_model_trend_daily(days)
}

#[tauri::command]
pub async fn get_token_stats_account_trend_hourly(
    hours: i64,
) -> Result<Vec<crate::modules::token_stats::AccountTrendPoint>, String> {
    crate::modules::token_stats::get_account_trend_hourly(hours)
}

#[tauri::command]
pub async fn get_token_stats_account_trend_daily(
    days: i64,
) -> Result<Vec<crate::modules::token_stats::AccountTrendPoint>, String> {
    crate::modules::token_stats::get_account_trend_daily(days)
}

// ============================================================================
// Instance Management Commands (多实例支持)
// ============================================================================

/// 列出所有实例
#[tauri::command]
pub async fn list_instances() -> Result<Vec<Instance>, String> {
    modules::instance::list_instances()
}

/// 创建新实例
#[tauri::command]
pub async fn create_instance(
    name: String,
    user_data_dir: String,
    extra_args: Option<Vec<String>>,
) -> Result<Instance, String> {
    let path = std::path::PathBuf::from(user_data_dir);
    modules::instance::create_instance(name, path, extra_args.unwrap_or_default())
}

/// 获取实例详情
#[tauri::command]
pub async fn get_instance(instance_id: String) -> Result<Instance, String> {
    modules::instance::load_instance(&instance_id)
}

/// 删除实例
#[tauri::command]
pub async fn delete_instance(instance_id: String) -> Result<(), String> {
    modules::instance::delete_instance(&instance_id)
}

/// 更新实例
#[tauri::command]
pub async fn update_instance(instance: Instance) -> Result<(), String> {
    modules::instance::update_instance(&instance)
}

/// 绑定账号到实例
#[tauri::command]
pub async fn bind_account_to_instance(
    account_id: String,
    instance_id: String,
) -> Result<(), String> {
    modules::instance::bind_account_to_instance(&account_id, &instance_id)
}

/// 解绑账号从实例
#[tauri::command]
pub async fn unbind_account_from_instance(
    account_id: String,
    instance_id: String,
) -> Result<(), String> {
    modules::instance::unbind_account_from_instance(&account_id, &instance_id)
}

/// 启动指定实例
#[tauri::command]
pub async fn start_instance(instance_id: String) -> Result<(), String> {
    let instance = modules::instance::load_instance(&instance_id)?;

    // 如果有保存的启动参数，使用它们；否则使用默认参数
    if let Some(ref saved_args) = instance.last_launch_args {
        // [Fix] 检查参数是否有效（不包含 --type=）
        let args_str = saved_args.join(" ");
        if !saved_args.is_empty() && !args_str.contains("--type=") {
            modules::logger::log_info(&format!(
                "Starting instance {} with saved args: {:?}",
                instance.name, saved_args
            ));
            return modules::process::start_instance_with_args(&instance, saved_args.clone());
        } else if args_str.contains("--type=") {
            modules::logger::log_warn(&format!(
                "Instance {} has invalid saved args (contains --type=), using default args",
                instance.name
            ));
        }
    }

    modules::process::start_instance(&instance)
}

/// 停止指定实例
#[tauri::command]
pub async fn stop_instance(instance_id: String) -> Result<(), String> {
    let mut instance = modules::instance::load_instance(&instance_id)?;

    // 在停止前保存主进程的命令行参数（跳过第一个参数，即可执行文件路径）
    if let Some(args) = modules::process::get_instance_root_process_args(&instance.user_data_dir) {
        // 跳过第一个参数（可执行文件路径），只保存实际的启动参数
        let args_without_exe: Vec<String> = args.into_iter().skip(1).collect();
        let args_str = args_without_exe.join(" ");

        // [Fix] 只保存有效参数（不包含 --type=）
        if !args_without_exe.is_empty() && !args_str.contains("--type=") {
            modules::logger::log_info(&format!(
                "Saving launch args for instance {}: {:?}",
                instance.name, args_without_exe
            ));
            instance.last_launch_args = Some(args_without_exe);
        } else if args_str.contains("--type=") {
            modules::logger::log_warn(&format!(
                "Discarding invalid args for instance {} (contains --type=)",
                instance.name
            ));
        }
    }

    // 清除缓存的 PID（实例已停止）
    instance.last_root_pid = None;
    let _ = modules::instance::save_instance(&instance);

    modules::process::close_instance(&instance.user_data_dir, 20)
}

/// 获取实例运行状态
/// 同时更新 last_root_pid 和 last_launch_args（如果实例正在运行）
#[tauri::command]
pub async fn get_instance_status(instance_id: String) -> Result<bool, String> {
    let mut instance = modules::instance::load_instance(&instance_id)?;

    // 使用缓存的 PID 进行快速检测
    let cached_pid = instance.last_root_pid;

    // 检测是否运行
    let (is_running, new_pid, new_args) = if instance.is_default {
        // 默认实例
        let running = modules::process::is_default_instance_running();
        if running {
            // 尝试获取 PID 和参数
            if let Some((pid, args)) = modules::process::get_instance_root_pid_and_args(
                &instance.user_data_dir,
                true,
                cached_pid,
            ) {
                (true, Some(pid), Some(args))
            } else {
                (true, None, None)
            }
        } else {
            (false, None, None)
        }
    } else {
        // 非默认实例：优先使用缓存 PID 检测
        if let Some(pid) = cached_pid {
            if modules::process::is_pid_valid_instance_root(pid, &instance.user_data_dir, false) {
                // 缓存 PID 仍有效
                if let Some((_, args)) = modules::process::get_instance_root_pid_and_args(
                    &instance.user_data_dir,
                    false,
                    Some(pid),
                ) {
                    (true, Some(pid), Some(args))
                } else {
                    (true, Some(pid), None)
                }
            } else {
                // 缓存 PID 无效，重新检测
                let running = modules::process::is_instance_running(&instance.user_data_dir);
                if running {
                    if let Some((pid, args)) = modules::process::get_instance_root_pid_and_args(
                        &instance.user_data_dir,
                        false,
                        None,
                    ) {
                        (true, Some(pid), Some(args))
                    } else {
                        (true, None, None)
                    }
                } else {
                    (false, None, None)
                }
            }
        } else {
            // 无缓存 PID
            let running = modules::process::is_instance_running(&instance.user_data_dir);
            if running {
                if let Some((pid, args)) = modules::process::get_instance_root_pid_and_args(
                    &instance.user_data_dir,
                    false,
                    None,
                ) {
                    (true, Some(pid), Some(args))
                } else {
                    (true, None, None)
                }
            } else {
                (false, None, None)
            }
        }
    };

    // 更新实例配置（只在 PID 变化时保存）
    let mut need_save = false;

    if is_running {
        if new_pid != instance.last_root_pid {
            instance.last_root_pid = new_pid;
            need_save = true;
        }
        if let Some(args) = new_args {
            // 只在参数有效且与现有不同时更新
            let args_str = args.join(" ");
            if !args_str.contains("--type=") {
                if instance.last_launch_args.as_ref() != Some(&args) {
                    instance.last_launch_args = Some(args);
                    need_save = true;
                }
            }
        }
    } else {
        // 实例未运行，清除缓存的 PID
        if instance.last_root_pid.is_some() {
            instance.last_root_pid = None;
            need_save = true;
        }
    }

    if need_save {
        let _ = modules::instance::save_instance(&instance);
    }

    Ok(is_running)
}

/// 获取默认实例（如果不存在则创建）
#[tauri::command]
pub async fn ensure_default_instance() -> Result<Instance, String> {
    modules::instance::ensure_default_instance()
}

/// 迁移现有账号到默认实例
#[tauri::command]
pub async fn migrate_accounts_to_default_instance() -> Result<(), String> {
    modules::instance::migrate_accounts_to_default_instance()
}

/// 获取账号所属的实例列表
#[tauri::command]
pub async fn get_instances_for_account(account_id: String) -> Result<Vec<Instance>, String> {
    modules::instance::get_instances_for_account(&account_id)
}

/// 设置实例的当前账号
#[tauri::command]
pub async fn set_current_account_for_instance(
    instance_id: String,
    account_id: String,
) -> Result<(), String> {
    modules::instance::set_current_account_for_instance(&instance_id, &account_id)
}

/// 在指定实例中切换账号
/// 返回 bool 表示实例之前是否在运行（即是否触发了自动重启）
#[tauri::command]
pub async fn switch_account_in_instance(
    instance_id: String,
    account_id: String,
) -> Result<bool, String> {
    // 加载实例配置
    let mut instance = modules::instance::load_instance(&instance_id)?;

    // 检查实例是否正在运行
    let was_running = if instance.is_default {
        modules::process::is_default_instance_running()
    } else {
        modules::process::is_instance_running(&instance.user_data_dir)
    };

    // 更新实例的 current_account_id
    instance.current_account_id = Some(account_id.clone());
    modules::instance::save_instance(&instance)?;

    // 使用新的 switch_account_for_instance 函数执行实际切换
    // 如果实例正在运行，会自动停止 -> 切换账号 -> 重启
    modules::account::switch_account_for_instance(&account_id, &instance, true).await?;

    // 返回实例之前是否在运行
    Ok(was_running)
}

/// 获取所有运行中的实例
#[tauri::command]
pub async fn get_running_instances() -> Result<Vec<Instance>, String> {
    modules::instance::get_running_instances()
}
