use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Antigravity 实例配置
/// 每个实例通过 --user-data-dir 参数隔离，拥有独立的进程组
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    /// 唯一标识符
    pub id: String,
    /// 显示名称（如 "工作账号"、"个人账号"）
    pub name: String,
    /// 核心隔离参数：--user-data-dir 路径
    pub user_data_dir: PathBuf,
    /// 可选自定义 Antigravity 可执行文件路径（默认使用全局配置）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antigravity_executable: Option<String>,
    /// 额外启动参数
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// 绑定的账号 ID 列表（一个实例可绑定多个账号）
    #[serde(default)]
    pub account_ids: Vec<String>,
    /// 当前在此实例中使用的账号 ID
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_account_id: Option<String>,
    /// 是否为默认实例（迁移时自动创建）
    #[serde(default)]
    pub is_default: bool,
    /// 上次启动时的命令行参数（用于停止后重新启动）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_launch_args: Option<Vec<String>>,
    /// 上次检测到的主进程 PID（用于快速验证实例是否运行）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_root_pid: Option<u32>,
    /// 创建时间戳
    pub created_at: i64,
}

impl Instance {
    pub fn new(id: String, name: String, user_data_dir: PathBuf) -> Self {
        Self {
            id,
            name,
            user_data_dir,
            antigravity_executable: None,
            extra_args: Vec::new(),
            account_ids: Vec::new(),
            current_account_id: None,
            is_default: false,
            last_launch_args: None,
            last_root_pid: None,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    /// 创建默认实例（用于迁移）
    pub fn new_default(user_data_dir: PathBuf) -> Self {
        let mut instance = Self::new(
            uuid::Uuid::new_v4().to_string(),
            "默认实例".to_string(),
            user_data_dir,
        );
        instance.is_default = true;
        instance
    }

    /// 检查账号是否绑定到此实例
    pub fn has_account(&self, account_id: &str) -> bool {
        self.account_ids.iter().any(|id| id == account_id)
    }

    /// 绑定账号
    pub fn bind_account(&mut self, account_id: String) {
        if !self.has_account(&account_id) {
            self.account_ids.push(account_id);
        }
    }

    /// 解绑账号
    pub fn unbind_account(&mut self, account_id: &str) {
        self.account_ids.retain(|id| id != account_id);
    }

    /// 获取完整的启动参数列表
    /// 注意：默认实例不需要 --user-data-dir 参数
    pub fn get_launch_args(&self) -> Vec<String> {
        let mut args = Vec::new();

        // 只有非默认实例才需要 --user-data-dir 参数
        if !self.is_default {
            args.push("--user-data-dir".to_string());
            args.push(self.user_data_dir.to_string_lossy().to_string());
        }

        args.extend(self.extra_args.clone());
        args
    }
}

/// 实例摘要信息（用于索引文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    pub user_data_dir: PathBuf,
    pub is_default: bool,
    pub account_count: usize,
}

impl From<&Instance> for InstanceSummary {
    fn from(instance: &Instance) -> Self {
        Self {
            id: instance.id.clone(),
            name: instance.name.clone(),
            user_data_dir: instance.user_data_dir.clone(),
            is_default: instance.is_default,
            account_count: instance.account_ids.len(),
        }
    }
}

/// 实例索引（instances.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceIndex {
    pub version: String,
    pub instances: Vec<InstanceSummary>,
}

impl InstanceIndex {
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            instances: Vec::new(),
        }
    }
}

impl Default for InstanceIndex {
    fn default() -> Self {
        Self::new()
    }
}
