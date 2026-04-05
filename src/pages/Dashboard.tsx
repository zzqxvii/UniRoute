import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ProxyStatus {
  is_running: boolean;
  port: number | null;
}

interface AppSettings {
  proxy_port: number;
  auto_start_proxy: boolean;
  log_level: string;
}

interface QuotaStatus {
  daily_used: number;
  monthly_used: number;
  daily_remaining: number | null;
  monthly_remaining: number | null;
  daily_percent: number | null;
  monthly_percent: number | null;
  is_exceeded: boolean;
  is_warning: boolean;
}

interface RequestStats {
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  total_tokens: number;
  total_cost: number;
  avg_latency_ms: number;
}

function Dashboard() {
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus>({ is_running: false, port: null });
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [providerCount, setProviderCount] = useState(0);
  const [groupCount, setGroupCount] = useState(0);
  const [quotaStatus, setQuotaStatus] = useState<QuotaStatus | null>(null);
  const [requestStats, setRequestStats] = useState<RequestStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 3000);
    return () => clearInterval(interval);
  }, []);

  const loadData = async () => {
    try {
      setError(null);
      const [status, settingsResult, providers, groups, quota, stats] = await Promise.all([
        invoke<ProxyStatus>('get_proxy_status'),
        invoke<AppSettings>('get_settings'),
        invoke<{ id: string }[]>('get_providers'),
        invoke<{ id: string }[]>('get_groups'),
        invoke<QuotaStatus>('get_quota_status'),
        invoke<RequestStats>('get_request_stats'),
      ]);
      setProxyStatus(status);
      setSettings(settingsResult);
      setProviderCount(providers.length);
      setGroupCount(groups.length);
      setQuotaStatus(quota);
      setRequestStats(stats);
    } catch (error) {
      console.error('Failed to load data:', error);
      setError('加载数据失败: ' + error);
    }
  };

  const baseUrl = `http://127.0.0.1:${proxyStatus.port || settings?.proxy_port || 8080}`;

  const apiEndpoints = [
    { method: 'POST', path: '/v1/chat/completions', desc: 'OpenAI 兼容聊天' },
    { method: 'POST', path: '/v1/messages', desc: 'Claude 消息' },
    { method: 'POST', path: '/v1/embeddings', desc: '嵌入向量' },
    { method: 'GET', path: '/v1/models', desc: '模型列表' },
  ];

  const formatCost = (cost: number) => {
    if (cost < 0.01) return `$${(cost * 1000).toFixed(1)}¢`;
    return `$${cost.toFixed(4)}`;
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">
          仪表盘
        </h1>
        <button
          onClick={loadData}
          className="text-sm text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 transition-colors"
        >
          刷新
        </button>
      </div>

      {error && (
        <div className="mb-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4">
          <p className="text-red-700 dark:text-red-300">{error}</p>
        </div>
      )}

      {/* Quota Warning Alert */}
      {quotaStatus?.is_exceeded && (
        <div className="mb-4 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4 flex items-center gap-3">
          <span className="text-red-600 dark:text-red-400 text-xl">⚠</span>
          <div>
            <p className="text-red-700 dark:text-red-300 font-medium">配额已超出</p>
            <p className="text-red-600 dark:text-red-400 text-sm">
              今日: ${quotaStatus.daily_used.toFixed(4)} | 本月: ${quotaStatus.monthly_used.toFixed(4)}
            </p>
          </div>
        </div>
      )}
      {quotaStatus?.is_warning && !quotaStatus.is_exceeded && (
        <div className="mb-4 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4 flex items-center gap-3">
          <span className="text-yellow-600 dark:text-yellow-400 text-xl">⚡</span>
          <div>
            <p className="text-yellow-700 dark:text-yellow-300 font-medium">配额即将耗尽</p>
            <p className="text-yellow-600 dark:text-yellow-400 text-sm">
              {quotaStatus.daily_percent ? `今日已用 ${quotaStatus.daily_percent.toFixed(0)}%` : ''}
              {' | '}
              {quotaStatus.monthly_percent ? `本月已用 ${quotaStatus.monthly_percent.toFixed(0)}%` : ''}
            </p>
          </div>
        </div>
      )}

      {/* Status Cards */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-4">
        <div className="card-base p-5">
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">代理状态</p>
          <div className="mt-2 flex items-center gap-2">
            <span className={`w-2.5 h-2.5 rounded-full ${proxyStatus.is_running ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}`}></span>
            <p className={`text-xl font-semibold ${proxyStatus.is_running ? 'text-green-600 dark:text-green-400' : 'text-gray-600 dark:text-gray-400'}`}>
              {proxyStatus.is_running ? `运行中 :${proxyStatus.port}` : '已停止'}
            </p>
          </div>
        </div>
        <div className="card-base p-5">
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">供应商数</p>
          <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{providerCount}</p>
        </div>
        <div className="card-base p-5">
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">Group 数</p>
          <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{groupCount}</p>
        </div>
        <div className="card-base p-5">
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">端口</p>
          <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white font-mono">{settings?.proxy_port || 8080}</p>
        </div>
      </div>

      {/* Quota & Stats Cards */}
      {requestStats && (
        <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-4">
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">总请求</p>
            <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{requestStats.total_requests}</p>
            <p className="text-xs text-gray-400 dark:text-gray-500 mt-1">
              成功 {requestStats.successful_requests} / 失败 {requestStats.failed_requests}
            </p>
          </div>
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">总 Token</p>
            <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">
              {requestStats.total_tokens > 1000 ? `${(requestStats.total_tokens / 1000).toFixed(1)}K` : requestStats.total_tokens}
            </p>
          </div>
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">总成本</p>
            <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{formatCost(requestStats.total_cost)}</p>
          </div>
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">平均延迟</p>
            <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{requestStats.avg_latency_ms.toFixed(0)}ms</p>
          </div>
        </div>
      )}

      {/* Quota Progress Bars */}
      {quotaStatus && (quotaStatus.daily_percent !== null || quotaStatus.monthly_percent !== null) && (
        <div className="mt-4 card-base p-5">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white mb-3">配额使用</h3>
          <div className="space-y-3">
            {quotaStatus.daily_percent !== null && (
              <div>
                <div className="flex justify-between text-xs mb-1">
                  <span className="text-gray-500 dark:text-gray-400">今日</span>
                  <span className={quotaStatus.is_exceeded ? 'text-red-600' : quotaStatus.is_warning ? 'text-yellow-600' : 'text-gray-600 dark:text-gray-400'}>
                    ${quotaStatus.daily_used.toFixed(4)} / {quotaStatus.daily_remaining !== null ? `$${(quotaStatus.daily_used + quotaStatus.daily_remaining).toFixed(2)}` : '无限制'}
                    {quotaStatus.daily_percent.toFixed(0)}%
                  </span>
                </div>
                <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                  <div
                    className={`h-2 rounded-full transition-all duration-300 ${
                      quotaStatus.is_exceeded ? 'bg-red-500' : quotaStatus.is_warning ? 'bg-yellow-500' : 'bg-green-500'
                    }`}
                    style={{ width: `${Math.min(quotaStatus.daily_percent, 100)}%` }}
                  ></div>
                </div>
              </div>
            )}
            {quotaStatus.monthly_percent !== null && (
              <div>
                <div className="flex justify-between text-xs mb-1">
                  <span className="text-gray-500 dark:text-gray-400">本月</span>
                  <span className={quotaStatus.is_exceeded ? 'text-red-600' : quotaStatus.is_warning ? 'text-yellow-600' : 'text-gray-600 dark:text-gray-400'}>
                    ${quotaStatus.monthly_used.toFixed(4)} / {quotaStatus.monthly_remaining !== null ? `$${(quotaStatus.monthly_used + quotaStatus.monthly_remaining).toFixed(2)}` : '无限制'}
                    {quotaStatus.monthly_percent.toFixed(0)}%
                  </span>
                </div>
                <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                  <div
                    className={`h-2 rounded-full transition-all duration-300 ${
                      quotaStatus.is_exceeded ? 'bg-red-500' : quotaStatus.is_warning ? 'bg-yellow-500' : 'bg-green-500'
                    }`}
                    style={{ width: `${Math.min(quotaStatus.monthly_percent, 100)}%` }}
                  ></div>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* API Endpoints */}
      <div className="mt-6">
        <h2 className="text-lg font-semibold text-gray-900 dark:text-white mb-4">API 端点</h2>
        <div className="card-base overflow-hidden">
          <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
            <thead className="bg-gray-50 dark:bg-gray-900/50">
              <tr>
                <th className="px-5 py-3 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">方法</th>
                <th className="px-5 py-3 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">端点</th>
                <th className="px-5 py-3 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wider">说明</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
              {apiEndpoints.map((ep, idx) => (
                <tr key={idx} className="hover:bg-gray-50 dark:hover:bg-gray-700/30 transition-colors">
                  <td className="px-5 py-3 whitespace-nowrap">
                    <span className={`px-2.5 py-1 text-xs font-semibold rounded-full ${
                      ep.method === 'GET'
                        ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400'
                        : 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400'
                    }`}>
                      {ep.method}
                    </span>
                  </td>
                  <td className="px-5 py-3 font-mono text-sm text-gray-900 dark:text-white">
                    {baseUrl}{ep.path}
                  </td>
                  <td className="px-5 py-3 text-sm text-gray-500 dark:text-gray-400">{ep.desc}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        {proxyStatus.is_running && (
          <div className="mt-4 p-4 bg-gray-900 dark:bg-gray-950 rounded-lg">
            <p className="text-sm text-gray-400 mb-2">测试命令：</p>
            <code className="block text-green-400 text-sm overflow-x-auto font-mono">
              curl {baseUrl}/v1/models
            </code>
          </div>
        )}
      </div>

      {/* Quick Links */}
      <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2">
        <a href="/providers" className="card-base p-5 group hover:border-indigo-400 transition-colors">
          <h3 className="text-sm font-semibold text-indigo-600 dark:text-indigo-400 group-hover:text-indigo-700 dark:group-hover:text-indigo-300">
            添加供应商 →
          </h3>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">配置 API 地址和密钥</p>
        </a>
        <a href="/groups" className="card-base p-5 group hover:border-indigo-400 transition-colors">
          <h3 className="text-sm font-semibold text-indigo-600 dark:text-indigo-400 group-hover:text-indigo-700 dark:group-hover:text-indigo-300">
            创建 Group →
          </h3>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">配置路由规则</p>
        </a>
      </div>
    </div>
  );
}

export default Dashboard;
