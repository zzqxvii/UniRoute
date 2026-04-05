import { useState, useEffect, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface RequestLog {
  id: number;
  timestamp: string;
  method: string;
  path: string;
  requested_model: string | null;
  model: string | null;
  provider: string | null;
  provider_prefix: string | null;
  url: string | null;
  protocol_transform: string | null;
  status_code: number;
  latency_ms: number;
  first_token_ms: number | null;
  prompt_tokens: number | null;
  completion_tokens: number | null;
  cost: number | null;
  error: string | null;
  original_request_body: string | null;
  request_body: string | null;
  original_response_body: string | null;
  response_body: string | null;
}

interface RequestStats {
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  total_tokens: number;
  total_cost: number;
  avg_latency_ms: number;
}

// Provider 颜色映射
const PROVIDER_COLORS: Record<string, { backgroundColor: string; color: string; label: string }> = {
  openai: { backgroundColor: '#10a37f', color: '#fff', label: 'OpenAI' },
  anthropic: { backgroundColor: '#d97706', color: '#fff', label: 'Anthropic' },
  azure: { backgroundColor: '#0078d4', color: '#fff', label: 'Azure' },
  google: { backgroundColor: '#4285f4', color: '#fff', label: 'Google' },
  deepseek: { backgroundColor: '#6366f1', color: '#fff', label: 'DeepSeek' },
  moonshot: { backgroundColor: '#ec4899', color: '#fff', label: 'Moonshot' },
  zhipu: { backgroundColor: '#8b5cf6', color: '#fff', label: '智谱' },
  qwen: { backgroundColor: '#f97316', color: '#fff', label: '通义' },
  baidu: { backgroundColor: '#3b82f6', color: '#fff', label: '百度' },
  siliconflow: { backgroundColor: '#14b8a6', color: '#fff', label: 'SiliconFlow' },
  groq: { backgroundColor: '#f59e0b', color: '#fff', label: 'Groq' },
  mistral: { backgroundColor: '#f97316', color: '#fff', label: 'Mistral' },
  cohere: { backgroundColor: '#d97706', color: '#fff', label: 'Cohere' },
  openrouter: { backgroundColor: '#6366f1', color: '#fff', label: 'OpenRouter' },
  api2d: { backgroundColor: '#10b981', color: '#fff', label: 'API2D' },
};

// 列定义
const COLUMNS = [
  { key: 'status', label: '状态' },
  { key: 'requested', label: '请求名称' },
  { key: 'model', label: '实际模型' },
  { key: 'provider', label: '供应商' },
  { key: 'transform', label: '协议转换' },
  { key: 'tokens', label: 'Tokens' },
  { key: 'cost', label: '成本' },
  { key: 'latency', label: '延迟' },
  { key: 'firstToken', label: '首Token' },
  { key: 'speed', label: '速度' },
  { key: 'time', label: '时间' },
];

const DEFAULT_VISIBLE = Object.fromEntries(COLUMNS.map((c) => [c.key, true]));

// 状态样式
function getStatusStyle(status: number): React.CSSProperties {
  if (status >= 200 && status < 300) {
    return { backgroundColor: '#10b981', color: '#fff' };
  }
  if (status >= 400 && status < 500) {
    return { backgroundColor: '#f59e0b', color: '#fff' };
  }
  if (status >= 500) {
    return { backgroundColor: '#ef4444', color: '#fff' };
  }
  return { backgroundColor: '#6b7280', color: '#fff' };
}

// 格式化时间
function formatTime(iso: string) {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  } catch {
    return iso;
  }
}

function formatDateTime(iso: string) {
  try {
    const d = new Date(iso);
    return d.toLocaleString('zh-CN');
  } catch {
    return iso;
  }
}

// 格式化延迟
function formatLatency(ms: number) {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

// 格式化 JSON
function formatJson(str: string | null) {
  if (!str) return null;
  try {
    return JSON.stringify(JSON.parse(str), null, 2);
  } catch {
    return str;
  }
}

// 导出数据为 JSON
function exportLogsAsJson(logs: RequestLog[], stats: RequestStats | null) {
  const data = {
    exported_at: new Date().toISOString(),
    stats,
    logs: logs.map((log) => ({
      ...log,
      original_request_body: log.original_request_body ? JSON.parse(log.original_request_body) : null,
      request_body: log.request_body ? JSON.parse(log.request_body) : null,
      original_response_body: log.original_response_body ? JSON.parse(log.original_response_body) : null,
      response_body: log.response_body ? JSON.parse(log.response_body) : null,
    })),
  };
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `uniroute-logs-${new Date().toISOString().slice(0, 10)}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

function Logs() {
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [stats, setStats] = useState<RequestStats | null>(null);
  const [loading, setLoading] = useState(false);

  // 筛选条件
  const [searchQuery, setSearchQuery] = useState('');
  const [filterProvider, setFilterProvider] = useState('');
  const [filterStatus, setFilterStatus] = useState('');
  const [sortBy, setSortBy] = useState('newest');

  // 显示设置
  const [recording, setRecording] = useState(true);
  const [visibleColumns, setVisibleColumns] = useState<Record<string, boolean>>(() => {
    try {
      const saved = localStorage.getItem('loggerVisibleColumns');
      return saved ? { ...DEFAULT_VISIBLE, ...JSON.parse(saved) } : DEFAULT_VISIBLE;
    } catch {
      return DEFAULT_VISIBLE;
    }
  });

  // 详情弹窗
  const [selectedLog, setSelectedLog] = useState<RequestLog | null>(null);

  // 切换列可见性
  const toggleColumn = useCallback((key: string) => {
    setVisibleColumns((prev) => {
      const next = { ...prev, [key]: !prev[key] };
      localStorage.setItem('loggerVisibleColumns', JSON.stringify(next));
      return next;
    });
  }, []);

  // 加载数据
  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const [logsResult, statsResult] = await Promise.all([
        invoke<RequestLog[]>('get_request_logs', {
          limit: 500,
          offset: 0,
          model: searchQuery || null,
          status: filterStatus ? parseInt(filterStatus) : null,
          dateFrom: null,
          dateTo: null,
        }),
        invoke<RequestStats>('get_request_stats'),
      ]);

      // 应用 Provider 筛选
      let filtered = logsResult;
      if (filterProvider) {
        filtered = filtered.filter((l) => l.provider === filterProvider);
      }

      setLogs(filtered);
      setStats(statsResult);
    } catch (error) {
      console.error('Failed to load logs:', error);
    } finally {
      setLoading(false);
    }
  }, [searchQuery, filterStatus, filterProvider]);

  useEffect(() => {
    loadData();
  }, [loadData]);

  // 自动刷新
  useEffect(() => {
    if (!recording) return;
    const interval = setInterval(loadData, 5000);
    return () => clearInterval(interval);
  }, [recording, loadData]);

  // 清空日志
  const handleClear = async () => {
    if (confirm('确定清空所有日志？此操作不可恢复。')) {
      try {
        await invoke('clear_request_logs');
        loadData();
      } catch (error) {
        alert('清空失败: ' + error);
      }
    }
  };

  // 排序后的日志
  const sortedLogs = useMemo(() => {
    const arr = [...logs];
    switch (sortBy) {
      case 'oldest':
        return arr.sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime());
      case 'latency_desc':
        return arr.sort((a, b) => b.latency_ms - a.latency_ms);
      case 'latency_asc':
        return arr.sort((a, b) => a.latency_ms - b.latency_ms);
      case 'status_desc':
        return arr.sort((a, b) => b.status_code - a.status_code);
      case 'status_asc':
        return arr.sort((a, b) => a.status_code - b.status_code);
      case 'tokens_desc':
        return arr.sort((a, b) => {
          const ta = (a.prompt_tokens || 0) + (a.completion_tokens || 0);
          const tb = (b.prompt_tokens || 0) + (b.completion_tokens || 0);
          return tb - ta;
        });
      case 'newest':
      default:
        return arr.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime());
    }
  }, [logs, sortBy]);

  // 唯一 Provider 列表
  const uniqueProviders = useMemo(() => {
    return [...new Set(logs.map((l) => l.provider).filter(Boolean))] as string[];
  }, [logs]);

  // 统计
  const okCount = logs.filter((l) => l.status_code >= 200 && l.status_code < 300).length;
  const errorCount = logs.filter((l) => l.status_code >= 400).length;

  return (
    <div className="px-4 py-6 sm:px-0 space-y-4">
      {/* 头部 */}
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">请求日志</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">查看代理请求记录和详情</p>
        </div>
        <div className="flex items-center gap-2">
          {/* 录制开关 */}
          <button
            onClick={() => setRecording(!recording)}
            className={`flex items-center gap-2 px-3 py-1.5 rounded-full text-sm font-medium border transition-colors ${
              recording
                ? 'bg-red-500/10 border-red-500/30 text-red-700 dark:text-red-400'
                : 'bg-gray-100 dark:bg-gray-800 border-gray-300 dark:border-gray-600 text-gray-600 dark:text-gray-400'
            }`}
          >
            <span className={`w-2 h-2 rounded-full ${recording ? 'bg-red-500 animate-pulse' : 'bg-gray-400'}`} />
            {recording ? '录制中' : '已暂停'}
          </button>

          <button onClick={loadData} className="btn-secondary">
            刷新
          </button>
          <button onClick={() => exportLogsAsJson(sortedLogs, stats)} className="btn-secondary">
            导出
          </button>
          <button onClick={handleClear} className="btn-danger">
            清空
          </button>
        </div>
      </div>

      {/* 统计卡片 */}
      {stats && (
        <div className="grid grid-cols-2 sm:grid-cols-6 gap-4">
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-gray-900 dark:text-white">{stats.total_requests}</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">总请求</p>
          </div>
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-green-600 dark:text-green-400">{stats.successful_requests}</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">成功</p>
          </div>
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-red-600 dark:text-red-400">{stats.failed_requests}</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">失败</p>
          </div>
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-gray-900 dark:text-white">{stats.total_tokens.toLocaleString()}</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">总 Tokens</p>
          </div>
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-amber-600 dark:text-amber-400">${stats.total_cost.toFixed(4)}</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">总成本</p>
          </div>
          <div className="card-base p-4 text-center">
            <p className="text-2xl font-bold text-gray-900 dark:text-white">{stats.avg_latency_ms.toFixed(0)}ms</p>
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">平均延迟</p>
          </div>
        </div>
      )}

      {/* 筛选条件 */}
      <div className="card-base p-4">
        <div className="flex flex-wrap items-center gap-3">
          {/* 搜索框 */}
          <div className="flex-1 min-w-[200px] relative">
            <svg
              className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-400"
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
            </svg>
            <input
              type="text"
              placeholder="搜索模型..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="w-full pl-10 pr-4 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 border-0 text-gray-900 dark:text-white placeholder:text-gray-400 focus:ring-2 focus:ring-indigo-500"
            />
          </div>

          {/* Provider 筛选 */}
          <select
            value={filterProvider}
            onChange={(e) => setFilterProvider(e.target.value)}
            className="px-3 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 border-0 text-gray-900 dark:text-white focus:ring-2 focus:ring-indigo-500"
          >
            <option value="">全部 Provider</option>
            {uniqueProviders.map((p) => {
              const colors = PROVIDER_COLORS[p] || { bg: '#6b7280', text: '#fff', label: p };
              return (
                <option key={p} value={p}>
                  {colors.label}
                </option>
              );
            })}
          </select>

          {/* 状态筛选 */}
          <select
            value={filterStatus}
            onChange={(e) => setFilterStatus(e.target.value)}
            className="px-3 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 border-0 text-gray-900 dark:text-white focus:ring-2 focus:ring-indigo-500"
          >
            <option value="">全部状态</option>
            <option value="200">200 成功</option>
            <option value="400">400 错误</option>
            <option value="401">401 认证失败</option>
            <option value="429">429 限流</option>
            <option value="500">500 服务器错误</option>
          </select>

          {/* 排序 */}
          <select
            value={sortBy}
            onChange={(e) => setSortBy(e.target.value)}
            className="px-3 py-2 text-sm rounded-lg bg-gray-100 dark:bg-gray-700 border-0 text-gray-900 dark:text-white focus:ring-2 focus:ring-indigo-500"
          >
            <option value="newest">最新优先</option>
            <option value="oldest">最早优先</option>
            <option value="latency_desc">延迟最高</option>
            <option value="latency_asc">延迟最低</option>
            <option value="tokens_desc">Tokens 最高</option>
            <option value="status_desc">状态码最高</option>
            <option value="status_asc">状态码最低</option>
          </select>

          {/* 统计 */}
          <div className="flex items-center gap-2 text-xs">
            <span className="px-2 py-1 rounded bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 font-mono">
              {sortedLogs.length} 条
            </span>
            <span className="px-2 py-1 rounded bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 font-mono">
              {okCount} OK
            </span>
            {errorCount > 0 && (
              <span className="px-2 py-1 rounded bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400 font-mono">
                {errorCount} ERR
              </span>
            )}
          </div>
        </div>

        {/* 列可见性切换 */}
        <div className="flex flex-wrap items-center gap-1.5 mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
          <span className="text-[10px] text-gray-400 uppercase tracking-wider mr-1">列显示</span>
          {COLUMNS.map((col) => (
            <button
              key={col.key}
              onClick={() => toggleColumn(col.key)}
              className={`px-2 py-0.5 rounded text-[10px] font-medium border transition-all ${
                visibleColumns[col.key]
                  ? 'bg-indigo-500/15 text-indigo-600 dark:text-indigo-400 border-indigo-500/30'
                  : 'bg-gray-100 dark:bg-gray-700 text-gray-400 dark:text-gray-500 border-gray-200 dark:border-gray-600 opacity-50 hover:opacity-80'
              }`}
            >
              {col.label}
            </button>
          ))}
        </div>

        {/* Provider 快捷筛选 */}
        {uniqueProviders.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5 mt-3 pt-3 border-t border-gray-200 dark:border-gray-700">
            <span className="text-[10px] text-gray-400 uppercase tracking-wider mr-1">Provider</span>
            {uniqueProviders.map((p) => {
              const colors = PROVIDER_COLORS[p] || { backgroundColor: '#6b7280', color: '#fff', label: p };
              const isActive = filterProvider === p;
              return (
                <button
                  key={p}
                  onClick={() => setFilterProvider(isActive ? '' : p)}
                  className={`px-2.5 py-1 rounded-full text-[10px] font-bold uppercase transition-all ${
                    isActive ? 'ring-2 ring-white/30' : 'opacity-60 hover:opacity-100'
                  }`}
                  style={{
                    backgroundColor: isActive ? colors.backgroundColor : `${colors.backgroundColor}33`,
                    color: isActive ? colors.color : colors.backgroundColor,
                  }}
                >
                  {colors.label}
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* 日志列表 */}
      {loading && logs.length === 0 ? (
        <div className="card-base p-8 text-center text-gray-500">加载中...</div>
      ) : logs.length === 0 ? (
        <div className="card-base p-8 text-center">
          <svg className="w-12 h-12 mx-auto mb-4 text-gray-300 dark:text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
          </svg>
          <p className="text-gray-500 dark:text-gray-400">暂无请求日志</p>
          <p className="text-sm text-gray-400 dark:text-gray-500 mt-2">
            启动代理并发送请求后，日志将显示在这里
          </p>
        </div>
      ) : (
        <div className="card-base overflow-hidden">
          <div className="overflow-x-auto max-h-[calc(100vh-400px)] overflow-y-auto">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
              <thead className="bg-gray-50 dark:bg-gray-900/50 sticky top-0 z-10">
                <tr>
                  {visibleColumns.status && (
                    <th className="px-4 py-3 text-left text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      状态
                    </th>
                  )}
                  {visibleColumns.requested && (
                    <th className="px-4 py-3 text-left text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      请求名称
                    </th>
                  )}
                  {visibleColumns.model && (
                    <th className="px-4 py-3 text-left text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      实际模型
                    </th>
                  )}
                  {visibleColumns.provider && (
                    <th className="px-4 py-3 text-left text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Provider
                    </th>
                  )}
                  {visibleColumns.transform && (
                    <th className="px-4 py-3 text-left text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      协议转换
                    </th>
                  )}
                  {visibleColumns.tokens && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      Tokens
                    </th>
                  )}
                  {visibleColumns.cost && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      成本
                    </th>
                  )}
                  {visibleColumns.latency && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      延迟
                    </th>
                  )}
                  {visibleColumns.firstToken && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      首Token
                    </th>
                  )}
                  {visibleColumns.speed && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      速度
                    </th>
                  )}
                  {visibleColumns.time && (
                    <th className="px-4 py-3 text-right text-[10px] font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">
                      时间
                    </th>
                  )}
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
                {sortedLogs.map((log) => {
                  const statusStyle = getStatusStyle(log.status_code);
                  const providerColors = PROVIDER_COLORS[log.provider || ''] || {
                    backgroundColor: '#6b7280',
                    color: '#fff',
                    label: log.provider || '-',
                  };
                  const isError = log.status_code >= 400;

                  return (
                    <tr
                      key={log.id}
                      onClick={() => setSelectedLog(log)}
                      className={`cursor-pointer hover:bg-indigo-50 dark:hover:bg-indigo-900/10 transition-colors ${
                        isError ? 'bg-red-50 dark:bg-red-900/5' : ''
                      }`}
                    >
                      {visibleColumns.status && (
                        <td className="px-4 py-2.5">
                          <span
                            className="inline-flex items-center justify-center px-2 py-0.5 rounded text-[10px] font-bold min-w-[40px]"
                            style={statusStyle}
                          >
                            {log.status_code || '...'}
                          </span>
                        </td>
                      )}
                      {visibleColumns.requested && (
                        <td className="px-4 py-2.5 text-xs font-mono">
                          <span className={
                            log.requested_model && log.model && log.requested_model !== log.model
                              ? 'text-amber-600 dark:text-amber-400'
                              : 'text-gray-500 dark:text-gray-400'
                          }>
                            {log.requested_model || '-'}
                          </span>
                        </td>
                      )}
                      {visibleColumns.model && (
                        <td className="px-4 py-2.5 text-xs font-medium text-indigo-600 dark:text-indigo-400 font-mono">
                          {log.model || '-'}
                        </td>
                      )}
                      {visibleColumns.provider && (
                        <td className="px-4 py-2.5">
                          <span
                            className="inline-block px-2 py-0.5 rounded text-[9px] font-bold uppercase"
                            style={providerColors}
                          >
                            {providerColors.label}
                          </span>
                        </td>
                      )}
                      {visibleColumns.transform && (
                        <td className="px-4 py-2.5">
                          {log.protocol_transform ? (
                            <span className={`inline-block px-2 py-0.5 rounded text-[9px] font-bold ${
                              log.protocol_transform === 'direct'
                                ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
                                : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400'
                            }`}>
                              {log.protocol_transform}
                            </span>
                          ) : (
                            <span className="text-gray-300 dark:text-gray-600">-</span>
                          )}
                        </td>
                      )}
                      {visibleColumns.tokens && (
                        <td className="px-4 py-2.5 text-right whitespace-nowrap text-xs">
                          {log.prompt_tokens != null && log.completion_tokens != null ? (
                            <>
                              <span className="text-gray-400">I:</span>{' '}
                              <span className="text-indigo-600 dark:text-indigo-400">{log.prompt_tokens}</span>
                              <span className="mx-1 text-gray-300 dark:text-gray-600">|</span>
                              <span className="text-gray-400">O:</span>{' '}
                              <span className="text-green-600 dark:text-green-400">{log.completion_tokens}</span>
                            </>
                          ) : (
                            <span className="text-gray-300 dark:text-gray-600">-</span>
                          )}
                        </td>
                      )}
                      {visibleColumns.cost && (
                        <td className="px-4 py-2.5 text-right text-xs font-mono">
                          {log.cost != null && log.cost > 0 ? (
                            <span className="text-amber-600 dark:text-amber-400">${log.cost.toFixed(6)}</span>
                          ) : (
                            <span className="text-gray-300 dark:text-gray-600">-</span>
                          )}
                        </td>
                      )}
                      {visibleColumns.latency && (
                        <td className="px-4 py-2.5 text-right text-xs text-gray-500 dark:text-gray-400 font-mono">
                          {formatLatency(log.latency_ms)}
                        </td>
                      )}
                      {visibleColumns.firstToken && (
                        <td className="px-4 py-2.5 text-right text-xs font-mono">
                          {log.first_token_ms ? (
                            <span className="text-cyan-600 dark:text-cyan-400">{formatLatency(log.first_token_ms)}</span>
                          ) : (
                            <span className="text-gray-300 dark:text-gray-600">-</span>
                          )}
                        </td>
                      )}
                      {visibleColumns.speed && (
                        <td className="px-4 py-2.5 text-right text-xs font-mono">
                          {log.first_token_ms && log.completion_tokens && log.latency_ms > log.first_token_ms ? (
                            <span className="text-emerald-600 dark:text-emerald-400">
                              {((log.completion_tokens / (log.latency_ms - log.first_token_ms)) * 1000).toFixed(1)} t/s
                            </span>
                          ) : (
                            <span className="text-gray-300 dark:text-gray-600">-</span>
                          )}
                        </td>
                      )}
                      {visibleColumns.time && (
                        <td className="px-4 py-2.5 text-right text-xs text-gray-500 dark:text-gray-400">
                          {formatTime(log.timestamp)}
                        </td>
                      )}
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* 详情弹窗 */}
      {selectedLog && (
        <div
          className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-start justify-center z-50 p-4 pt-[5vh]"
          onClick={() => setSelectedLog(null)}
        >
          <div
            className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] overflow-hidden flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            {/* 弹窗头部 */}
            <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
              <div className="flex items-center gap-3">
                <span
                  className="inline-flex items-center justify-center px-2.5 py-1 rounded text-xs font-bold"
                  style={getStatusStyle(selectedLog.status_code)}
                >
                  {selectedLog.status_code}
                </span>
                <span className="font-bold text-lg text-gray-900 dark:text-white">{selectedLog.method}</span>
                <span className="text-gray-500 dark:text-gray-400 font-mono text-sm">{selectedLog.path}</span>
              </div>
              <button
                onClick={() => setSelectedLog(null)}
                className="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
              >
                <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            <div className="p-6 space-y-4 overflow-y-auto">
              {/* 元信息 */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 p-4 bg-gray-50 dark:bg-gray-900/50 rounded-xl border border-gray-200 dark:border-gray-700">
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">时间</div>
                  <div className="text-sm font-medium text-gray-900 dark:text-white">
                    {formatDateTime(selectedLog.timestamp)}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">延迟</div>
                  <div className="text-sm font-medium text-gray-900 dark:text-white">
                    {formatLatency(selectedLog.latency_ms)}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">首Token</div>
                  <div className="text-sm font-medium text-cyan-600 dark:text-cyan-400">
                    {selectedLog.first_token_ms ? formatLatency(selectedLog.first_token_ms) : '-'}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">生成速度</div>
                  <div className="text-sm font-medium text-emerald-600 dark:text-emerald-400">
                    {selectedLog.first_token_ms && selectedLog.completion_tokens && selectedLog.latency_ms > selectedLog.first_token_ms
                      ? `${((selectedLog.completion_tokens / (selectedLog.latency_ms - selectedLog.first_token_ms)) * 1000).toFixed(1)} t/s`
                      : '-'}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">请求名称</div>
                  <div className={`text-sm font-medium font-mono ${
                    selectedLog.requested_model && selectedLog.model && selectedLog.requested_model !== selectedLog.model
                      ? 'text-amber-600 dark:text-amber-400'
                      : 'text-gray-500 dark:text-gray-400'
                  }`}>
                    {selectedLog.requested_model || '-'}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">实际模型</div>
                  <div className="text-sm font-medium text-indigo-600 dark:text-indigo-400 font-mono">
                    {selectedLog.model || '-'}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">Provider</div>
                  <div className="text-sm">
                    <span
                      className="inline-block px-2 py-0.5 rounded text-[10px] font-bold"
                      style={PROVIDER_COLORS[selectedLog.provider || ''] || { backgroundColor: '#6b7280', color: '#fff' }}
                    >
                      {selectedLog.provider || '-'}
                    </span>
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">前缀</div>
                  <div className="text-sm">
                    <span className="px-2 py-0.5 bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 rounded font-mono">
                      {selectedLog.provider_prefix || '-'}
                    </span>
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">协议转换</div>
                  <div className="text-sm">
                    {selectedLog.protocol_transform ? (
                      <span className={`inline-block px-2 py-0.5 rounded text-xs font-bold ${
                        selectedLog.protocol_transform === 'direct'
                          ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
                          : 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400'
                      }`}>
                        {selectedLog.protocol_transform}
                      </span>
                    ) : (
                      <span className="text-gray-400">-</span>
                    )}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">Tokens</div>
                  <div className="flex items-center gap-2">
                    <span className="px-2 py-0.5 rounded bg-indigo-100 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-400 text-xs font-bold">
                      I: {selectedLog.prompt_tokens || 0}
                    </span>
                    <span className="px-2 py-0.5 rounded bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 text-xs font-bold">
                      O: {selectedLog.completion_tokens || 0}
                    </span>
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-gray-400 uppercase tracking-wider mb-1">成本</div>
                  <div className="text-sm font-medium text-amber-600 dark:text-amber-400">
                    {selectedLog.cost != null && selectedLog.cost > 0 ? `$${selectedLog.cost.toFixed(6)}` : '-'}
                  </div>
                </div>
              </div>

              {/* 实际请求 URL */}
              {selectedLog.url && (
                <div className="p-3 rounded-lg bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800">
                  <div className="text-[10px] text-blue-600 dark:text-blue-400 uppercase tracking-wider mb-1 font-bold">
                    实际请求 URL
                  </div>
                  <div className="text-xs text-blue-700 dark:text-blue-300 font-mono break-all">
                    {selectedLog.url}
                  </div>
                </div>
              )}

              {/* 错误信息 */}
              {selectedLog.error && (
                <div className="p-4 rounded-xl bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800">
                  <div className="text-[10px] text-red-600 dark:text-red-400 uppercase tracking-wider mb-1 font-bold">
                    错误
                  </div>
                  <div className="text-sm text-red-700 dark:text-red-300 font-mono">{selectedLog.error}</div>
                </div>
              )}

              {/* 原始请求内容（仅在协议转换时显示） */}
              {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_request_body && (
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h3 className="text-[11px] text-blue-600 dark:text-blue-400 uppercase tracking-wider font-bold">
                      原始请求（转换前）
                    </h3>
                    <button
                      onClick={() => {
                        const json = formatJson(selectedLog.original_request_body);
                        if (json) navigator.clipboard.writeText(json);
                      }}
                      className="flex items-center gap-1 px-2 py-1 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                    >
                      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
                        />
                      </svg>
                      复制
                    </button>
                  </div>
                  <pre className="bg-blue-950 text-blue-100 p-4 rounded-lg text-xs overflow-x-auto max-h-60 overflow-y-auto leading-relaxed border border-blue-800">
                    {formatJson(selectedLog.original_request_body) || '-'}
                  </pre>
                </div>
              )}

              {/* 请求内容 */}
              <div>
                <div className="flex items-center justify-between mb-2">
                  <h3 className="text-[11px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_request_body
                      ? '转换后请求'
                      : '请求内容'}
                    {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_request_body && (
                      <span className="ml-2 px-1.5 py-0.5 rounded text-[9px] bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400">
                        已转换
                      </span>
                    )}
                  </h3>
                  <button
                    onClick={() => {
                      const json = formatJson(selectedLog.request_body);
                      if (json) navigator.clipboard.writeText(json);
                    }}
                    className="flex items-center gap-1 px-2 py-1 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                  >
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
                      />
                    </svg>
                    复制
                  </button>
                </div>
                <pre className="bg-gray-900 text-gray-100 p-4 rounded-lg text-xs overflow-x-auto max-h-60 overflow-y-auto leading-relaxed">
                  {formatJson(selectedLog.request_body) || '-'}
                </pre>
              </div>

              {/* 响应内容 */}
              <div>
                <div className="flex items-center justify-between mb-2">
                  <h3 className="text-[11px] text-gray-500 dark:text-gray-400 uppercase tracking-wider font-bold">
                    {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_response_body
                      ? '转换后响应'
                      : '响应内容'}
                    {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_response_body && (
                      <span className="ml-2 px-1.5 py-0.5 rounded text-[9px] bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400">
                        已转换
                      </span>
                    )}
                  </h3>
                  <button
                    onClick={() => {
                      const json = formatJson(selectedLog.response_body);
                      if (json) navigator.clipboard.writeText(json);
                    }}
                    className="flex items-center gap-1 px-2 py-1 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                  >
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        strokeWidth={2}
                        d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
                      />
                    </svg>
                    复制
                  </button>
                </div>
                <pre className="bg-gray-900 text-gray-100 p-4 rounded-lg text-xs overflow-x-auto max-h-60 overflow-y-auto leading-relaxed">
                  {formatJson(selectedLog.response_body) || '-'}
                </pre>
              </div>

              {/* 原始响应内容（仅在协议转换时显示） */}
              {selectedLog.protocol_transform && selectedLog.protocol_transform !== 'direct' && selectedLog.original_response_body && (
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h3 className="text-[11px] text-amber-600 dark:text-amber-400 uppercase tracking-wider font-bold">
                      原始响应（转换前）
                    </h3>
                    <button
                      onClick={() => {
                        const json = formatJson(selectedLog.original_response_body);
                        if (json) navigator.clipboard.writeText(json);
                      }}
                      className="flex items-center gap-1 px-2 py-1 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-200 transition-colors"
                    >
                      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path
                          strokeLinecap="round"
                          strokeLinejoin="round"
                          strokeWidth={2}
                          d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z"
                        />
                      </svg>
                      复制
                    </button>
                  </div>
                  <pre className="bg-amber-950 text-amber-100 p-4 rounded-lg text-xs overflow-x-auto max-h-60 overflow-y-auto leading-relaxed border border-amber-800">
                    {formatJson(selectedLog.original_response_body) || '-'}
                  </pre>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Logs;
