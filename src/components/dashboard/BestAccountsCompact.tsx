import { TrendingUp, ArrowRight } from 'lucide-react';
import { Account } from '../../types/account';
import { useTranslation } from 'react-i18next';

interface BestAccountsCompactProps {
    accounts: Account[];
    currentAccountId?: string;
    onSwitch?: (accountId: string) => void;
}

function BestAccountsCompact({ accounts, currentAccountId, onSwitch }: BestAccountsCompactProps) {
    const { t } = useTranslation();

    // 获取按配额排序的列表 (排除当前账号)
    const geminiSorted = accounts
        .filter(a => a.id !== currentAccountId)
        .map(a => {
            const proQuota = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-pro-high')?.percentage || 0;
            const flashQuota = a.quota?.models.find(m => m.name.toLowerCase() === 'gemini-3-flash')?.percentage || 0;
            return { ...a, quotaVal: Math.round(proQuota * 0.7 + flashQuota * 0.3) };
        })
        .filter(a => a.quotaVal > 0)
        .sort((a, b) => b.quotaVal - a.quotaVal);

    const claudeSorted = accounts
        .filter(a => a.id !== currentAccountId)
        .map(a => ({
            ...a,
            quotaVal: a.quota?.models.find(m => m.name.toLowerCase().includes('claude'))?.percentage || 0,
        }))
        .filter(a => a.quotaVal > 0)
        .sort((a, b) => b.quotaVal - a.quotaVal);

    let bestGemini = geminiSorted[0];
    let bestClaude = claudeSorted[0];

    // 避免推荐同一个账号
    if (bestGemini && bestClaude && bestGemini.id === bestClaude.id) {
        const nextGemini = geminiSorted[1];
        const nextClaude = claudeSorted[1];
        const scoreA = bestGemini.quotaVal + (nextClaude?.quotaVal || 0);
        const scoreB = (nextGemini?.quotaVal || 0) + bestClaude.quotaVal;
        if (nextClaude && (!nextGemini || scoreA >= scoreB)) {
            bestClaude = nextClaude;
        } else if (nextGemini) {
            bestGemini = nextGemini;
        }
    }

    if (!bestGemini && !bestClaude) {
        return null;
    }

    return (
        <div className="bg-white dark:bg-base-100 rounded-xl p-3 shadow-sm border border-gray-100 dark:border-base-200">
            <div className="flex items-center gap-3 flex-wrap">
                <div className="flex items-center gap-1.5 text-gray-500 dark:text-gray-400 shrink-0">
                    <TrendingUp className="w-3.5 h-3.5 text-blue-500" />
                    <span className="text-xs font-medium">{t('dashboard.best_accounts')}</span>
                </div>

                <div className="flex items-center gap-3 flex-1 min-w-0">
                    {bestGemini && (
                        <button
                            onClick={() => onSwitch?.(bestGemini.id)}
                            className="flex items-center gap-2 px-2 py-1 bg-green-50 dark:bg-green-900/20 rounded-md border border-green-100 dark:border-green-900/30 hover:bg-green-100 dark:hover:bg-green-900/30 transition-colors group min-w-0"
                        >
                            <span className="text-[10px] text-green-600 dark:text-green-400 font-medium shrink-0">Gemini</span>
                            <span className="text-xs text-gray-700 dark:text-gray-300 truncate max-w-[120px]">
                                {bestGemini.email.split('@')[0]}
                            </span>
                            <span className="text-[10px] font-bold text-green-600 dark:text-green-400 shrink-0">
                                {bestGemini.quotaVal}%
                            </span>
                            <ArrowRight className="w-3 h-3 text-green-400 group-hover:translate-x-0.5 transition-transform shrink-0" />
                        </button>
                    )}

                    {bestClaude && (
                        <button
                            onClick={() => onSwitch?.(bestClaude.id)}
                            className="flex items-center gap-2 px-2 py-1 bg-cyan-50 dark:bg-cyan-900/20 rounded-md border border-cyan-100 dark:border-cyan-900/30 hover:bg-cyan-100 dark:hover:bg-cyan-900/30 transition-colors group min-w-0"
                        >
                            <span className="text-[10px] text-cyan-600 dark:text-cyan-400 font-medium shrink-0">Claude</span>
                            <span className="text-xs text-gray-700 dark:text-gray-300 truncate max-w-[120px]">
                                {bestClaude.email.split('@')[0]}
                            </span>
                            <span className="text-[10px] font-bold text-cyan-600 dark:text-cyan-400 shrink-0">
                                {bestClaude.quotaVal}%
                            </span>
                            <ArrowRight className="w-3 h-3 text-cyan-400 group-hover:translate-x-0.5 transition-transform shrink-0" />
                        </button>
                    )}
                </div>
            </div>
        </div>
    );
}

export default BestAccountsCompact;
