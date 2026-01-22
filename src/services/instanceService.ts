/**
 * Instance Service - 实例管理 API 封装
 * 对应后端 commands/mod.rs 中的实例管理命令
 */

import { request as invoke } from '../utils/request';
import { Instance } from '../types/instance';

/**
 * 列出所有实例
 */
export async function listInstances(): Promise<Instance[]> {
    return await invoke('list_instances');
}

/**
 * 创建新实例
 * @param name 实例显示名称
 * @param userDataDir --user-data-dir 路径
 * @param extraArgs 额外启动参数
 */
export async function createInstance(name: string, userDataDir: string, extraArgs?: string[]): Promise<Instance> {
    return await invoke('create_instance', { name, userDataDir, extraArgs });
}

/**
 * 获取实例详情
 * @param instanceId 实例 ID
 */
export async function getInstance(instanceId: string): Promise<Instance> {
    return await invoke('get_instance', { instanceId });
}

/**
 * 删除实例
 * @param instanceId 实例 ID
 */
export async function deleteInstance(instanceId: string): Promise<void> {
    return await invoke('delete_instance', { instanceId });
}

/**
 * 更新实例
 * @param instance 实例对象
 */
export async function updateInstance(instance: Instance): Promise<void> {
    return await invoke('update_instance', { instance });
}

/**
 * 绑定账号到实例
 * @param accountId 账号 ID
 * @param instanceId 实例 ID
 */
export async function bindAccountToInstance(accountId: string, instanceId: string): Promise<void> {
    return await invoke('bind_account_to_instance', { accountId, instanceId });
}

/**
 * 解绑账号从实例
 * @param accountId 账号 ID
 * @param instanceId 实例 ID
 */
export async function unbindAccountFromInstance(accountId: string, instanceId: string): Promise<void> {
    return await invoke('unbind_account_from_instance', { accountId, instanceId });
}

/**
 * 启动指定实例
 * @param instanceId 实例 ID
 */
export async function startInstance(instanceId: string): Promise<void> {
    return await invoke('start_instance', { instanceId });
}

/**
 * 停止指定实例
 * @param instanceId 实例 ID
 */
export async function stopInstance(instanceId: string): Promise<void> {
    return await invoke('stop_instance', { instanceId });
}

/**
 * 获取实例运行状态
 * @param instanceId 实例 ID
 * @returns true 表示运行中
 */
export async function getInstanceStatus(instanceId: string): Promise<boolean> {
    return await invoke('get_instance_status', { instanceId });
}

/**
 * 确保默认实例存在（如果不存在则创建）
 */
export async function ensureDefaultInstance(): Promise<Instance> {
    return await invoke('ensure_default_instance');
}

/**
 * 迁移现有账号到默认实例
 */
export async function migrateAccountsToDefaultInstance(): Promise<void> {
    return await invoke('migrate_accounts_to_default_instance');
}

/**
 * 获取账号所属的实例列表
 * @param accountId 账号 ID
 */
export async function getInstancesForAccount(accountId: string): Promise<Instance[]> {
    return await invoke('get_instances_for_account', { accountId });
}

/**
 * 设置实例的当前账号
 * @param instanceId 实例 ID
 * @param accountId 账号 ID
 */
export async function setCurrentAccountForInstance(instanceId: string, accountId: string): Promise<void> {
    return await invoke('set_current_account_for_instance', { instanceId, accountId });
}

/**
 * 在指定实例中切换账号
 * 1. 如果账号未绑定到该实例，自动绑定
 * 2. 更新实例的 current_account_id
 * 3. 如果实例正在运行，执行实际的账号切换
 * @param instanceId 实例 ID
 * @param accountId 账号 ID
 */
export async function switchAccountInInstance(instanceId: string, accountId: string): Promise<void> {
    return await invoke('switch_account_in_instance', { instanceId, accountId });
}

/**
 * 获取所有运行中的实例
 */
export async function getRunningInstances(): Promise<Instance[]> {
    return await invoke('get_running_instances');
}
