/**
 * Instance TypeScript 类型定义
 * 对应后端 models/instance.rs
 */

export interface Instance {
  id: string;
  name: string;
  user_data_dir: string;
  antigravity_executable?: string;
  extra_args: string[];
  account_ids: string[];
  current_account_id?: string;
  is_default: boolean;
  last_launch_args?: string[];
  created_at: number;
}

export interface InstanceSummary {
  id: string;
  name: string;
  user_data_dir: string;
  is_default: boolean;
  account_count: number;
}

export interface InstanceIndex {
  version: string;
  instances: InstanceSummary[];
}
