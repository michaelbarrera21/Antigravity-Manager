import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Layers, Check, X } from 'lucide-react';
import { Instance } from '../../types/instance';
import { cn } from '../../utils/cn';

interface InstanceSelectDialogProps {
    isOpen: boolean;
    onClose: () => void;
    instances: Instance[];
    instanceStatuses?: Record<string, boolean>; // 实例运行状态映射
    accountEmail: string;
    onSelect: (instanceId: string, instanceName: string) => void;
}

/**
 * 实例选择对话框
 * 用于在多实例场景下选择要切换账号的目标实例
 */
export function InstanceSelectDialog({
    isOpen,
    onClose,
    instances,
    instanceStatuses = {},
    accountEmail,
    onSelect,
}: InstanceSelectDialogProps) {
    const { t } = useTranslation();
    const [selectedId, setSelectedId] = useState<string | null>(null);

    if (!isOpen) return null;

    const handleConfirm = () => {
        if (selectedId) {
            const instance = instances.find(i => i.id === selectedId);
            if (instance) {
                onSelect(selectedId, instance.name);
                onClose();
            }
        }
    };

    return (
        <div className="fixed inset-0 z-50 flex items-center justify-center">
            {/* 背景遮罩 */}
            <div
                className="absolute inset-0 bg-black/50 backdrop-blur-sm"
                onClick={onClose}
            />

            {/* 对话框 */}
            <div className="relative bg-white dark:bg-base-100 rounded-2xl shadow-2xl w-full max-w-md mx-4 overflow-hidden">
                {/* 标题栏 */}
                <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 dark:border-base-300">
                    <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-purple-100 dark:bg-purple-900/30 flex items-center justify-center">
                            <Layers className="w-5 h-5 text-purple-600 dark:text-purple-400" />
                        </div>
                        <div>
                            <h3 className="font-semibold text-gray-900 dark:text-base-content">
                                {t('instances.select_instance', '选择实例')}
                            </h3>
                            <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                                {t('instances.switch_account_to', '将账号切换到以下实例')}
                            </p>
                        </div>
                    </div>
                    <button
                        onClick={onClose}
                        className="p-2 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg transition-colors"
                    >
                        <X className="w-5 h-5" />
                    </button>
                </div>

                {/* 账号信息 */}
                <div className="px-5 py-3 bg-gray-50 dark:bg-base-200/50 border-b border-gray-100 dark:border-base-300">
                    <p className="text-sm text-gray-600 dark:text-gray-400">
                        <span className="font-medium text-gray-900 dark:text-base-content">{accountEmail}</span>
                    </p>
                </div>

                {/* 实例列表 */}
                <div className="p-4 space-y-2 max-h-64 overflow-y-auto">
                    {instances.map(instance => (
                        <button
                            key={instance.id}
                            onClick={() => setSelectedId(instance.id)}
                            className={cn(
                                "w-full flex items-center gap-3 p-3 rounded-xl border-2 transition-all text-left",
                                selectedId === instance.id
                                    ? "border-purple-500 bg-purple-50 dark:bg-purple-900/20"
                                    : "border-gray-200 dark:border-base-300 hover:border-purple-300 dark:hover:border-purple-700 hover:bg-gray-50 dark:hover:bg-base-200"
                            )}
                        >
                            <div className={cn(
                                "w-8 h-8 rounded-lg flex items-center justify-center shrink-0",
                                selectedId === instance.id
                                    ? "bg-purple-500 text-white"
                                    : "bg-gray-100 dark:bg-base-200 text-gray-400"
                            )}>
                                {selectedId === instance.id ? (
                                    <Check className="w-4 h-4" />
                                ) : (
                                    <Layers className="w-4 h-4" />
                                )}
                            </div>
                            <div className="flex-1 min-w-0">
                                <div className="flex items-center gap-2">
                                    <span className={cn(
                                        "font-medium truncate",
                                        selectedId === instance.id
                                            ? "text-purple-700 dark:text-purple-300"
                                            : "text-gray-900 dark:text-base-content"
                                    )}>
                                        {instance.name}
                                    </span>
                                    {instance.is_default && (
                                        <span className="px-1.5 py-0.5 text-[10px] font-medium bg-blue-100 dark:bg-blue-900/30 text-blue-600 dark:text-blue-400 rounded">
                                            {t('instances.default', '默认')}
                                        </span>
                                    )}
                                </div>
                                <p className="text-xs text-gray-500 dark:text-gray-400 truncate mt-0.5 flex items-center gap-1.5">
                                    <span className={`w-2 h-2 rounded-full ${instanceStatuses[instance.id] ? 'bg-green-500' : 'bg-gray-400'}`} />
                                    {instanceStatuses[instance.id] ? t('instances.running', '运行中') : t('instances.stopped', '未运行')}
                                </p>
                            </div>
                        </button>
                    ))}
                </div>

                {/* 底部按钮 */}
                <div className="flex items-center justify-end gap-3 px-5 py-4 border-t border-gray-100 dark:border-base-300 bg-gray-50 dark:bg-base-200/50">
                    <button
                        onClick={onClose}
                        className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-base-200 rounded-lg transition-colors"
                    >
                        {t('common.cancel', '取消')}
                    </button>
                    <button
                        onClick={handleConfirm}
                        disabled={!selectedId}
                        className={cn(
                            "px-4 py-2 text-sm font-medium rounded-lg transition-colors",
                            selectedId
                                ? "bg-purple-600 text-white hover:bg-purple-700"
                                : "bg-gray-200 dark:bg-base-300 text-gray-400 cursor-not-allowed"
                        )}
                    >
                        {t('common.confirm', '确认')}
                    </button>
                </div>
            </div>
        </div>
    );
}

export default InstanceSelectDialog;
