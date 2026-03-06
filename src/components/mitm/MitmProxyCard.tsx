import React, { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useTranslation } from 'react-i18next';
import {
    Shield,
    Power,
    RefreshCw,
    Trash2,
    Key,
    FileKey,
    Activity,
    AlertCircle,
    CheckCircle,
    XCircle,
    FolderOpen,
} from 'lucide-react';
import { showToast } from '../common/ToastContainer';
import HelpTooltip from '../common/HelpTooltip';
import { cn } from '../../utils/cn';

interface MitmConfig {
    enabled: boolean;
    port: number;
    root_ca_path: string;
    root_ca_key_path: string;
    target_domains: string[];
    enable_logging: boolean;
    max_logs: number;
}

interface MitmStatus {
    running: boolean;
    port: number;
    proxy_url: string;
    requests_processed: number;
    cert_cache_size: number;
    enable_monitoring: boolean;
    target_domains: string[];
}

interface SpeedStats {
    total_requests: number;
    total_input_tokens: number;
    total_output_tokens: number;
    avg_speed: number;
    stats: Array<{
        model: string;
        request_count: number;
        input_tokens: number;
        output_tokens: number;
        avg_speed: number;
    }>;
}

interface MitmProxyCardProps {
    className?: string;
}

export const MitmProxyCard: React.FC<MitmProxyCardProps> = ({ className }) => {
    const { t } = useTranslation();
    const [status, setStatus] = useState<MitmStatus | null>(null);
    const [config, setConfig] = useState<MitmConfig>({
        enabled: false,
        port: 8081,
        root_ca_path: '',
        root_ca_key_path: '',
        target_domains: ['daily-cloudcode-pa.googleapis.com'],
        enable_logging: true,
        max_logs: 1000,
    });
    const [speedStats, setSpeedStats] = useState<SpeedStats | null>(null);
    const [loading, setLoading] = useState(false);
    const [caValid, setCaValid] = useState<boolean | null>(null);

    // 从后端加载配置
    const loadConfigFromBackend = async () => {
        try {
            const appConfig: any = await invoke('load_config');
            if (appConfig?.mitm) {
                setConfig(appConfig.mitm);
            }
        } catch (error) {
            console.error('Failed to load MITM config from backend:', error);
        }
    };

    // 保存配置到后端
    const saveConfigToBackend = async (newConfig: MitmConfig) => {
        try {
            const appConfig: any = await invoke('load_config');
            appConfig.mitm = newConfig;
            await invoke('save_config', { config: appConfig });
        } catch (error) {
            console.error('Failed to save MITM config to backend:', error);
            // 兜底回落
            localStorage.setItem('mitm_config', JSON.stringify(newConfig));
        }
    };

    useEffect(() => {
        loadConfigFromBackend();
        loadStatus();
        const interval = setInterval(loadStatus, 5000);
        return () => clearInterval(interval);
    }, []);

    useEffect(() => {
        if (status?.running) {
            loadSpeedStats();
        }
    }, [status?.running]);

    const loadStatus = async () => {
        try {
            const s = await invoke<MitmStatus>('get_mitm_proxy_status');
            setStatus(s);
        } catch (error) {
            console.error('Failed to load MITM status:', error);
        }
    };

    const loadSpeedStats = async () => {
        try {
            const stats = await invoke<SpeedStats>('get_mitm_speed_stats');
            setSpeedStats(stats);
        } catch (error) {
            console.error('Failed to load speed stats:', error);
        }
    };

    const handleStart = async () => {
        if (!config.root_ca_path || !config.root_ca_key_path) {
            showToast(t('mitm.error.no_ca_path', '请先设置 Root CA 证书和私钥路径'), 'error');
            return;
        }
        setLoading(true);
        try {
            const newConfig = { ...config, enabled: true };
            setConfig(newConfig);
            saveConfigToBackend(newConfig);
            await invoke('start_mitm_proxy_service', { config: newConfig });
            await loadStatus();
            showToast(t('mitm.success.started', 'MITM 代理已启动'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setLoading(false);
        }
    };

    const handleStop = async () => {
        setLoading(true);
        try {
            const newConfig = { ...config, enabled: false };
            setConfig(newConfig);
            saveConfigToBackend(newConfig);
            await invoke('stop_mitm_proxy_service');
            await loadStatus();
            showToast(t('mitm.success.stopped', 'MITM 代理已停止'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        } finally {
            setLoading(false);
        }
    };

    const handleValidateCa = async () => {
        if (!config.root_ca_path || !config.root_ca_key_path) {
            showToast(t('mitm.error.no_ca_path', '请先设置 Root CA 证书和私钥路径'), 'error');
            return;
        }
        try {
            const result = await invoke<boolean>('validate_mitm_root_ca', {
                certPath: config.root_ca_path,
                keyPath: config.root_ca_key_path,
            });
            setCaValid(result);
            if (result) {
                showToast(t('mitm.success.ca_valid', 'Root CA 验证成功'), 'success');
            } else {
                showToast(t('mitm.error.ca_invalid', 'Root CA 验证失败'), 'error');
            }
        } catch (error) {
            setCaValid(false);
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleClearStats = async () => {
        try {
            await invoke('clear_mitm_speed_stats');
            setSpeedStats(null);
            showToast(t('common.success'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleClearCertCache = async () => {
        try {
            await invoke('clear_mitm_cert_cache');
            await loadStatus();
            showToast(t('common.success'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    const handleToggleMonitoring = async () => {
        try {
            const newEnabled = !status?.enable_monitoring;
            await invoke('set_mitm_monitor_enabled', { enabled: newEnabled });
            setStatus(prev => prev ? { ...prev, enable_monitoring: newEnabled } : prev);
            showToast(t('common.success'), 'success');
        } catch (error) {
            showToast(`${t('common.error')}: ${error}`, 'error');
        }
    };

    return (
        <div className={cn('bg-white dark:bg-base-100 rounded-xl shadow-sm border border-gray-100 dark:border-gray-700/50 overflow-hidden', className)}>
            {/* Header */}
            <div className="px-5 py-4 flex items-center justify-between bg-gray-50/50 dark:bg-gray-800/50 border-b border-gray-100 dark:border-gray-700/50">
                <div className="flex items-center gap-3">
                    <div className="text-gray-500 dark:text-gray-400">
                        <Shield size={20} />
                    </div>
                    <div className="flex flex-col">
                        <span className="font-semibold text-sm text-gray-900 dark:text-gray-100">
                            {t('mitm.title', 'MITM 代理监控')}
                        </span>
                        <span className="text-xs text-gray-500 dark:text-gray-400">
                            {t('mitm.subtitle', '监控 Antigravity 内部 API 调用')}
                        </span>
                    </div>
                    {status?.running && (
                        <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-green-100 dark:bg-green-900/40 text-green-700 dark:text-green-400 text-xs">
                            <div className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
                            {t('common.running', '运行中')}
                        </div>
                    )}
                </div>

                <div className="flex items-center gap-2">
                    <button
                        onClick={loadStatus}
                        className="p-1.5 rounded-lg hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
                        title={t('common.refresh')}
                    >
                        <RefreshCw size={16} className={loading ? 'animate-spin' : ''} />
                    </button>
                    {status?.running ? (
                        <button
                            onClick={handleStop}
                            disabled={loading}
                            className="px-3 py-1.5 rounded-lg text-xs font-medium bg-red-50 text-red-600 hover:bg-red-100 border border-red-200 transition-colors flex items-center gap-1.5"
                        >
                            <Power size={14} />
                            {t('common.stop', '停止')}
                        </button>
                    ) : (
                        <button
                            onClick={handleStart}
                            disabled={loading}
                            className="px-3 py-1.5 rounded-lg text-xs font-medium bg-blue-600 hover:bg-blue-700 text-white transition-colors flex items-center gap-1.5"
                        >
                            <Power size={14} />
                            {t('common.start', '启动')}
                        </button>
                    )}
                </div>
            </div>

            {/* Content */}
            <div className="p-5 space-y-4">
                {/* Configuration */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                    {/* Port */}
                    <div>
                        <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                            <span className="inline-flex items-center gap-1">
                                {t('mitm.config.port', '监听端口')}
                                <HelpTooltip
                                    text={t('mitm.config.port_tooltip', 'MITM 代理监听的本地端口')}
                                    ariaLabel="Port"
                                    placement="top"
                                />
                            </span>
                        </label>
                        <input
                            type="number"
                            value={config.port}
                            onChange={(e) => {
                                const newConfig = { ...config, port: parseInt(e.target.value) || 8081 };
                                setConfig(newConfig);
                                saveConfigToBackend(newConfig);
                            }}
                            disabled={status?.running}
                            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-800 text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
                        />
                    </div>

                    {/* Target Domains */}
                    <div>
                        <label className="block text-xs font-medium text-gray-700 dark:text-gray-300 mb-1">
                            <span className="inline-flex items-center gap-1">
                                {t('mitm.config.domains', '目标域名')}
                                <HelpTooltip
                                    text={t('mitm.config.domains_tooltip', '需要拦截的目标域名列表')}
                                    ariaLabel="Domains"
                                    placement="top"
                                />
                            </span>
                        </label>
                        <input
                            type="text"
                            value={(config.target_domains || []).join(', ')}
                            onChange={(e) => {
                                const newConfig = { ...config, target_domains: e.target.value.split(',').map(d => d.trim()).filter(Boolean) };
                                setConfig(newConfig);
                                saveConfigToBackend(newConfig);
                            }}
                            disabled={status?.running}
                            className="w-full px-3 py-2 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-800 text-sm focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
                            placeholder="daily-cloudcode-pa.googleapis.com"
                        />
                    </div>
                </div>

                {/* Root CA Configuration */}
                <div className="p-4 bg-gray-50 dark:bg-gray-800/50 rounded-lg border border-gray-200 dark:border-gray-700">
                    <div className="flex items-center justify-between mb-3">
                        <div className="flex items-center gap-2">
                            <FileKey size={16} className="text-gray-500" />
                            <span className="text-sm font-medium text-gray-700 dark:text-gray-300">
                                {t('mitm.config.root_ca', 'Root CA 配置')}
                            </span>
                        </div>
                        {caValid !== null && (
                            <div className={cn('flex items-center gap-1 text-xs', caValid ? 'text-green-600' : 'text-red-600')}>
                                {caValid ? <CheckCircle size={14} /> : <XCircle size={14} />}
                                {caValid ? t('mitm.status.valid', '有效') : t('mitm.status.invalid', '无效')}
                            </div>
                        )}
                    </div>

                    <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                        <div>
                            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">
                                {t('mitm.config.cert_path', '证书路径 (PEM)')}
                            </label>
                            <div className="flex gap-2">
                                <input
                                    type="text"
                                    value={config.root_ca_path}
                                    onChange={(e) => {
                                        setConfig({ ...config, root_ca_path: e.target.value });
                                        setCaValid(null);
                                    }}
                                    disabled={status?.running}
                                    className="flex-1 px-3 py-1.5 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-800 text-xs focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
                                    placeholder="/path/to/root-ca.crt"
                                />
                                <button
                                    onClick={async () => {
                                        const selected = await open({
                                            multiple: false,
                                            filters: [{ name: 'PEM Certificate', extensions: ['pem', 'crt', 'cer'] }],
                                        });
                                        if (selected && typeof selected === 'string') {
                                            const newConfig = { ...config, root_ca_path: selected };
                                            setConfig(newConfig);
                                            saveConfigToBackend(newConfig);
                                            setCaValid(null);
                                        }
                                    }}
                                    disabled={status?.running}
                                    className="px-2 py-1.5 rounded-lg text-xs font-medium bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors disabled:opacity-50"
                                    title={t('mitm.action.select_file', '选择文件')}
                                >
                                    <FolderOpen size={14} />
                                </button>
                            </div>
                        </div>
                        <div>
                            <label className="block text-xs text-gray-500 dark:text-gray-400 mb-1">
                                {t('mitm.config.key_path', '私钥路径 (PEM)')}
                            </label>
                            <div className="flex gap-2">
                                <input
                                    type="text"
                                    value={config.root_ca_key_path}
                                    onChange={(e) => {
                                        setConfig({ ...config, root_ca_key_path: e.target.value });
                                        setCaValid(null);
                                    }}
                                    disabled={status?.running}
                                    className="flex-1 px-3 py-1.5 border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-800 text-xs focus:ring-2 focus:ring-blue-500 focus:border-transparent disabled:opacity-50"
                                    placeholder="/path/to/root-ca.key"
                                />
                                <button
                                    onClick={async () => {
                                        const selected = await open({
                                            multiple: false,
                                            filters: [{ name: 'PEM Private Key', extensions: ['pem', 'key'] }],
                                        });
                                        if (selected && typeof selected === 'string') {
                                            const newConfig = { ...config, root_ca_key_path: selected };
                                            setConfig(newConfig);
                                            saveConfigToBackend(newConfig);
                                            setCaValid(null);
                                        }
                                    }}
                                    disabled={status?.running}
                                    className="px-2 py-1.5 rounded-lg text-xs font-medium bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors disabled:opacity-50"
                                    title={t('mitm.action.select_file', '选择文件')}
                                >
                                    <FolderOpen size={14} />
                                </button>
                            </div>
                        </div>
                    </div>

                    <button
                        onClick={handleValidateCa}
                        disabled={!config.root_ca_path || !config.root_ca_key_path || status?.running}
                        className="mt-3 px-3 py-1.5 rounded-lg text-xs font-medium bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors flex items-center gap-1.5 disabled:opacity-50"
                    >
                        <Key size={12} />
                        {t('mitm.action.validate_ca', '验证 Root CA')}
                    </button>
                </div>

                {/* Status & Stats */}
                {status?.running && (
                    <div className="space-y-3">
                        {/* Quick Stats */}
                        <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                            <div className="p-3 bg-blue-50 dark:bg-blue-900/30 rounded-lg">
                                <div className="text-xs text-blue-600 dark:text-blue-400 font-medium">
                                    {t('mitm.stats.port', '端口')}
                                </div>
                                <div className="text-lg font-bold text-blue-700 dark:text-blue-300">
                                    {status.port}
                                </div>
                            </div>
                            <div className="p-3 bg-green-50 dark:bg-green-900/30 rounded-lg">
                                <div className="text-xs text-green-600 dark:text-green-400 font-medium">
                                    {t('mitm.stats.certs', '证书缓存')}
                                </div>
                                <div className="text-lg font-bold text-green-700 dark:text-green-300">
                                    {status.cert_cache_size}
                                </div>
                            </div>
                            <div className="p-3 bg-purple-50 dark:bg-purple-900/30 rounded-lg">
                                <div className="text-xs text-purple-600 dark:text-purple-400 font-medium">
                                    {t('mitm.stats.requests', '请求数')}
                                </div>
                                <div className="text-lg font-bold text-purple-700 dark:text-purple-300">
                                    {status.requests_processed || 0}
                                </div>
                            </div>
                            <div className="p-3 bg-orange-50 dark:bg-orange-900/30 rounded-lg">
                                <div className="text-xs text-orange-600 dark:text-orange-400 font-medium">
                                    {t('mitm.stats.speed', '平均速度')}
                                </div>
                                <div className="text-lg font-bold text-orange-700 dark:text-orange-300">
                                    {(speedStats?.avg_speed || 0).toFixed(1)}
                                    <span className="text-xs ml-1">t/s</span>
                                </div>
                            </div>
                        </div>

                        {/* Token Stats */}
                        {speedStats && (speedStats.total_input_tokens > 0 || speedStats.total_output_tokens > 0) && (
                            <div className="p-3 bg-gray-50 dark:bg-gray-800/50 rounded-lg">
                                <div className="flex items-center justify-between mb-2">
                                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400">
                                        {t('mitm.stats.tokens', 'Token 统计')}
                                    </span>
                                </div>
                                <div className="flex gap-4">
                                    <div className="flex items-center gap-2">
                                        <span className="text-xs text-gray-500">{t('mitm.stats.input', '输入')}:</span>
                                        <span className="text-sm font-bold text-blue-600 dark:text-blue-400">
                                            {speedStats.total_input_tokens.toLocaleString()}
                                        </span>
                                    </div>
                                    <div className="flex items-center gap-2">
                                        <span className="text-xs text-gray-500">{t('mitm.stats.output', '输出')}:</span>
                                        <span className="text-sm font-bold text-green-600 dark:text-green-400">
                                            {speedStats.total_output_tokens.toLocaleString()}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        )}

                        {/* Model Stats */}
                        {speedStats && speedStats.stats && speedStats.stats.length > 0 && (
                            <div className="p-3 bg-gray-50 dark:bg-gray-800/50 rounded-lg">
                                <div className="flex items-center justify-between mb-2">
                                    <span className="text-xs font-medium text-gray-600 dark:text-gray-400">
                                        {t('mitm.stats.by_model', '按模型统计')}
                                    </span>
                                </div>
                                <div className="space-y-2 max-h-40 overflow-y-auto">
                                    {speedStats.stats.map((stat, idx) => (
                                        <div key={idx} className="flex items-center justify-between text-xs">
                                            <span className="font-mono text-gray-700 dark:text-gray-300 truncate max-w-[200px]">
                                                {stat.model}
                                            </span>
                                            <div className="flex gap-3 text-gray-500">
                                                <span>{stat.request_count} reqs</span>
                                                <span>{stat.avg_speed.toFixed(1)} t/s</span>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            </div>
                        )}

                        {/* Actions */}
                        <div className="flex flex-wrap gap-2">
                            <button
                                onClick={handleToggleMonitoring}
                                className={cn(
                                    'px-3 py-1.5 rounded-lg text-xs font-medium transition-colors flex items-center gap-1.5',
                                    status.enable_monitoring
                                        ? 'bg-green-100 text-green-700 dark:bg-green-900/40 dark:text-green-400'
                                        : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-300'
                                )}
                            >
                                <Activity size={12} />
                                {status.enable_monitoring ? t('mitm.monitoring.enabled', '监控已启用') : t('mitm.monitoring.disabled', '监控已禁用')}
                            </button>
                            <button
                                onClick={handleClearStats}
                                className="px-3 py-1.5 rounded-lg text-xs font-medium bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors flex items-center gap-1.5"
                            >
                                <Trash2 size={12} />
                                {t('mitm.action.clear_stats', '清除统计')}
                            </button>
                            <button
                                onClick={handleClearCertCache}
                                className="px-3 py-1.5 rounded-lg text-xs font-medium bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 transition-colors flex items-center gap-1.5"
                            >
                                <RefreshCw size={12} />
                                {t('mitm.action.clear_cache', '清除证书缓存')}
                            </button>
                        </div>
                    </div>
                )}

                {/* Usage Hint */}
                <div className="p-3 bg-amber-50 dark:bg-amber-900/30 rounded-lg border border-amber-200 dark:border-amber-800/50">
                    <div className="flex items-start gap-2">
                        <AlertCircle size={14} className="text-amber-600 dark:text-amber-400 mt-0.5 flex-shrink-0" />
                        <div className="text-xs text-amber-700 dark:text-amber-300">
                            <p className="font-medium mb-1">{t('mitm.hint.title', '使用说明')}</p>
                            <ol className="list-decimal list-inside space-y-0.5 text-amber-600 dark:text-amber-400">
                                <li>{t('mitm.hint.step1', '准备 ECDSA Root CA 证书和私钥 (仅支持 ECDSA P-256/P-384)')}
                                    <div className="ml-4 mt-1">
                                        <a href="#" className="text-amber-700 dark:text-amber-300 underline text-xs" onClick={(e) => { e.preventDefault(); window.open('https://github.com/your-repo/antigravity-tools/tree/main/scripts', '_blank'); }}>
                                            查看证书生成脚本
                                        </a>
                                    </div>
                                </li>
                                <li>{t('mitm.hint.step2', '将 Root CA 证书安装到系统受信任的根证书颁发机构')}</li>
                                <li>{t('mitm.hint.step3', `配置 Antigravity 使用代理地址: 127.0.0.1:{{port}}`, { port: config.port })}</li>
                                <li>{t('mitm.hint.step4', '启动 MITM 代理后，所有目标域名的 HTTPS 请求将被拦截和监控')}</li>
                            </ol>

                            <div className="mt-2 pt-2 border-t border-amber-300/30 dark:border-amber-700/30">
                                <p className="font-medium mb-1 text-amber-700 dark:text-amber-300">
                                    {t('mitm.hint.cert_install', '证书安装命令:')}
                                </p>
                                <div className="space-y-1">
                                    <div>
                                        <p className="font-medium text-amber-600 dark:text-amber-400">
                                            {t('mitm.hint.windows_cmd', 'Windows (管理员权限):')}
                                        </p>
                                        <code className="block bg-amber-100 dark:bg-amber-900/50 px-2 py-1 rounded text-amber-800 dark:text-amber-200 text-xs mt-0.5">
                                            certutil -addstore Root "{config.root_ca_path || 'path\\to\\ca-cert.pem'}"
                                        </code>
                                    </div>
                                    <div>
                                        <p className="font-medium text-amber-600 dark:text-amber-400">
                                            {t('mitm.hint.macos_cmd', 'macOS:')}
                                        </p>
                                        <code className="block bg-amber-100 dark:bg-amber-900/50 px-2 py-1 rounded text-amber-800 dark:text-amber-200 text-xs mt-0.5">
                                            sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "{config.root_ca_path || 'path/to/ca-cert.pem'}"
                                        </code>
                                    </div>
                                    <div>
                                        <p className="font-medium text-amber-600 dark:text-amber-400">
                                            {t('mitm.hint.linux_cmd', 'Linux:')}
                                        </p>
                                        <code className="block bg-amber-100 dark:bg-amber-900/50 px-2 py-1 rounded text-amber-800 dark:text-amber-200 text-xs mt-0.5">
                                            sudo cp "{config.root_ca_path || 'path/to/ca-cert.pem'}" /usr/local/share/ca-certificates/antigravity-ca.crt<br />
                                            sudo update-ca-certificates
                                        </code>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
};

export default MitmProxyCard;
