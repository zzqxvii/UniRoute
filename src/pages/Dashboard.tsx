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

interface HourlyTraffic {
  hour: string;
  request_count: number;
  total_tokens: number;
  total_cost: number;
}

interface ProviderHealth {
  provider: string;
  provider_prefix: string;
  request_count: number;
  success_count: number;
  failed_count: number;
  success_rate: number;
  avg_latency_ms: number;
  total_cost: number;
}

// 简单的 SVG 柱状图组件
function TrafficChart({ data, maxValue, color }: { data: number[]; maxValue: number; color: string }) {
  if (data.length === 0) return null;
  
  const height = 80;
  const width = data.length * 12;
  const barWidth = 10;
  
  return (
    <svg width="100%" height={height} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none">
      {data.map((value, i) => {
        const barHeight = maxValue > 0 ? (value / maxValue) * height : 0;
        return (
          <rect
            key={i}
            x={i * 12}
            y={height - barHeight}
            width={barWidth}
            height={barHeight}
            fill={color}
            rx={1}
          />
        );
      })}
    </svg>
  );
}

function Dashboard() {
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus>({ is_running: false, port: null });
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [providerCount, setProviderCount] = useState(0);
  const [groupCount, setGroupCount] = useState(0);
  const [quotaStatus, setQuotaStatus] = useState<QuotaStatus | null>(null);
  const [requestStats, setRequestStats] = useState<RequestStats | null>(null);
  const [hourlyTraffic, setHourlyTraffic] = useState<HourlyTraffic[]>([]);
  const [providerHealth, setProviderHealth] = useState<ProviderHealth[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 10000);
    return () => clearInterval(interval);
  }, []);

  const loadData = async () => {
    try {
      setError(null);
      const [status, settingsResult, providers, groups, quota, stats, traffic, health] = await Promise.all([
        invoke<ProxyStatus>('get_proxy_status'),
        invoke<AppSettings>('get_settings'),
        invoke<{ id: string }[]>('get_providers'),
        invoke<{ id: string }[]>('get_groups'),
        invoke<QuotaStatus>('get_quota_status'),
        invoke<RequestStats>('get_request_stats'),
        invoke<HourlyTraffic[]>('get_hourly_traffic', { hours: 24 }),
        invoke<ProviderHealth[]>('get_provider_health', { hours: 24 }),
      ]);
      setProxyStatus(status);
      setSettings(settingsResult);
      setProviderCount(providers.length);
      setGroupCount(groups.length);
      setQuotaStatus(quota);
      setRequestStats(stats);
      setHourlyTraffic(traffic);
      setProviderHealth(health);
    } catch (error) {
      console.error('Failed to load data:', error);
      setError('加载数据失败: ' + error);
    }
  };

  const baseUrl = `http://127.0.0.1:${proxyStatus.port || settings?.proxy_port || 8080}`;

  // 计算图表数据
  const trafficData = hourlyTraffic.map(t => t.request_count);
  const maxTraffic = Math.max(...trafficData, 1);
  const tokenData = hourlyTraffic.map(t => Math.round(t.total_tokens / 1000));
  const maxTokens = Math.max(...tokenData, 1);

  const formatCost = (cost: number) => {
    if (cost < 0.01) return `¥${(cost * 1000).toFixed(1)}¢`;
    return `¥${cost.toFixed(4)}`;
  };

  const getSuccessRateColor = (rate: number) => {
    if (rate >= 95) return 'text-green-600 dark:text-green-400';
    if (rate >= 80) return 'text-yellow-600 dark:text-yellow-400';
    return 'text-red-600 dark:text-red-400';
  };

  const getLatencyColor = (ms: number) => {
    if (ms <= 500) return 'text-green-600 dark:text-green-400';
    if (ms <= 2000) return 'text-yellow-600 dark:text-yellow-400';
    return 'text-red-600 dark:text-red-400';
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">仪表盘</h1>
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
          <svg className="w-6 h-6 text-red-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
          </svg>
          <div>
            <p className="text-red-700 dark:text-red-300 font-medium">配额已超出</p>
            <p className="text-red-600 dark:text-red-400 text-sm">
              今日: ¥{quotaStatus.daily_used.toFixed(4)} | 本月: ¥{quotaStatus.monthly_used.toFixed(4)}
            </p>
          </div>
        </div>
      )}
      {quotaStatus?.is_warning && !quotaStatus.is_exceeded && (
        <div className="mb-4 bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4 flex items-center gap-3">
          <svg className="w-6 h-6 text-yellow-500" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 10V3L4 14h7v7l9-11h-7z" />
          </svg>
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
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">组合数</p>
          <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{groupCount}</p>
        </div>
        <div className="card-base p-5">
          <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">总请求 (24h)</p>
          <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">
            {hourlyTraffic.reduce((sum, t) => sum + t.request_count, 0)}
          </p>
        </div>
      </div>

      {/* Stats Cards */}
      {requestStats && (
        <div className="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-4">
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">总请求 (累计)</p>
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
            <p className="mt-2 text-xl font-semibold text-amber-600 dark:text-amber-400">{formatCost(requestStats.total_cost)}</p>
          </div>
          <div className="card-base p-5">
            <p className="text-sm text-gray-500 dark:text-gray-400 font-medium">平均延迟</p>
            <p className="mt-2 text-xl font-semibold text-gray-900 dark:text-white">{requestStats.avg_latency_ms.toFixed(0)}ms</p>
          </div>
        </div>
      )}

      {/* Traffic Charts */}
      <div className="mt-6 grid grid-cols-1 gap-4 lg:grid-cols-2">
        {/* Request Traffic */}
        <div className="card-base p-5">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white mb-4">请求流量 (24小时)</h3>
          {hourlyTraffic.length > 0 ? (
            <div className="space-y-2">
              <TrafficChart data={trafficData} maxValue={maxTraffic} color="#6366f1" />
              <div className="flex justify-between text-xs text-gray-400">
                <span>24h前</span>
                <span>现在</span>
              </div>
            </div>
          ) : (
            <p className="text-sm text-gray-400">暂无数据</p>
          )}
        </div>

        {/* Token Usage */}
        <div className="card-base p-5">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white mb-4">Token 使用 (24小时, K)</h3>
          {hourlyTraffic.length > 0 ? (
            <div className="space-y-2">
              <TrafficChart data={tokenData} maxValue={maxTokens} color="#10b981" />
              <div className="flex justify-between text-xs text-gray-400">
                <span>24h前</span>
                <span>现在</span>
              </div>
            </div>
          ) : (
            <p className="text-sm text-gray-400">暂无数据</p>
          )}
        </div>
      </div>

      {/* Provider Health */}
      <div className="mt-6">
        <h3 className="text-lg font-semibold text-gray-900 dark:text-white mb-4">供应商健康状态 (24小时)</h3>
        {providerHealth.length > 0 ? (
          <div className="card-base overflow-hidden">
            <table className="min-w-full divide-y divide-gray-200 dark:divide-gray-700">
              <thead className="bg-gray-50 dark:bg-gray-900/50">
                <tr>
                  <th className="px-4 py-3 text-left text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase">供应商</th>
                  <th className="px-4 py-3 text-right text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase">请求数</th>
                  <th className="px-4 py-3 text-right text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase">成功率</th>
                  <th className="px-4 py-3 text-right text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase">平均延迟</th>
                  <th className="px-4 py-3 text-right text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase">成本</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-gray-200 dark:divide-gray-700">
                {providerHealth.map((provider) => (
                  <tr key={provider.provider_prefix} className="hover:bg-gray-50 dark:hover:bg-gray-700/30">
                    <td className="px-4 py-3">
                      <div className="flex items-center gap-2">
                        <span className={`w-2 h-2 rounded-full ${provider.success_rate >= 95 ? 'bg-green-500' : provider.success_rate >= 80 ? 'bg-yellow-500' : 'bg-red-500'}`}></span>
                        <span className="text-sm font-medium text-gray-900 dark:text-white">{provider.provider}</span>
                      </div>
                    </td>
                    <td className="px-4 py-3 text-right text-sm text-gray-600 dark:text-gray-400">{provider.request_count}</td>
                    <td className="px-4 py-3 text-right">
                      <span className={`text-sm font-semibold ${getSuccessRateColor(provider.success_rate)}`}>
                        {provider.success_rate.toFixed(1)}%
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right">
                      <span className={`text-sm ${getLatencyColor(provider.avg_latency_ms)}`}>
                        {provider.avg_latency_ms.toFixed(0)}ms
                      </span>
                    </td>
                    <td className="px-4 py-3 text-right text-sm text-amber-600 dark:text-amber-400">{formatCost(provider.total_cost)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <div className="card-base p-8 text-center">
            <p className="text-gray-400">暂无供应商使用数据</p>
          </div>
        )}
      </div>

      {/* Quota Progress */}
      {quotaStatus && (quotaStatus.daily_percent !== null || quotaStatus.monthly_percent !== null) && (
        <div className="mt-6 card-base p-5">
          <h3 className="text-sm font-semibold text-gray-900 dark:text-white mb-4">配额使用</h3>
          <div className="space-y-4">
            {quotaStatus.daily_percent !== null && (
              <div>
                <div className="flex justify-between text-sm mb-2">
                  <span className="text-gray-500 dark:text-gray-400">今日</span>
                  <span className={quotaStatus.is_exceeded ? 'text-red-600' : quotaStatus.is_warning ? 'text-yellow-600' : 'text-gray-600 dark:text-gray-400'}>
                    ¥{quotaStatus.daily_used.toFixed(4)} / {quotaStatus.daily_remaining !== null ? `¥${(quotaStatus.daily_used + quotaStatus.daily_remaining).toFixed(2)}` : '无限制'}
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
                <div className="flex justify-between text-sm mb-2">
                  <span className="text-gray-500 dark:text-gray-400">本月</span>
                  <span className={quotaStatus.is_exceeded ? 'text-red-600' : quotaStatus.is_warning ? 'text-yellow-600' : 'text-gray-600 dark:text-gray-400'}>
                    ¥{quotaStatus.monthly_used.toFixed(4)} / {quotaStatus.monthly_remaining !== null ? `¥${(quotaStatus.monthly_used + quotaStatus.monthly_remaining).toFixed(2)}` : '无限制'}
                  </span>
                </div>
                <div className="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
                  <div
                    className={`h-2 rounded-full transition-all duration-300 ${
                      quotaStatus.is_exceeded ? 'bg-red-500' : quotaStatus.is_warning ? 'bg-yellow-500' : 'bg-blue-500'
                    }`}
                    style={{ width: `${Math.min(quotaStatus.monthly_percent, 100)}%` }}
                  ></div>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* API Base URL */}
      <div className="mt-6 card-base p-5">
        <h3 className="text-sm font-semibold text-gray-900 dark:text-white mb-3">API 地址</h3>
        <div className="flex items-center gap-2 p-3 bg-gray-50 dark:bg-gray-900 rounded-lg">
          <code className="text-sm font-mono text-gray-700 dark:text-gray-300">{baseUrl}</code>
          {proxyStatus.is_running && (
            <button
              onClick={() => navigator.clipboard.writeText(baseUrl)}
              className="ml-auto text-xs text-indigo-600 hover:text-indigo-700 dark:text-indigo-400"
            >
              复制
            </button>
          )}
        </div>
        {proxyStatus.is_running && (
          <div className="mt-3 p-3 bg-gray-900 dark:bg-gray-950 rounded-lg">
            <code className="block text-green-400 text-sm font-mono">
              curl {baseUrl}/v1/models
            </code>
          </div>
        )}
      </div>

      {/* Quick Links */}
      <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2">
        <a href="/providers" className="card-base p-5 group hover:border-indigo-400 transition-colors">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-sm font-semibold text-indigo-600 dark:text-indigo-400 group-hover:text-indigo-700 dark:group-hover:text-indigo-300">
                添加供应商
              </h3>
              <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">配置 API 地址和密钥</p>
            </div>
            <span className="text-indigo-400 group-hover:translate-x-1 transition-transform">→</span>
          </div>
        </a>
        <a href="/groups" className="card-base p-5 group hover:border-indigo-400 transition-colors">
          <div className="flex items-center justify-between">
            <div>
              <h3 className="text-sm font-semibold text-indigo-600 dark:text-indigo-400 group-hover:text-indigo-700 dark:group-hover:text-indigo-300">
                创建组合
              </h3>
              <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">配置路由规则</p>
            </div>
            <span className="text-indigo-400 group-hover:translate-x-1 transition-transform">→</span>
          </div>
        </a>
      </div>
    </div>
  );
}

export default Dashboard;
