/**
 * InstanceManager - 实例管理组件
 * 用于在设置页面中管理 Antigravity 实例
 */

import { useState, useEffect } from 'react';
import { Plus, Trash2, Play, Square, FolderOpen, Edit2, Check, X, Layers, AlertCircle, User } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { open } from '@tauri-apps/plugin-dialog';
import { useInstanceStore } from '../../stores/useInstanceStore';
import { useAccountStore } from '../../stores/useAccountStore';
import { Instance } from '../../types/instance';
import { showToast } from '../common/ToastContainer';


export function InstanceManager() {
    const { t } = useTranslation();
    const {
        instances,
        loading,
        fetchInstances,
        createInstance,
        deleteInstance,
        updateInstance,
        startInstance,
        stopInstance,
        getInstanceStatus,
        ensureDefaultInstance
    } = useInstanceStore();

    const { accounts, fetchAccounts } = useAccountStore();

    const [instanceStatuses, setInstanceStatuses] = useState<Record<string, boolean>>({});
    const [editingId, setEditingId] = useState<string | null>(null);
    const [editName, setEditName] = useState('');
    const [isCreating, setIsCreating] = useState(false);
    const [newName, setNewName] = useState('');
    const [newPath, setNewPath] = useState('');

    // 初始化加载
    useEffect(() => {
        const init = async () => {
            await ensureDefaultInstance();
            await Promise.all([fetchInstances(), fetchAccounts()]);
        };
        init();
    }, []);

    // 刷新所有实例运行状态
    useEffect(() => {
        const refreshStatuses = async () => {
            const statuses: Record<string, boolean> = {};
            for (const inst of instances) {
                try {
                    statuses[inst.id] = await getInstanceStatus(inst.id);
                } catch {
                    statuses[inst.id] = false;
                }
            }
            setInstanceStatuses(statuses);
        };

        if (instances.length > 0) {
            refreshStatuses();
            const interval = setInterval(refreshStatuses, 2000);
            return () => clearInterval(interval);
        }
    }, [instances]);

    // 获取账号名称
    const getAccountEmail = (accountId: string): string => {
        const account = accounts.find(a => a.id === accountId);
        return account?.email || accountId;
    };

    const handleSelectPath = async () => {
        try {
            const selected = await open({
                directory: true,
                multiple: false,
                title: t('settings.instances.select_data_dir'),
            });
            if (selected && typeof selected === 'string') {
                setNewPath(selected);
            }
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleCreate = async () => {
        if (!newName.trim() || !newPath.trim()) {
            showToast(t('settings.instances.create_validation_error'), 'error');
            return;
        }
        try {
            await createInstance(newName.trim(), newPath.trim());
            setIsCreating(false);
            setNewName('');
            setNewPath('');
            showToast(t('settings.instances.created'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleDelete = async (inst: Instance) => {
        if (inst.is_default) {
            showToast(t('settings.instances.cannot_delete_default'), 'error');
            return;
        }
        try {
            await deleteInstance(inst.id);
            showToast(t('settings.instances.deleted'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleStartStop = async (inst: Instance) => {
        const isRunning = instanceStatuses[inst.id];
        try {
            if (isRunning) {
                await stopInstance(inst.id);
                showToast(t('settings.instances.stopped'), 'success');
            } else {
                await startInstance(inst.id);
                showToast(t('settings.instances.started'), 'success');
            }
            // 刷新状态
            const newStatus = await getInstanceStatus(inst.id);
            setInstanceStatuses(prev => ({ ...prev, [inst.id]: newStatus }));
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleRename = async (inst: Instance) => {
        if (!editName.trim()) {
            setEditingId(null);
            return;
        }
        try {
            await updateInstance({ ...inst, name: editName.trim() });
            setEditingId(null);
            showToast(t('settings.instances.updated'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    return (
        <div className="space-y-6">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                    <div className="w-10 h-10 rounded-xl bg-purple-50 dark:bg-purple-900/20 flex items-center justify-center text-purple-500">
                        <Layers size={20} />
                    </div>
                    <div>
                        <h2 className="text-lg font-semibold text-gray-900 dark:text-base-content">
                            {t('settings.instances.title')}
                        </h2>
                        <p className="text-sm text-gray-500 dark:text-gray-400">
                            {t('settings.instances.description')}
                        </p>
                    </div>
                </div>

                <button
                    className="px-4 py-2 bg-purple-500 text-white text-sm rounded-lg hover:bg-purple-600 transition-colors flex items-center gap-2 shadow-sm"
                    onClick={() => setIsCreating(true)}
                >
                    <Plus className="w-4 h-4" />
                    {t('settings.instances.create')}
                </button>
            </div>

            {/* 创建新实例表单 */}
            {isCreating && (
                <div className="bg-purple-50 dark:bg-purple-900/10 rounded-xl p-5 border border-purple-200 dark:border-purple-800/30 animate-in slide-in-from-top-2 duration-200">
                    <h3 className="font-medium text-gray-900 dark:text-base-content mb-4">
                        {t('settings.instances.create_new')}
                    </h3>
                    <div className="space-y-4">
                        <div>
                            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                {t('settings.instances.name')}
                            </label>
                            <input
                                type="text"
                                className="w-full px-4 py-3 border border-gray-200 dark:border-base-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-purple-500 text-gray-900 dark:text-base-content bg-white dark:bg-base-200"
                                placeholder={t('settings.instances.name_placeholder')}
                                value={newName}
                                onChange={(e) => setNewName(e.target.value)}
                            />
                        </div>
                        <div>
                            <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                                {t('settings.instances.data_dir')}
                            </label>
                            <div className="flex gap-2">
                                <input
                                    type="text"
                                    className="flex-1 px-4 py-3 border border-gray-200 dark:border-base-300 rounded-lg bg-gray-50 dark:bg-base-200 text-gray-900 dark:text-base-content"
                                    placeholder={t('settings.instances.data_dir_placeholder')}
                                    value={newPath}
                                    readOnly
                                />
                                <button
                                    className="px-4 py-2 border border-gray-200 dark:border-base-300 text-gray-700 dark:text-gray-300 rounded-lg hover:bg-gray-100 dark:hover:bg-base-200 transition-colors flex items-center gap-2"
                                    onClick={handleSelectPath}
                                >
                                    <FolderOpen size={16} />
                                    {t('common.select')}
                                </button>
                            </div>
                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-2">
                                {t('settings.instances.data_dir_hint')}
                            </p>
                        </div>
                        <div className="flex gap-2 justify-end">
                            <button
                                className="px-4 py-2 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition-colors"
                                onClick={() => { setIsCreating(false); setNewName(''); setNewPath(''); }}
                            >
                                {t('common.cancel')}
                            </button>
                            <button
                                className="px-4 py-2 bg-purple-500 text-white rounded-lg hover:bg-purple-600 transition-colors"
                                onClick={handleCreate}
                            >
                                {t('common.create')}
                            </button>
                        </div>
                    </div>
                </div>
            )}

            {/* 实例列表 */}
            {loading ? (
                <div className="flex items-center justify-center py-12">
                    <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-purple-500"></div>
                </div>
            ) : instances.length === 0 ? (
                <div className="text-center py-12 text-gray-500 dark:text-gray-400">
                    <Layers size={48} className="mx-auto mb-4 opacity-30" />
                    <p>{t('settings.instances.empty')}</p>
                </div>
            ) : (
                <div className="space-y-3">
                    {instances.map((inst) => {
                        const isRunning = instanceStatuses[inst.id];
                        const isEditing = editingId === inst.id;

                        return (
                            <div
                                key={inst.id}
                                className={`bg-white dark:bg-base-100 rounded-xl p-4 border transition-all duration-200 ${isRunning
                                    ? 'border-green-200 dark:border-green-800/30 shadow-green-100 dark:shadow-none'
                                    : 'border-gray-100 dark:border-base-200'
                                    } hover:shadow-md`}
                            >
                                <div className="flex items-center justify-between">
                                    <div className="flex items-center gap-4">
                                        {/* 状态指示器 */}
                                        <div className={`w-3 h-3 rounded-full ${isRunning ? 'bg-green-500 animate-pulse' : 'bg-gray-300 dark:bg-gray-600'}`} />

                                        {/* 名称 */}
                                        <div className="flex-1">
                                            {isEditing ? (
                                                <div className="flex items-center gap-2">
                                                    <input
                                                        type="text"
                                                        className="px-2 py-1 border border-gray-200 dark:border-base-300 rounded text-sm focus:ring-2 focus:ring-purple-500 outline-none bg-white dark:bg-base-200"
                                                        value={editName}
                                                        onChange={(e) => setEditName(e.target.value)}
                                                        autoFocus
                                                    />
                                                    <button
                                                        className="p-1 text-green-500 hover:bg-green-50 dark:hover:bg-green-900/20 rounded"
                                                        onClick={() => handleRename(inst)}
                                                    >
                                                        <Check size={16} />
                                                    </button>
                                                    <button
                                                        className="p-1 text-gray-400 hover:bg-gray-100 dark:hover:bg-base-200 rounded"
                                                        onClick={() => setEditingId(null)}
                                                    >
                                                        <X size={16} />
                                                    </button>
                                                </div>
                                            ) : (
                                                <div className="flex items-center gap-2">
                                                    <span className="font-medium text-gray-900 dark:text-base-content">
                                                        {inst.name}
                                                    </span>
                                                    {inst.is_default && (
                                                        <span className="px-2 py-0.5 text-xs bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded-full">
                                                            {t('settings.instances.default')}
                                                        </span>
                                                    )}
                                                    <button
                                                        className="p-1 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                                                        onClick={() => { setEditingId(inst.id); setEditName(inst.name); }}
                                                    >
                                                        <Edit2 size={14} />
                                                    </button>
                                                </div>
                                            )}
                                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 font-mono truncate max-w-md">
                                                {inst.user_data_dir}
                                            </p>
                                        </div>
                                    </div>


                                    {/* 操作按钮 */}
                                    <div className="flex items-center gap-2">
                                        {/* 当前账号显示 */}
                                        <div className={`px-3 py-1.5 text-xs rounded-lg flex items-center gap-2 border transition-colors ${inst.current_account_id
                                            ? 'bg-blue-50 dark:bg-blue-900/20 border-blue-100 dark:border-blue-900/30 text-blue-700 dark:text-blue-300'
                                            : 'bg-gray-50 dark:bg-base-200 border-gray-100 dark:border-base-300 text-gray-500 dark:text-gray-400'
                                            }`}>
                                            <User size={14} />
                                            <span className="font-medium">
                                                {inst.current_account_id
                                                    ? getAccountEmail(inst.current_account_id)
                                                    : t('settings.instances.no_active_account', '未激活账号')
                                                }
                                            </span>
                                        </div>

                                        {/* 启动/停止 */}
                                        <button
                                            className={`p-2 rounded-lg transition-colors ${isRunning
                                                ? 'text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20'
                                                : 'text-green-500 hover:bg-green-50 dark:hover:bg-green-900/20'
                                                }`}
                                            onClick={() => handleStartStop(inst)}
                                            title={isRunning ? t('settings.instances.stop') : t('settings.instances.start')}
                                        >
                                            {isRunning ? <Square size={18} /> : <Play size={18} />}
                                        </button>

                                        {/* 删除 */}
                                        {!inst.is_default && (
                                            <button
                                                className="p-2 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/20 rounded-lg transition-colors"
                                                onClick={() => handleDelete(inst)}
                                                title={t('common.delete')}
                                            >
                                                <Trash2 size={18} />
                                            </button>
                                        )}
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                </div>
            )}

            {/* 提示信息 */}
            <div className="flex items-start gap-3 p-4 bg-amber-50 dark:bg-amber-900/10 rounded-lg border border-amber-200 dark:border-amber-800/30">
                <AlertCircle size={20} className="text-amber-500 mt-0.5 flex-shrink-0" />
                <div className="text-sm text-amber-700 dark:text-amber-300">
                    <p className="font-medium mb-1">{t('settings.instances.tip_title')}</p>
                    <p className="text-amber-600 dark:text-amber-400">{t('settings.instances.tip_content')}</p>
                </div>
            </div>
        </div>
    );
}

export default InstanceManager;
