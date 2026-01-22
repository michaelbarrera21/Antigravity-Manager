use once_cell::sync::Lazy;
use serde_json;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

use crate::models::{Instance, InstanceIndex, InstanceSummary};
use crate::modules::logger;

/// 全局实例写锁，防止并发操作时数据损坏
static INSTANCE_INDEX_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

const DATA_DIR: &str = ".antigravity_tools";
const INSTANCES_INDEX: &str = "instances.json";
const INSTANCES_DIR: &str = "instances";

/// 获取数据目录路径
fn get_data_dir() -> Result<PathBuf, String> {
    let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
    let data_dir = home.join(DATA_DIR);

    if !data_dir.exists() {
        fs::create_dir_all(&data_dir).map_err(|e| format!("failed_to_create_data_dir: {}", e))?;
    }

    Ok(data_dir)
}

/// 获取实例目录路径
fn get_instances_dir() -> Result<PathBuf, String> {
    let data_dir = get_data_dir()?;
    let instances_dir = data_dir.join(INSTANCES_DIR);

    if !instances_dir.exists() {
        fs::create_dir_all(&instances_dir)
            .map_err(|e| format!("failed_to_create_instances_dir: {}", e))?;
    }

    Ok(instances_dir)
}

/// 加载实例索引
pub fn load_instance_index() -> Result<InstanceIndex, String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(INSTANCES_INDEX);

    if !index_path.exists() {
        logger::log_info("Instance index file not found, creating new");
        return Ok(InstanceIndex::new());
    }

    let content = fs::read_to_string(&index_path)
        .map_err(|e| format!("failed_to_read_instance_index: {}", e))?;

    if content.trim().is_empty() {
        logger::log_warn("Instance index is empty, initializing new");
        return Ok(InstanceIndex::new());
    }

    let index: InstanceIndex = serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_instance_index: {}", e))?;

    logger::log_info(&format!(
        "Loaded instance index with {} instances",
        index.instances.len()
    ));
    Ok(index)
}

/// 保存实例索引（原子写入）
pub fn save_instance_index(index: &InstanceIndex) -> Result<(), String> {
    let data_dir = get_data_dir()?;
    let index_path = data_dir.join(INSTANCES_INDEX);
    let temp_path = data_dir.join(format!("{}.tmp", INSTANCES_INDEX));

    let content = serde_json::to_string_pretty(index)
        .map_err(|e| format!("failed_to_serialize_instance_index: {}", e))?;

    fs::write(&temp_path, content)
        .map_err(|e| format!("failed_to_write_temp_index_file: {}", e))?;

    fs::rename(temp_path, index_path).map_err(|e| format!("failed_to_replace_index_file: {}", e))
}

/// 加载实例完整数据
pub fn load_instance(instance_id: &str) -> Result<Instance, String> {
    let instances_dir = get_instances_dir()?;
    let instance_path = instances_dir.join(format!("{}.json", instance_id));

    if !instance_path.exists() {
        return Err(format!("Instance not found: {}", instance_id));
    }

    let content = fs::read_to_string(&instance_path)
        .map_err(|e| format!("failed_to_read_instance_data: {}", e))?;

    let mut instance: Instance = serde_json::from_str(&content)
        .map_err(|e| format!("failed_to_parse_instance_data: {}", e))?;

    // [Fix] 自动清理无效的 last_launch_args（包含 --type= 的辅助进程参数）
    if let Some(ref args) = instance.last_launch_args {
        let args_str = args.join(" ");
        if args_str.contains("--type=") {
            logger::log_warn(&format!(
                "Instance {} has invalid last_launch_args (contains --type=), cleaning up",
                instance.name
            ));
            instance.last_launch_args = None;
            // 自动保存清理后的实例配置
            let _ = save_instance(&instance);
        }
    }

    Ok(instance)
}

/// 保存实例数据
pub fn save_instance(instance: &Instance) -> Result<(), String> {
    let instances_dir = get_instances_dir()?;
    let instance_path = instances_dir.join(format!("{}.json", instance.id));

    let content = serde_json::to_string_pretty(instance)
        .map_err(|e| format!("failed_to_serialize_instance_data: {}", e))?;

    fs::write(&instance_path, content).map_err(|e| format!("failed_to_save_instance_data: {}", e))
}

/// 列出所有实例
pub fn list_instances() -> Result<Vec<Instance>, String> {
    let index = load_instance_index()?;
    let mut instances = Vec::new();
    let mut invalid_ids = Vec::new();

    for summary in &index.instances {
        match load_instance(&summary.id) {
            Ok(instance) => instances.push(instance),
            Err(e) => {
                logger::log_error(&format!("Failed to load instance {}: {}", summary.id, e));
                invalid_ids.push(summary.id.clone());
            }
        }
    }

    // 自动修复索引：删除无效的实例 ID
    if !invalid_ids.is_empty() {
        logger::log_warn(&format!(
            "Found {} invalid instance indexes, auto-cleaning...",
            invalid_ids.len()
        ));
        let _lock = INSTANCE_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        let mut index = load_instance_index()?;
        index.instances.retain(|s| !invalid_ids.contains(&s.id));
        let _ = save_instance_index(&index);
    }

    Ok(instances)
}

/// 创建新实例
pub fn create_instance(
    name: String,
    user_data_dir: PathBuf,
    extra_args: Vec<String>,
) -> Result<Instance, String> {
    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    // 检查 user_data_dir 是否已被使用
    let index = load_instance_index()?;
    for summary in &index.instances {
        if summary.user_data_dir == user_data_dir {
            return Err(format!(
                "user_data_dir already in use by instance: {}",
                summary.name
            ));
        }
    }

    // 创建新实例
    let instance_id = Uuid::new_v4().to_string();
    let mut instance = Instance::new(instance_id, name, user_data_dir);
    instance.extra_args = extra_args;

    // 保存实例数据
    save_instance(&instance)?;

    // 更新索引
    let mut index = load_instance_index()?;
    index.instances.push(InstanceSummary::from(&instance));
    save_instance_index(&index)?;

    logger::log_info(&format!(
        "Created instance: {} ({})",
        instance.name, instance.id
    ));
    Ok(instance)
}

/// 删除实例
pub fn delete_instance(instance_id: &str) -> Result<(), String> {
    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    // 检查是否存在
    let instance = load_instance(instance_id)?;

    // 不允许删除默认实例
    if instance.is_default {
        return Err("Cannot delete default instance".to_string());
    }

    // 从索引中移除
    let mut index = load_instance_index()?;
    let original_len = index.instances.len();
    index.instances.retain(|s| s.id != instance_id);

    if index.instances.len() == original_len {
        return Err(format!("Instance ID not found: {}", instance_id));
    }

    save_instance_index(&index)?;

    // 删除实例文件
    let instances_dir = get_instances_dir()?;
    let instance_path = instances_dir.join(format!("{}.json", instance_id));

    if instance_path.exists() {
        fs::remove_file(&instance_path)
            .map_err(|e| format!("failed_to_delete_instance_file: {}", e))?;
    }

    logger::log_info(&format!("Deleted instance: {}", instance_id));
    Ok(())
}

/// 更新实例
pub fn update_instance(instance: &Instance) -> Result<(), String> {
    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    // 确保实例存在
    let _ = load_instance(&instance.id)?;

    // 保存实例数据
    save_instance(instance)?;

    // 更新索引中的摘要
    let mut index = load_instance_index()?;
    if let Some(summary) = index.instances.iter_mut().find(|s| s.id == instance.id) {
        *summary = InstanceSummary::from(instance);
    }
    save_instance_index(&index)?;

    logger::log_info(&format!(
        "Updated instance: {} ({})",
        instance.name, instance.id
    ));
    Ok(())
}

/// 获取默认实例
pub fn get_default_instance() -> Result<Option<Instance>, String> {
    let instances = list_instances()?;
    Ok(instances.into_iter().find(|i| i.is_default))
}

/// 确保默认实例存在（用于迁移）
/// 如果不存在，根据当前配置创建一个
pub fn ensure_default_instance() -> Result<Instance, String> {
    // 检查是否已有默认实例
    if let Some(instance) = get_default_instance()? {
        return Ok(instance);
    }

    logger::log_info("No default instance found, creating one for migration...");

    // 尝试从当前进程或配置获取 user_data_dir
    let user_data_dir = if let Some(dir) = crate::modules::process::get_user_data_dir_from_process()
    {
        dir
    } else {
        // 使用系统默认路径作为默认实例的 user_data_dir
        get_default_user_data_dir()?
    };

    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;

    // 创建默认实例
    let instance = Instance::new_default(user_data_dir);
    save_instance(&instance)?;

    // 更新索引
    let mut index = load_instance_index()?;
    index.instances.push(InstanceSummary::from(&instance));
    save_instance_index(&index)?;

    logger::log_info(&format!("Created default instance: {}", instance.id));
    Ok(instance)
}

/// 获取系统默认 user_data_dir 路径
fn get_default_user_data_dir() -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
        Ok(home.join("Library/Application Support/Antigravity"))
    }

    #[cfg(target_os = "windows")]
    {
        let appdata =
            std::env::var("APPDATA").map_err(|_| "failed_to_get_appdata_env".to_string())?;
        Ok(PathBuf::from(appdata).join("Antigravity"))
    }

    #[cfg(target_os = "linux")]
    {
        let home = dirs::home_dir().ok_or("failed_to_get_home_dir")?;
        Ok(home.join(".config/Antigravity"))
    }
}

/// 绑定账号到实例
pub fn bind_account_to_instance(account_id: &str, instance_id: &str) -> Result<(), String> {
    let mut instance = load_instance(instance_id)?;
    instance.bind_account(account_id.to_string());
    save_instance(&instance)?;

    // 更新索引中的账号数量
    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_instance_index()?;
    if let Some(summary) = index.instances.iter_mut().find(|s| s.id == instance_id) {
        summary.account_count = instance.account_ids.len();
    }
    save_instance_index(&index)?;

    logger::log_info(&format!(
        "Bound account {} to instance {}",
        account_id, instance_id
    ));
    Ok(())
}

/// 解绑账号从实例
pub fn unbind_account_from_instance(account_id: &str, instance_id: &str) -> Result<(), String> {
    let mut instance = load_instance(instance_id)?;
    instance.unbind_account(account_id);
    save_instance(&instance)?;

    // 更新索引中的账号数量
    let _lock = INSTANCE_INDEX_LOCK
        .lock()
        .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
    let mut index = load_instance_index()?;
    if let Some(summary) = index.instances.iter_mut().find(|s| s.id == instance_id) {
        summary.account_count = instance.account_ids.len();
    }
    save_instance_index(&index)?;

    logger::log_info(&format!(
        "Unbound account {} from instance {}",
        account_id, instance_id
    ));
    Ok(())
}

/// 根据账号 ID 查找所属实例
/// 由于账号可属于多个实例，返回第一个匹配的实例
pub fn get_instance_for_account(account_id: &str) -> Result<Option<Instance>, String> {
    let instances = list_instances()?;
    Ok(instances.into_iter().find(|i| i.has_account(account_id)))
}

/// 获取账号所属的所有实例
pub fn get_instances_for_account(account_id: &str) -> Result<Vec<Instance>, String> {
    let instances = list_instances()?;
    Ok(instances
        .into_iter()
        .filter(|i| i.has_account(account_id))
        .collect())
}

/// 迁移现有账号到默认实例
pub fn migrate_accounts_to_default_instance() -> Result<(), String> {
    // 确保默认实例存在
    let mut default_instance = ensure_default_instance()?;

    // 获取所有账号
    let accounts = crate::modules::account::list_accounts()?;

    // 将所有未绑定的账号绑定到默认实例
    let instances = list_instances()?;
    let mut bound_count = 0;

    for account in &accounts {
        // 检查是否已绑定到任何实例
        let already_bound = instances.iter().any(|i| i.has_account(&account.id));
        if !already_bound {
            default_instance.bind_account(account.id.clone());
            bound_count += 1;
        }
    }

    if bound_count > 0 {
        save_instance(&default_instance)?;

        // 更新索引
        let _lock = INSTANCE_INDEX_LOCK
            .lock()
            .map_err(|e| format!("failed_to_acquire_lock: {}", e))?;
        let mut index = load_instance_index()?;
        if let Some(summary) = index
            .instances
            .iter_mut()
            .find(|s| s.id == default_instance.id)
        {
            summary.account_count = default_instance.account_ids.len();
        }
        save_instance_index(&index)?;

        logger::log_info(&format!(
            "Migrated {} accounts to default instance",
            bound_count
        ));
    }

    Ok(())
}

/// 设置实例的当前账号
pub fn set_current_account_for_instance(instance_id: &str, account_id: &str) -> Result<(), String> {
    let mut instance = load_instance(instance_id)?;

    // 验证账号已绑定到此实例
    if !instance.has_account(account_id) {
        return Err(format!(
            "Account {} is not bound to instance {}",
            account_id, instance_id
        ));
    }

    instance.current_account_id = Some(account_id.to_string());
    save_instance(&instance)?;

    logger::log_info(&format!(
        "Set current account {} for instance {}",
        account_id, instance_id
    ));
    Ok(())
}

/// 在指定实例中切换账号
/// 只更新实例的 current_account_id，不维护 account_ids 绑定关系
/// 返回：是否需要执行实际切换（实例是否运行中）
pub fn switch_account_in_instance(instance_id: &str, account_id: &str) -> Result<bool, String> {
    let mut instance = load_instance(instance_id)?;

    // 只更新当前账号，不修改 account_ids
    instance.current_account_id = Some(account_id.to_string());
    save_instance(&instance)?;

    // 检查实例是否正在运行
    let user_data_path = std::path::Path::new(&instance.user_data_dir);
    let is_running = if instance.is_default {
        crate::modules::process::is_default_instance_running()
    } else {
        crate::modules::process::is_instance_running(user_data_path)
    };

    if is_running {
        logger::log_info(&format!(
            "Instance {} is running, will perform account switch to {}",
            instance.name, account_id
        ));
    } else {
        logger::log_info(&format!(
            "Instance {} is not running, saved current_account_id for next launch",
            instance.name
        ));
    }

    logger::log_info(&format!(
        "Switched to account {} in instance {}",
        account_id, instance.name
    ));
    Ok(is_running)
}

/// 获取所有运行中的实例
pub fn get_running_instances() -> Result<Vec<Instance>, String> {
    let instances = list_instances()?;
    let mut running = Vec::new();

    for instance in instances {
        let user_data_path = std::path::Path::new(&instance.user_data_dir);
        let is_running = if instance.is_default {
            crate::modules::process::is_default_instance_running()
        } else {
            crate::modules::process::is_instance_running(user_data_path)
        };

        if is_running {
            running.push(instance);
        }
    }

    Ok(running)
}
