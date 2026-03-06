import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';
import { MitmProxyCard } from '../components/mitm/MitmProxyCard';
import { Trash2, RefreshCw, Clock, Activity, FileJson } from 'lucide-react';
import { showToast } from '../components/common/ToastContainer';
import { cn } from '../utils/cn';

interface MitmRequestLog {
    id: string;
    timestamp: number;
    method: string;
    url: string;
    status: number;
    duration: number;
    req_headers: Record<string, string>;
    res_headers: Record<string, string>;
    request_body: string | null;
    response_body: string | null;
}

export default function MitmDashboard() {
    const { t } = useTranslation();
    const [logs, setLogs] = useState<MitmRequestLog[]>([]);
    const [selectedLogId, setSelectedLogId] = useState<string | null>(null);
    const [autoScroll, setAutoScroll] = useState(true);
    const listRef = useRef<HTMLDivElement>(null);

    const fetchLogs = async () => {
        try {
            const data = await invoke<MitmRequestLog[]>('get_mitm_logs');
            setLogs(data);
        } catch (error) {
            console.error('Failed to fetch MITM logs:', error);
        }
    };

    const handleClearLogs = async () => {
        try {
            await invoke('clear_mitm_logs');
            setLogs([]);
            setSelectedLogId(null);
            showToast(t('common.success'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    useEffect(() => {
        fetchLogs();
        const interval = setInterval(fetchLogs, 2000);
        return () => clearInterval(interval);
    }, []);

    const selectedLog = logs.find(l => l.id === selectedLogId);

    return (
        <div className="h-full w-full overflow-y-auto overflow-x-hidden mitm-scrollable">
            <div className="bg-[#FAFBFC] dark:bg-base-300 p-6">
                <div className="flex-none mb-6">
                    <MitmProxyCard />
                </div>

                <div className="flex items-center justify-between mb-4">
                    <h1 className="text-2xl font-bold text-gray-900 dark:text-gray-100 flex items-center gap-2">
                        <Activity className="w-6 h-6 text-blue-500" />
                        {t('mitm.dashboard_title', '网络抓包 (MITM)')}
                    </h1>
                    <div className="flex items-center gap-3">
                        <label className="flex items-center gap-2 cursor-pointer cursor-pointer text-sm text-gray-600 dark:text-gray-400">
                            <input
                                type="checkbox"
                                checked={autoScroll}
                                onChange={(e) => setAutoScroll(e.target.checked)}
                                className="checkbox checkbox-sm checkbox-primary"
                            />
                            {t('common.auto_scroll', '自动滚动')}
                        </label>
                        <button
                            onClick={fetchLogs}
                            className="p-2 text-gray-500 hover:bg-gray-200 dark:hover:bg-gray-700 rounded-lg transition-colors"
                            title={t('common.refresh')}
                        >
                            <RefreshCw size={18} />
                        </button>
                        <button
                            onClick={handleClearLogs}
                            className="flex items-center gap-2 px-4 py-2 bg-red-50 text-red-600 hover:bg-red-100 dark:bg-red-900/20 dark:text-red-400 dark:hover:bg-red-900/40 rounded-lg transition-colors font-medium text-sm"
                        >
                            <Trash2 size={16} />
                            {t('common.clear', '清空')}
                        </button>
                    </div>
                </div>

                <div className="flex gap-4 overflow-hidden" style={{ height: 'calc(100vh - 200px)', minHeight: '400px' }}>
                    {/* 左侧：请求列表 */}
                    <div className="w-1/3 flex flex-col bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-100 dark:border-gray-700 overflow-hidden">
                        <div className="overflow-y-auto flex-1 h-full p-2" ref={listRef}>
                            {logs.length === 0 ? (
                                <div className="flex flex-col items-center justify-center h-full text-gray-400">
                                    <Activity className="w-12 h-12 mb-2 opacity-20" />
                                    <p>等待捕获请求...</p>
                                </div>
                            ) : (
                                <div className="flex flex-col gap-1">
                                    {logs.map(log => (
                                        <button
                                            key={log.id}
                                            onClick={() => setSelectedLogId(log.id)}
                                            className={cn(
                                                "flex flex-col text-left px-3 py-2 rounded-lg transition-colors border-l-4",
                                                selectedLogId === log.id
                                                    ? "bg-blue-50 dark:bg-blue-900/20 border-blue-500"
                                                    : "hover:bg-gray-50 dark:hover:bg-gray-700 border-transparent",
                                                log.status >= 400 ? "border-red-400" : ""
                                            )}
                                        >
                                            <div className="flex items-center justify-between mb-1">
                                                <span className={cn(
                                                    "font-mono text-xs font-bold px-1.5 py-0.5 rounded",
                                                    log.method === 'GET' ? 'bg-green-100 text-green-700' :
                                                        log.method === 'POST' ? 'bg-blue-100 text-blue-700' :
                                                            'bg-gray-100 text-gray-700'
                                                )}>{log.method}</span>
                                                <span className={cn(
                                                    "text-xs font-medium",
                                                    log.status >= 400 ? "text-red-500" : "text-gray-500"
                                                )}>{log.status}</span>
                                            </div>
                                            <div className="truncate text-sm font-medium text-gray-700 dark:text-gray-300">
                                                {log.url}
                                            </div>
                                            <div className="flex items-center gap-1 mt-1 text-xs text-gray-400">
                                                <Clock size={12} />
                                                <span>{log.duration}ms</span>
                                                <span className="ml-auto">{new Date(log.timestamp).toLocaleTimeString()}</span>
                                            </div>
                                        </button>
                                    ))}
                                </div>
                            )}
                        </div>
                    </div>

                    {/* 右侧：请求详情 */}
                    <div className="w-2/3 flex flex-col bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-100 dark:border-gray-700 overflow-hidden">
                        {selectedLog ? (
                            <div className="flex flex-col h-full overflow-y-auto p-4">
                                <div className="mb-4 pb-4 border-b border-gray-100 dark:border-gray-700">
                                    <h2 className="text-lg font-bold text-gray-900 dark:text-white break-all">{selectedLog.url}</h2>
                                    <div className="flex items-center gap-3 mt-2 text-sm">
                                        <span className="font-mono font-bold text-gray-600 dark:text-gray-400">{selectedLog.method}</span>
                                        <span className={selectedLog.status >= 400 ? 'text-red-500 font-bold' : 'text-green-500 font-bold'}>{selectedLog.status}</span>
                                        <span className="text-gray-500">{selectedLog.duration}ms</span>
                                    </div>
                                </div>

                                <div className="grid grid-cols-2 gap-6">
                                    {/* Headers 区 */}
                                    <div className="space-y-4">
                                        <div>
                                            <h3 className="font-semibold text-gray-800 dark:text-gray-200 mb-2 border-b border-gray-100 dark:border-gray-700 pb-1">Request Headers</h3>
                                            <div className="bg-gray-50 dark:bg-gray-900 rounded-md p-2 overflow-x-auto">
                                                {Object.entries(selectedLog.req_headers).map(([k, v]) => (
                                                    <div key={k} className="flex text-xs font-mono py-0.5">
                                                        <span className="text-blue-600 dark:text-blue-400 min-w-[120px] shrink-0 font-semibold">{k}:</span>
                                                        <span className="text-gray-700 dark:text-gray-300 break-all">{v}</span>
                                                    </div>
                                                ))}
                                            </div>
                                        </div>
                                        <div>
                                            <h3 className="font-semibold text-gray-800 dark:text-gray-200 mb-2 border-b border-gray-100 dark:border-gray-700 pb-1">Response Headers</h3>
                                            <div className="bg-gray-50 dark:bg-gray-900 rounded-md p-2 overflow-x-auto">
                                                {Object.entries(selectedLog.res_headers).map(([k, v]) => (
                                                    <div key={k} className="flex text-xs font-mono py-0.5">
                                                        <span className="text-blue-600 dark:text-blue-400 min-w-[120px] shrink-0 font-semibold">{k}:</span>
                                                        <span className="text-gray-700 dark:text-gray-300 break-all">{v}</span>
                                                    </div>
                                                ))}
                                            </div>
                                        </div>
                                    </div>

                                    {/* Body 区 */}
                                    <div className="space-y-4">
                                        <div>
                                            <h3 className="font-semibold text-gray-800 dark:text-gray-200 mb-2 flex items-center gap-2 border-b border-gray-100 dark:border-gray-700 pb-1">
                                                <FileJson size={14} /> Request Body
                                            </h3>
                                            <pre className="bg-gray-50 dark:bg-gray-900 rounded-md p-3 text-xs font-mono text-gray-800 dark:text-gray-200 overflow-x-auto min-h-[100px] whitespace-pre-wrap word-break-all">
                                                {selectedLog.request_body || 'No payload'}
                                            </pre>
                                        </div>
                                        <div>
                                            <h3 className="font-semibold text-gray-800 dark:text-gray-200 mb-2 flex items-center gap-2 border-b border-gray-100 dark:border-gray-700 pb-1">
                                                <FileJson size={14} /> Response Body
                                            </h3>
                                            <pre className="bg-gray-50 dark:bg-gray-900 rounded-md p-3 text-xs font-mono text-gray-800 dark:text-gray-200 overflow-x-auto min-h-[150px] whitespace-pre-wrap word-break-all">
                                                {selectedLog.response_body || 'Empty response'}
                                            </pre>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        ) : (
                            <div className="flex flex-col items-center justify-center h-full text-gray-400">
                                <p>选择左侧请求查看详细及包体</p>
                            </div>
                        )}
                    </div>
                </div>
            </div>
        </div>
    );
}
