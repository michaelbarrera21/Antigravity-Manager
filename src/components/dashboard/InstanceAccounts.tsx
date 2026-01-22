import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Layers, Mail, Diamond, Gem, Circle, RefreshCw } from 'lucide-react';
import { Instance } from '../../types/instance';
import { Account } from '../../types/account';
import { listInstances, getInstanceStatus } from '../../services/instanceService';
import { formatTimeRemaining } from '../../utils/format';

interface InstanceAccountsProps {
    accounts: Account[];
    onRefresh?: (accountId: string) => Promise<void>;
}

interface InstanceWithStatus extends Instance {
    isRunning: boolean;
    currentAccount?: Account;
}

function InstanceAccounts({ accounts, onRefresh }: InstanceAccountsProps) {
    const { t } = useTranslation();
    const [instances, setInstances] = useState<InstanceWithStatus[]>([]);
    const [loading, setLoading] = useState(true);
    const [refreshingId, setRefreshingId] = useState<string | null>(null);

    useEffect(() => {
        fetchInstances();
        // 每 5 秒轮询一次状态
        const interval = setInterval(fetchInstances, 5000);
        return () => clearInterval(interval);
    }, [accounts]);

    const fetchInstances = async () => {
        try {
            const allInstances = await listInstances();
            const instancesWithStatus: InstanceWithStatus[] = await Promise.all(
                allInstances.map(async (inst) => {
                    const isRunning = await getInstanceStatus(inst.id);
                    const currentAccount = inst.current_account_id
                        ? accounts.find(a => a.id === inst.current_account_id)
                        : undefined;
                    return { ...inst, isRunning, currentAccount };
                })
            );
            // 按运行状态排序：运行中的在前
            instancesWithStatus.sort((a, b) => {
                if (a.isRunning && !b.isRunning) return -1;
                if (!a.isRunning && b.isRunning) return 1;
                return 0;
            });
            setInstances(instancesWithStatus);
        } catch (error) {
            console.error('Failed to fetch instances:', error);
        } finally {
            setLoading(false);
        }
    };

    const handleRefresh = async (accountId: string) => {
        if (!onRefresh || refreshingId) return;
        setRefreshingId(accountId);
        try {
            await onRefresh(accountId);
            await fetchInstances();
        } finally {
            setRefreshingId(null);
        }
    };

    if (loading) {
        return (
            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                <div className="flex items-center justify-center py-8">
                    <RefreshCw className="w-5 h-5 animate-spin text-gray-400" />
                </div>
            </div>
        );
    }

    const runningCount = instances.filter(i => i.isRunning).length;

    if (instances.length === 0) {
        return (
            <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
                <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                    <Layers className="w-4 h-4 text-indigo-500" />
                    {t('dashboard.instance_accounts')}
                </h2>
                <div className="text-center py-4 text-gray-400 dark:text-gray-500 text-sm">
                    {t('dashboard.no_instance')}
                </div>
            </div>
        );
    }

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl p-4 shadow-sm border border-gray-100 dark:border-base-200">
            <h2 className="text-base font-semibold text-gray-900 dark:text-base-content mb-3 flex items-center gap-2">
                <Layers className="w-4 h-4 text-indigo-500" />
                {t('dashboard.instance_accounts')}
                <span className="ml-auto text-xs font-normal text-gray-400">
                    {runningCount}/{instances.length} {t('dashboard.running')}
                </span>
            </h2>

            <div className={`grid gap-3 ${instances.length > 1 ? 'grid-cols-1 md:grid-cols-2' : 'grid-cols-1'}`}>
                {instances.map((instance) => (
                    <InstanceCard
                        key={instance.id}
                        instance={instance}
                        isRefreshing={refreshingId === instance.currentAccount?.id}
                        onRefresh={onRefresh && instance.currentAccount ? () => handleRefresh(instance.currentAccount!.id) : undefined}
                    />
                ))}
            </div>
        </div>
    );
}

interface InstanceCardProps {
    instance: InstanceWithStatus;
    isRefreshing?: boolean;
    onRefresh?: () => void;
}

function InstanceCard({ instance, isRefreshing, onRefresh }: InstanceCardProps) {
    const { t } = useTranslation();
    const account = instance.currentAccount;

    return (
        <div className="p-3 bg-gray-50 dark:bg-base-200 rounded-lg border border-gray-100 dark:border-base-300">
            {/* 实例名称 */}
            <div className="flex items-center gap-2 mb-2">
                <span className={`w-2 h-2 rounded-full ${instance.isRunning ? 'bg-green-500' : 'bg-gray-300'}`} />
                <span className="text-xs font-medium text-gray-600 dark:text-gray-400 truncate">
                    {instance.name}
                </span>
                {onRefresh && (
                    <button
                        onClick={onRefresh}
                        disabled={isRefreshing}
                        className="ml-auto p-1 hover:bg-gray-200 dark:hover:bg-base-300 rounded transition-colors"
                    >
                        <RefreshCw className={`w-3 h-3 text-gray-400 ${isRefreshing ? 'animate-spin' : ''}`} />
                    </button>
                )}
            </div>

            {account ? (
                <>
                    {/* 邮箱和订阅 */}
                    <div className="flex items-center gap-2 mb-2">
                        <Mail className="w-3 h-3 text-gray-400 shrink-0" />
                        <span className="text-sm font-medium text-gray-700 dark:text-gray-300 truncate flex-1">
                            {account.email}
                        </span>
                        {account.quota?.subscription_tier && <SubscriptionBadge tier={account.quota.subscription_tier} />}
                    </div>

                    {/* 配额条 */}
                    <div className="space-y-1.5">
                        <QuotaBar
                            label="Gemini Pro"
                            model={account.quota?.models.find(m => m.name === 'gemini-3-pro-high')}
                            color="emerald"
                        />
                        <QuotaBar
                            label="Claude 4.5"
                            model={account.quota?.models.find(m => m.name === 'claude-sonnet-4-5-thinking')}
                            color="cyan"
                        />
                    </div>
                </>
            ) : (
                <div className="text-xs text-gray-400 text-center py-2">
                    {t('dashboard.no_account_selected')}
                </div>
            )}
        </div>
    );
}

function SubscriptionBadge({ tier }: { tier: string }) {
    const tierLower = tier.toLowerCase();
    if (tierLower.includes('ultra')) {
        return (
            <span className="flex items-center gap-0.5 px-1.5 py-0.5 rounded bg-gradient-to-r from-purple-600 to-pink-600 text-white text-[9px] font-bold shrink-0">
                <Gem className="w-2 h-2" />
                ULTRA
            </span>
        );
    } else if (tierLower.includes('pro')) {
        return (
            <span className="flex items-center gap-0.5 px-1.5 py-0.5 rounded bg-gradient-to-r from-blue-600 to-indigo-600 text-white text-[9px] font-bold shrink-0">
                <Diamond className="w-2 h-2" />
                PRO
            </span>
        );
    }
    return (
        <span className="flex items-center gap-0.5 px-1.5 py-0.5 rounded bg-gray-100 dark:bg-white/10 text-gray-500 text-[9px] font-bold shrink-0">
            <Circle className="w-2 h-2" />
            FREE
        </span>
    );
}

interface QuotaBarProps {
    label: string;
    model?: { percentage: number; reset_time: string };
    color: 'emerald' | 'cyan';
}

function QuotaBar({ label, model, color }: QuotaBarProps) {
    if (!model) return null;

    const colorClasses = {
        emerald: {
            high: 'bg-gradient-to-r from-emerald-400 to-emerald-500',
            mid: 'bg-gradient-to-r from-amber-400 to-amber-500',
            low: 'bg-gradient-to-r from-rose-400 to-rose-500',
            textHigh: 'text-emerald-600 dark:text-emerald-400',
            textMid: 'text-amber-600 dark:text-amber-400',
            textLow: 'text-rose-600 dark:text-rose-400',
        },
        cyan: {
            high: 'bg-gradient-to-r from-cyan-400 to-cyan-500',
            mid: 'bg-gradient-to-r from-orange-400 to-orange-500',
            low: 'bg-gradient-to-r from-rose-400 to-rose-500',
            textHigh: 'text-cyan-600 dark:text-cyan-400',
            textMid: 'text-orange-600 dark:text-orange-400',
            textLow: 'text-rose-600 dark:text-rose-400',
        },
    };

    const c = colorClasses[color];
    const barClass = model.percentage >= 50 ? c.high : model.percentage >= 20 ? c.mid : c.low;
    const textClass = model.percentage >= 50 ? c.textHigh : model.percentage >= 20 ? c.textMid : c.textLow;

    return (
        <div className="space-y-0.5">
            <div className="flex justify-between items-baseline">
                <span className="text-[10px] font-medium text-gray-500 dark:text-gray-400">{label}</span>
                <div className="flex items-center gap-1.5">
                    <span className="text-[9px] text-gray-400" title={model.reset_time ? new Date(model.reset_time).toLocaleString() : ''}>
                        {model.reset_time ? `R: ${formatTimeRemaining(model.reset_time)}` : ''}
                    </span>
                    <span className={`text-[10px] font-bold ${textClass}`}>
                        {model.percentage}%
                    </span>
                </div>
            </div>
            <div className="w-full bg-gray-100 dark:bg-base-300 rounded-full h-1 overflow-hidden">
                <div
                    className={`h-full rounded-full transition-all duration-500 ${barClass}`}
                    style={{ width: `${model.percentage}%` }}
                />
            </div>
        </div>
    );
}

export default InstanceAccounts;
