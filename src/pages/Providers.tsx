import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ModelPricing {
  input: number;
  output: number;
}

// 端点能力类型
type EndpointCapability = 'chat' | 'responses' | 'claude' | 'gemini' | 'embeddings' | 'images' | 'videos' | 'music' | 'audio' | 'tts' | 'moderation' | 'rerank';

interface ModelConfig {
  name: string;
  pricing?: ModelPricing;
  endpoints?: EndpointCapability[];
  rpm?: number;
  tpm?: number;
}

interface OAuthConfig {
  client_id: string;
  client_secret?: string;
  token_url?: string;
  refresh_url?: string;
  auth_url?: string;
  initiate_url?: string;
  poll_url_base?: string;
}

interface OAuthTokens {
  access_token: string;
  refresh_token?: string;
  expires_at?: string;
  email?: string;
}

interface Provider {
  id: string;
  name: string;
  prefix: string;
  base_url: string;
  api_key: string | null;
  api_format: string;
  models: ModelConfig[];
  enable_cost: boolean;
  currency: string;
  auth_type: 'api_key' | 'oauth';
  oauth?: OAuthConfig;
  oauth_tokens?: OAuthTokens;
  headers: Record<string, string>;
  auth_header: string;
  auth_prefix?: string;
  is_active: boolean;
  is_builtin: boolean;
}

interface ProviderTemplate {
  id: string;
  name: string;
  prefix: string;
  default_base_url: string;
  api_format: string;
  endpoint_types?: string[];
  auth_type: 'api_key' | 'oauth';
  oauth?: OAuthConfig;
  headers: Record<string, string>;
  auth_header: string;
  auth_prefix?: string;
  models: ModelConfig[] | string[];
}

interface ProviderTestResult {
  success: boolean;
  message: string;
  balance: {
    available: string;
    currency: string;
    details: any;
  } | null;
  latency_ms: number | null;
}

// 端点测试结果
interface EndpointTestResult {
  provider_id: string;
  provider_name: string;
  model: string;
  endpoint: string;
  success: boolean;
  latency_ms: number;
  status_code?: number;
  error?: string;
  response_preview?: string;
}

interface OAuthFlowStatus {
  device_code: string;
  user_code: string;
  verification_uri: string;
  verification_uri_complete?: string;
  expires_in: number;
  interval: number;
}

interface OAuthProviderStatus {
  has_oauth: boolean;
  needs_auth: boolean;
  needs_refresh: boolean;
  has_token: boolean;
  expires_at?: string;
}

function Providers() {
  const [providers, setProviders] = useState<Provider[]>([]);
  const [templates, setTemplates] = useState<ProviderTemplate[]>([]);
  const [showModal, setShowModal] = useState(false);
  const [editingProvider, setEditingProvider] = useState<Provider | null>(null);
  const [showTemplates, setShowTemplates] = useState(false);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, ProviderTestResult>>({});
  const [oauthStatus, setOauthStatus] = useState<Record<string, OAuthProviderStatus>>({});

  // 端点测试状态
  const [testingEndpoint, setTestingEndpoint] = useState<{ providerId: string; model: string; endpoint: string } | null>(null);
  const [endpointTestResults, setEndpointTestResults] = useState<Record<string, EndpointTestResult>>({});

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [providersResult, templatesResult] = await Promise.all([
        invoke<Provider[]>('get_providers'),
        invoke<ProviderTemplate[]>('get_builtin_templates'),
      ]);
      setProviders(providersResult);
      setTemplates(templatesResult);

      // 检查所有 OAuth 供应商的状态
      for (const p of providersResult) {
        if (p.auth_type === 'oauth') {
          try {
            const status = await invoke<OAuthProviderStatus>('check_oauth_status', { providerId: p.id });
            setOauthStatus(prev => ({ ...prev, [p.id]: status }));
          } catch (e) {
            console.error('Failed to check OAuth status:', e);
          }
        }
      }
    } catch (error) {
      console.error('Failed to load data:', error);
    }
  };

  const handleDelete = async (id: string) => {
    if (confirm('确定删除此供应商？')) {
      try {
        await invoke('delete_provider', { id });
        loadData();
      } catch (error) {
        alert('删除失败: ' + error);
      }
    }
  };

  const handleTest = async (id: string) => {
    setTestingId(id);
    try {
      const result = await invoke<ProviderTestResult>('test_provider', { id });
      setTestResults(prev => ({ ...prev, [id]: result }));
    } catch (error) {
      setTestResults(prev => ({
        ...prev,
        [id]: {
          success: false,
          message: String(error),
          balance: null,
          latency_ms: null,
        },
      }));
    } finally {
      setTestingId(null);
    }
  };

  // 测试模型端点
  const handleTestEndpoint = async (providerId: string, model: string, endpoint: EndpointCapability) => {
    const key = `${providerId}:${model}:${endpoint}`;
    setTestingEndpoint({ providerId, model, endpoint });
    try {
      const result = await invoke<EndpointTestResult>('test_model_endpoint', {
        providerId,
        model,
        endpoint,
      });
      setEndpointTestResults(prev => ({ ...prev, [key]: result }));
    } catch (error) {
      setEndpointTestResults(prev => ({
        ...prev,
        [key]: {
          provider_id: providerId,
          provider_name: '',
          model,
          endpoint,
          success: false,
          latency_ms: 0,
          error: String(error),
        },
      }));
    } finally {
      setTestingEndpoint(null);
    }
  };

  const handleCreateFromTemplate = async (template: ProviderTemplate) => {
    // 处理 models 字段：可能是 ModelConfig[] 或 string[]
    const models: ModelConfig[] = template.models.map(m =>
      typeof m === 'string' ? { name: m } : m
    );

    setEditingProvider({
      id: '',
      name: template.name,
      prefix: template.prefix,
      base_url: template.default_base_url,
      api_key: null,
      api_format: template.api_format,
      models: models,
      enable_cost: false,
      currency: 'CNY',
      auth_type: template.auth_type,
      oauth: template.oauth,
      headers: template.headers || {},
      auth_header: template.auth_header || 'Authorization',
      auth_prefix: template.auth_prefix,
      is_active: true,
      is_builtin: false,
    });
    setShowTemplates(false);
    setShowModal(true);
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">供应商管理</h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            配置 AI 服务的 API 地址和密钥
          </p>
        </div>
        <div className="flex space-x-3">
          <button
            onClick={() => setShowTemplates(true)}
            className="btn-secondary"
          >
            从模板添加
          </button>
          <button
            onClick={() => { setEditingProvider(null); setShowModal(true); }}
            className="btn-primary"
          >
            + 添加供应商
          </button>
        </div>
      </div>

      {providers.length === 0 ? (
        <div className="card-base p-8 text-center">
          <p className="text-gray-500 dark:text-gray-400">暂无供应商</p>
          <button
            onClick={() => setShowTemplates(true)}
            className="mt-4 text-indigo-600 hover:text-indigo-900 dark:text-indigo-400"
          >
            从模板添加第一个供应商
          </button>
        </div>
      ) : (
        <div className="space-y-4">
          {providers.map((provider) => (
            <div key={provider.id} className="card-base overflow-hidden">
              <div className="px-6 py-4 flex items-center justify-between">
                <div className="flex items-center space-x-4">
                  <div>
                    <div className="flex items-center gap-2">
                      <h3 className="text-sm font-medium text-gray-900 dark:text-white">
                        {provider.name}
                      </h3>
                      <span className="px-2 py-0.5 bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 text-xs rounded font-mono">
                        {provider.prefix}
                      </span>
                      {provider.auth_type === 'oauth' && (
                        <span className="px-2 py-0.5 bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 text-xs rounded">
                          OAuth
                        </span>
                      )}
                      {provider.is_builtin && (
                        <span className="px-2 py-0.5 bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400 text-xs rounded">
                          内置
                        </span>
                      )}
                    </div>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-0.5 font-mono">
                      {provider.base_url}
                    </p>
                  </div>
                  <span className={`px-2 py-0.5 text-xs rounded-full font-medium ${
                    provider.is_active
                      ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
                      : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400'
                  }`}>
                    {provider.is_active ? '启用' : '禁用'}
                  </span>
                </div>
                <div className="flex space-x-3">
                  <button
                    onClick={() => handleTest(provider.id)}
                    disabled={testingId === provider.id}
                    className="text-sm text-cyan-600 hover:text-cyan-800 dark:text-cyan-400 font-medium transition-colors disabled:opacity-50"
                  >
                    {testingId === provider.id ? '测试中...' : '测试'}
                  </button>
                  <button
                    onClick={() => { setEditingProvider(provider); setShowModal(true); }}
                    className="text-sm text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 font-medium transition-colors"
                  >
                    编辑
                  </button>
                  {!provider.is_builtin && (
                    <button
                      onClick={() => handleDelete(provider.id)}
                      className="text-sm text-red-600 hover:text-red-800 dark:text-red-400 font-medium transition-colors"
                    >
                      删除
                    </button>
                  )}
                </div>
              </div>

              <div className="px-6 py-3 bg-gray-50 dark:bg-gray-700/30 border-t border-gray-100 dark:border-gray-700">
                <div className="flex items-center justify-between mb-2">
                  <p className="text-xs text-gray-500 dark:text-gray-400 font-medium">支持的模型</p>
                  {provider.enable_cost && (
                    <span className="px-2 py-0.5 bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 text-[10px] rounded-full font-medium">
                      成本统计
                    </span>
                  )}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {provider.models.length === 0 ? (
                    <span className="text-xs text-gray-400 italic">未配置</span>
                  ) : provider.models.some(m => m.name === '*') ? (
                    <span className="px-2.5 py-1 bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 text-xs rounded-full font-medium">
                      所有模型 (*)
                    </span>
                  ) : (
                    provider.models.slice(0, 8).map((model) => {
                      const endpoints = model.endpoints || ['chat'];
                      return (
                        <div key={model.name} className="group relative">
                          <div className="flex items-center gap-1 px-2.5 py-1 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs rounded-full font-mono">
                            {model.name}
                            {/* 端点能力标签 */}
                            <span className="flex gap-0.5 ml-1">
                              {endpoints.slice(0, 3).map(ep => {
                                const info = ENDPOINT_LABELS[ep];
                                return (
                                  <span key={ep} className={`px-1 text-[9px] rounded ${info?.color || 'bg-gray-100 text-gray-500'}`}>
                                    {info?.shortLabel || ep}
                                  </span>
                                );
                              })}
                              {endpoints.length > 3 && <span className="text-gray-400 text-[9px]">+{endpoints.length - 3}</span>}
                            </span>
                            {/* RPM 限制 */}
                            {model.rpm && (
                              <span className="ml-1 px-1 bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400 text-[9px] rounded" title={`${model.rpm} RPM`}>
                                {model.rpm}
                              </span>
                            )}
                            {/* 价格 */}
                            {model.pricing && model.pricing.input > 0 && (
                              <span className="ml-1 text-amber-600 dark:text-amber-400">${model.pricing.input}/${model.pricing.output}</span>
                            )}
                          </div>
                          {/* 悬停时显示测试按钮 */}
                          <div className="absolute left-0 top-full mt-1 z-10 hidden group-hover:flex gap-1 bg-white dark:bg-gray-800 p-1.5 rounded-lg shadow-lg border border-gray-200 dark:border-gray-700">
                            {COMMON_ENDPOINTS.slice(0, 4).map(ep => {
                              const testKey = `${provider.id}:${model.name}:${ep}`;
                              const testResult = endpointTestResults[testKey];
                              const isTesting = testingEndpoint?.providerId === provider.id &&
                                testingEndpoint?.model === model.name &&
                                testingEndpoint?.endpoint === ep;
                              const epInfo = ENDPOINT_LABELS[ep];
                              return (
                                <button
                                  key={ep}
                                  onClick={(e) => { e.stopPropagation(); handleTestEndpoint(provider.id, model.name, ep); }}
                                  disabled={isTesting}
                                  className={`px-1.5 py-0.5 text-[9px] font-medium rounded transition-all ${
                                    testResult?.success
                                      ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 ring-1 ring-green-400'
                                      : testResult && !testResult.success
                                      ? 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400 ring-1 ring-red-400'
                                      : 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300 hover:bg-gray-200 dark:hover:bg-gray-600'
                                  }`}
                                  title={`${epInfo.label}${testResult ? ` - ${testResult.latency_ms}ms` : ''}`}
                                >
                                  {isTesting ? '...' : epInfo.shortLabel}
                                </button>
                              );
                            })}
                          </div>
                        </div>
                      );
                    })
                  )}
                  {provider.models.length > 8 && (
                    <span className="px-2.5 py-1 bg-gray-100 dark:bg-gray-600 text-gray-500 dark:text-gray-400 text-xs rounded-full">
                      +{provider.models.length - 8} 更多
                    </span>
                  )}
                </div>

                {/* OAuth 状态 */}
                {provider.auth_type === 'oauth' && oauthStatus[provider.id] && (
                  <div className="mt-3 p-2 bg-emerald-50 dark:bg-emerald-900/20 border border-emerald-200 dark:border-emerald-800 rounded-lg">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-emerald-700 dark:text-emerald-300 font-medium">
                          {oauthStatus[provider.id].has_token ? '已授权' : '需要授权'}
                        </span>
                        {oauthStatus[provider.id].expires_at && (
                          <span className="text-xs text-gray-500 dark:text-gray-400">
                            过期: {new Date(oauthStatus[provider.id].expires_at!).toLocaleString()}
                          </span>
                        )}
                      </div>
                      <button
                        onClick={() => { setEditingProvider(provider); setShowModal(true); }}
                        className="text-xs text-emerald-600 hover:text-emerald-800 dark:text-emerald-400 font-medium"
                      >
                        {oauthStatus[provider.id].has_token ? '重新授权' : '立即授权'}
                      </button>
                    </div>
                  </div>
                )}

                {/* 测试结果 */}
                {testResults[provider.id] && (
                  <div className={`mt-3 p-3 rounded-lg ${
                    testResults[provider.id].success
                      ? 'bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800'
                      : 'bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800'
                  }`}>
                    <div className="flex items-center gap-2 mb-1">
                      <span className={`text-xs font-semibold ${
                        testResults[provider.id].success
                          ? 'text-green-700 dark:text-green-300'
                          : 'text-red-700 dark:text-red-300'
                      }`}>
                        {testResults[provider.id].success ? '✓ 连接成功' : '✗ 连接失败'}
                      </span>
                      {testResults[provider.id].latency_ms && (
                        <span className="text-xs text-gray-500 dark:text-gray-400">
                          {testResults[provider.id].latency_ms}ms
                        </span>
                      )}
                    </div>

                    {/* 余额信息 */}
                    {testResults[provider.id].balance && (
                      <div className="flex items-center gap-3 mt-2">
                        <div className="flex items-center gap-1">
                          <span className="text-xs text-gray-500 dark:text-gray-400">余额:</span>
                          <span className="text-sm font-semibold text-emerald-600 dark:text-emerald-400">
                            {testResults[provider.id].balance!.available} {testResults[provider.id].balance!.currency}
                          </span>
                        </div>
                      </div>
                    )}

                    {!testResults[provider.id].success && testResults[provider.id].message && (
                      <p className="text-xs text-red-600 dark:text-red-400 mt-1">
                        {testResults[provider.id].message}
                      </p>
                    )}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      )}

      {/* 添加/编辑模态框 */}
      {showModal && (
        <ProviderModal
          provider={editingProvider}
          existingPrefixes={providers.map(p => p.prefix)}
          onClose={() => { setShowModal(false); loadData(); }}
        />
      )}

      {/* 模板选择模态框 */}
      {showTemplates && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && setShowTemplates(false)}>
          <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-4xl max-h-[80vh] overflow-hidden">
            <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">选择供应商模板</h3>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">从预定义模板快速创建供应商</p>
            </div>
            <div className="p-4 grid grid-cols-2 gap-3 overflow-y-auto max-h-[60vh]">
              {templates.map((t) => (
                <button
                  key={t.id}
                  onClick={() => handleCreateFromTemplate(t)}
                  className="text-left p-4 border border-gray-200 dark:border-gray-700 rounded-lg hover:border-indigo-400 hover:bg-indigo-50 dark:hover:bg-indigo-900/20 transition-colors"
                >
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-gray-900 dark:text-white">{t.name}</span>
                    <span className="px-2 py-0.5 bg-purple-100 dark:bg-purple-900/30 text-purple-700 dark:text-purple-300 text-xs rounded font-mono">
                      {t.prefix}
                    </span>
                    {t.auth_type === 'oauth' && (
                      <span className="px-2 py-0.5 bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 text-xs rounded">
                        OAuth
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-gray-500 dark:text-gray-400 mt-1 font-mono truncate">
                    {t.default_base_url}
                  </p>
                </button>
              ))}
            </div>
            <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 flex justify-end">
              <button onClick={() => setShowTemplates(false)} className="btn-secondary">
                取消
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// 端点标签映射
const ENDPOINT_LABELS: Record<string, { label: string; shortLabel: string; color: string }> = {
  chat: { label: 'Chat', shortLabel: 'Chat', color: 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400' },
  responses: { label: 'Responses', shortLabel: 'Responses', color: 'bg-amber-100 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400' },
  claude: { label: 'Claude', shortLabel: 'Claude', color: 'bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400' },
  gemini: { label: 'Gemini', shortLabel: 'Gemini', color: 'bg-indigo-100 text-indigo-700 dark:bg-indigo-900/30 dark:text-indigo-400' },
  embeddings: { label: '嵌入', shortLabel: '嵌入', color: 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' },
  images: { label: '图像', shortLabel: '图像', color: 'bg-purple-100 text-purple-700 dark:bg-purple-900/30 dark:text-purple-400' },
  videos: { label: '视频', shortLabel: '视频', color: 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400' },
  music: { label: '音乐', shortLabel: '音乐', color: 'bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-400' },
  audio: { label: '语音(ASR)', shortLabel: 'ASR', color: 'bg-pink-100 text-pink-700 dark:bg-pink-900/30 dark:text-pink-400' },
  tts: { label: '语音合成', shortLabel: 'TTS', color: 'bg-cyan-100 text-cyan-700 dark:bg-cyan-900/30 dark:text-cyan-400' },
  moderation: { label: '审核', shortLabel: '审核', color: 'bg-slate-100 text-slate-700 dark:bg-slate-900/30 dark:text-slate-400' },
  rerank: { label: '重排', shortLabel: '重排', color: 'bg-teal-100 text-teal-700 dark:bg-teal-900/30 dark:text-teal-400' },
};

// 常用端点（用于 UI 显示）
const COMMON_ENDPOINTS: EndpointCapability[] = ['chat', 'responses', 'claude', 'gemini', 'embeddings', 'images', 'audio', 'tts'];

// 模型卡片组件
function ModelCard({
  model,
  enableCost,
  providerId,
  onToggleEndpoint,
  onUpdateRpm,
  onUpdatePricing,
  onRemove,
}: {
  model: ModelConfig;
  enableCost: boolean;
  providerId: string;
  onToggleEndpoint: (ep: EndpointCapability) => void;
  onUpdateRpm: (rpm: number | undefined) => void;
  onUpdatePricing: (field: 'input' | 'output', val: string) => void;
  onRemove: () => void;
}) {
  const [testingEndpoint, setTestingEndpoint] = useState<EndpointCapability | null>(null);
  const [testResults, setTestResults] = useState<Record<string, EndpointTestResult>>({});

  const handleTest = async (endpoint: EndpointCapability) => {
    if (!providerId) return;
    setTestingEndpoint(endpoint);
    try {
      const result = await invoke<EndpointTestResult>('test_model_endpoint', {
        providerId,
        model: model.name,
        endpoint,
      });
      setTestResults(prev => ({ ...prev, [endpoint]: result }));
    } catch (e) {
      setTestResults(prev => ({
        ...prev,
        [endpoint]: {
          provider_id: providerId,
          provider_name: '',
          model: model.name,
          endpoint,
          success: false,
          latency_ms: 0,
          error: String(e),
        },
      }));
    } finally {
      setTestingEndpoint(null);
    }
  };

  const endpoints = model.endpoints || ['chat'];

  return (
    <div className="bg-gray-50 dark:bg-gray-700/30 rounded-lg p-3 space-y-2">
      {/* 第一行：模型名 + 操作按钮 */}
      <div className="flex items-center justify-between">
        <span className="font-mono text-sm text-gray-900 dark:text-white font-medium">{model.name}</span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => handleTest('chat')}
            disabled={!providerId || testingEndpoint !== null}
            className="px-2 py-0.5 text-[10px] bg-cyan-100 dark:bg-cyan-900/30 text-cyan-700 dark:text-cyan-400 rounded hover:bg-cyan-200 dark:hover:bg-cyan-900/50 transition-colors disabled:opacity-50"
          >
            {testingEndpoint ? '测试中...' : '测试'}
          </button>
          <button
            type="button"
            onClick={onRemove}
            className="text-gray-400 hover:text-red-500 transition-colors"
            title="删除"
          >
            <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      </div>

      {/* 第二行：端点能力 */}
      <div className="flex flex-wrap items-center gap-1">
        <span className="text-[10px] text-gray-400 mr-1">端点:</span>
        {COMMON_ENDPOINTS.map(ep => {
          const supported = endpoints.includes(ep);
          const testResult = testResults[ep];
          const isTesting = testingEndpoint === ep;
          const epInfo = ENDPOINT_LABELS[ep];

          return (
            <button
              key={ep}
              type="button"
              onClick={() => onToggleEndpoint(ep)}
              onContextMenu={(e) => { e.preventDefault(); handleTest(ep); }}
              disabled={isTesting}
              className={`relative px-1.5 py-0.5 text-[9px] font-medium rounded transition-all ${
                supported
                  ? epInfo.color
                  : 'bg-gray-200 dark:bg-gray-600 text-gray-400 dark:text-gray-500 hover:bg-gray-300 dark:hover:bg-gray-500'
              }`}
              title={`${epInfo.label}${supported ? ' ✓' : ''}${testResult ? ` (${testResult.latency_ms}ms)` : ''} | 右键测试`}
            >
              {isTesting ? '...' : epInfo.shortLabel}
              {testResult?.success && (
                <span className="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 bg-green-500 rounded-full" />
              )}
              {testResult && !testResult.success && (
                <span className="absolute -top-0.5 -right-0.5 w-1.5 h-1.5 bg-red-500 rounded-full" />
              )}
            </button>
          );
        })}
      </div>

      {/* 第三行：RPM + 价格 */}
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-1">
          <span className="text-[10px] text-gray-400">RPM:</span>
          <input
            type="number"
            value={model.rpm || ''}
            onChange={(e) => onUpdateRpm(e.target.value ? parseInt(e.target.value) : undefined)}
            placeholder="-"
            className="w-14 px-1.5 py-0.5 text-xs text-center bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded"
          />
        </div>
        {enableCost && (
          <div className="flex items-center gap-1">
            <span className="text-[10px] text-gray-400">$/1M:</span>
            <input
              type="number"
              step="0.01"
              value={model.pricing?.input || ''}
              onChange={(e) => onUpdatePricing('input', e.target.value)}
              placeholder="in"
              className="w-12 px-1 py-0.5 text-xs text-center bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded"
            />
            <span className="text-gray-300 dark:text-gray-500">/</span>
            <input
              type="number"
              step="0.01"
              value={model.pricing?.output || ''}
              onChange={(e) => onUpdatePricing('output', e.target.value)}
              placeholder="out"
              className="w-12 px-1 py-0.5 text-xs text-center bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded"
            />
          </div>
        )}
      </div>
    </div>
  );
}

function ProviderModal({
  provider,
  existingPrefixes,
  onClose,
}: {
  provider: Provider | null;
  existingPrefixes: string[];
  onClose: () => void;
}) {
  const [name, setName] = useState(provider?.name || '');
  const [prefix, setPrefix] = useState(provider?.prefix || '');
  const [baseUrl, setBaseUrl] = useState(provider?.base_url || '');
  const [apiKey, setApiKey] = useState('');
  const [models, setModels] = useState<ModelConfig[]>(provider?.models || []);
  const [newModel, setNewModel] = useState('');
  const [newModelPricing, setNewModelPricing] = useState({ input: '', output: '' });
  const [enableCost, setEnableCost] = useState(provider?.enable_cost ?? false);
  const [isActive, setIsActive] = useState(provider?.is_active ?? true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // OAuth 相关状态
  const isOAuth = provider?.auth_type === 'oauth';
  const [oauthFlow, setOauthFlow] = useState<OAuthFlowStatus | null>(null);
  const [oauthPolling, setOauthPolling] = useState(false);
  const [oauthAuthorized, setOauthAuthorized] = useState(false);

  const isEdit = !!provider?.id;

  // 开始 OAuth 流程
  const startOAuthFlow = async () => {
    if (!provider?.id) {
      setError('请先保存供应商后再进行 OAuth 授权');
      return;
    }

    try {
      const flow = await invoke<OAuthFlowStatus>('start_oauth_flow', { providerId: provider.id });
      setOauthFlow(flow);

      // 开始轮询
      pollOAuthToken(flow);
    } catch (e) {
      setError('OAuth 流程启动失败: ' + e);
    }
  };

  // 轮询 OAuth Token
  const pollOAuthToken = async (flow: OAuthFlowStatus) => {
    setOauthPolling(true);

    const poll = async () => {
      try {
        await invoke<OAuthTokens>('poll_oauth_token', { providerId: provider!.id });
        setOauthAuthorized(true);
        setOauthFlow(null);
        setOauthPolling(false);
        return;
      } catch (e: any) {
        if (e === 'pending') {
          // 继续轮询
          setTimeout(poll, (flow.interval || 5) * 1000);
        } else {
          setError('OAuth 授权失败: ' + e);
          setOauthPolling(false);
        }
      }
    };

    setTimeout(poll, (flow.interval || 5) * 1000);
  };

  // 取消 OAuth 流程
  const cancelOAuthFlow = async () => {
    if (provider?.id) {
      try {
        await invoke('cancel_oauth_flow', { providerId: provider.id });
      } catch (e) {
        console.error('Failed to cancel OAuth flow:', e);
      }
    }
    setOauthFlow(null);
    setOauthPolling(false);
  };

  // 远程模型列表
  const [remoteModels, setRemoteModels] = useState<{ id: string; name: string; owned_by?: string }[]>([]);
  const [loadingModels, setLoadingModels] = useState(false);
  const [showModelPicker, setShowModelPicker] = useState(false);
  const [modelSearchQuery, setModelSearchQuery] = useState('');

  // 获取 Provider 的模型列表
  const fetchRemoteModels = async () => {
    if (!provider?.id) {
      setError('请先保存供应商后再获取模型列表');
      return;
    }
    setLoadingModels(true);
    setModelSearchQuery(''); // 清空搜索
    try {
      const models = await invoke<{ id: string; name: string; owned_by?: string }[]>('fetch_provider_models', {
        providerId: provider.id,
      });
      setRemoteModels(models);
      setShowModelPicker(true);
    } catch (e) {
      setError('获取模型列表失败: ' + e);
    } finally {
      setLoadingModels(false);
    }
  };

  // 从远程列表添加模型
  const addModelFromRemote = (modelId: string) => {
    if (!models.some(m => m.name === modelId)) {
      setModels([...models, {
        name: modelId,
        endpoints: ['chat'],
      }]);
    }
  };

  // 新模型的端点能力选择（用于快速添加）
  const [newModelEndpoints, setNewModelEndpoints] = useState<EndpointCapability[]>(['chat']);
  const [newModelRpm, setNewModelRpm] = useState('');

  const updateModelPricing = (modelName: string, field: 'input' | 'output', value: string) => {
    setModels(models.map(m => {
      if (m.name === modelName) {
        return {
          ...m,
          pricing: {
            ...(m.pricing || { input: 0, output: 0 }),
            [field]: parseFloat(value) || 0,
          }
        };
      }
      return m;
    }));
  };

  // 切换模型的端点能力
  const toggleModelEndpoint = (modelName: string, endpoint: EndpointCapability) => {
    setModels(models.map(m => {
      if (m.name === modelName) {
        const endpoints = m.endpoints || ['chat'];
        const hasIt = endpoints.includes(endpoint);
        const newEndpoints = hasIt
          ? endpoints.filter(e => e !== endpoint)
          : [...endpoints, endpoint];
        return { ...m, endpoints: newEndpoints.length > 0 ? newEndpoints : ['chat'] };
      }
      return m;
    }));
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setError('请输入名称');
      return;
    }
    if (!prefix.trim()) {
      setError('请输入前缀');
      return;
    }
    if (!baseUrl.trim()) {
      setError('请输入 API 地址');
      return;
    }
    if (!isOAuth && !isEdit && !apiKey.trim()) {
      setError('请输入 API Key');
      return;
    }

    // 检查前缀冲突
    if (!isEdit && existingPrefixes.includes(prefix.toLowerCase())) {
      setError('前缀已被使用');
      return;
    }

    setSaving(true);
    setError(null);

    try {
      if (isEdit) {
        await invoke('update_provider', {
          id: provider.id,
          provider: {
            ...provider,
            name: name.trim(),
            prefix: prefix.toLowerCase(),
            base_url: baseUrl.trim(),
            api_key: apiKey.trim() || provider.api_key,
            models: models,
            enable_cost: enableCost,
            is_active: isActive,
          },
        });
      } else {
        await invoke('create_provider', {
          name: name.trim(),
          prefix: prefix.toLowerCase(),
          baseUrl: baseUrl.trim(),
          apiKey: isOAuth ? null : apiKey.trim(),
          models: models.length > 0 ? models : null,
          authType: isOAuth ? 'oauth' : 'api_key',
          oauth: isOAuth ? provider?.oauth : null,
          headers: provider?.headers || null,
          authHeader: provider?.auth_header || null,
          authPrefix: provider?.auth_prefix || null,
        });
      }
      onClose();
    } catch (e) {
      setError('保存失败: ' + e);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-4xl overflow-hidden max-h-[90vh] overflow-y-auto">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            {isEdit ? '编辑供应商' : '添加供应商'}
          </h3>
          {isOAuth && (
            <span className="inline-block mt-1 px-2 py-0.5 bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300 text-xs rounded">
              OAuth 认证
            </span>
          )}
        </div>

        <div className="px-6 py-5 space-y-4">
          <div>
            <label className="label-base">名称 <span className="text-red-500">*</span></label>
            <input
              type="text"
              value={name}
              onChange={(e) => { setName(e.target.value); setError(null); }}
              placeholder="如: DeepSeek、我的 Ollama"
              className="input-base"
            />
          </div>

          <div>
            <label className="label-base">前缀 <span className="text-red-500">*</span></label>
            <input
              type="text"
              value={prefix}
              onChange={(e) => { setPrefix(e.target.value); setError(null); }}
              placeholder="如: ds、qf、ollama"
              className="input-base font-mono lowercase"
              disabled={isEdit}
            />
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              用于路由：<code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">{prefix || 'xxx'}/模型名</code>
            </p>
          </div>

          <div>
            <label className="label-base">API 地址 <span className="text-red-500">*</span></label>
            <input
              type="text"
              value={baseUrl}
              onChange={(e) => { setBaseUrl(e.target.value); setError(null); }}
              placeholder="https://api.example.com"
              className="input-base font-mono text-sm"
            />
          </div>

          {/* OAuth 授权区域 */}
          {isOAuth && isEdit && (
            <div className="p-4 bg-emerald-50 dark:bg-emerald-900/20 border border-emerald-200 dark:border-emerald-800 rounded-lg">
              {oauthFlow ? (
                <div className="text-center">
                  <p className="text-sm text-emerald-700 dark:text-emerald-300 font-medium mb-2">
                    请在浏览器中完成授权
                  </p>
                  <div className="bg-white dark:bg-gray-700 p-3 rounded-lg mb-3">
                    <p className="text-xs text-gray-500 dark:text-gray-400 mb-1">访问地址：</p>
                    <p className="font-mono text-sm text-gray-900 dark:text-white break-all">
                      {oauthFlow.verification_uri}
                    </p>
                    <p className="text-xs text-gray-500 dark:text-gray-400 mt-2 mb-1">输入代码：</p>
                    <p className="font-mono text-xl font-bold text-emerald-600 dark:text-emerald-400">
                      {oauthFlow.user_code}
                    </p>
                  </div>
                  {oauthPolling && (
                    <p className="text-xs text-gray-500 dark:text-gray-400 animate-pulse">
                      等待授权中...
                    </p>
                  )}
                  <button
                    onClick={cancelOAuthFlow}
                    className="mt-2 text-sm text-red-600 hover:text-red-800 dark:text-red-400"
                  >
                    取消授权
                  </button>
                </div>
              ) : oauthAuthorized ? (
                <div className="text-center">
                  <p className="text-sm text-emerald-700 dark:text-emerald-300 font-medium">
                    ✓ 授权成功
                  </p>
                </div>
              ) : (
                <div className="text-center">
                  <p className="text-sm text-gray-600 dark:text-gray-400 mb-3">
                    此供应商需要 OAuth 授权
                  </p>
                  <button
                    onClick={startOAuthFlow}
                    className="px-4 py-2 bg-emerald-500 text-white rounded-lg hover:bg-emerald-600 text-sm font-medium"
                  >
                    开始授权
                  </button>
                </div>
              )}
            </div>
          )}

          {/* API Key 输入（非 OAuth 供应商） */}
          {!isOAuth && (
            <div>
              <label className="label-base">
                API Key {!isEdit && <span className="text-red-500">*</span>}
                {isEdit && <span className="text-gray-400 text-xs font-normal">(留空不变)</span>}
              </label>
              <input
                type="password"
                value={apiKey}
                onChange={(e) => { setApiKey(e.target.value); setError(null); }}
                placeholder="sk-..."
                className="input-base font-mono"
              />
            </div>
          )}

          {/* 成本统计开关 */}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={enableCost}
              onChange={(e) => setEnableCost(e.target.checked)}
              className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500"
            />
            <span className="text-sm text-gray-900 dark:text-gray-300">启用成本统计</span>
            <span className="text-xs text-gray-400">(按 token 计费的供应商)</span>
          </label>

          {/* 模型列表 */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="label-base">支持的模型</label>
              <div className="flex items-center gap-2">
                {/* 从远程获取按钮（编辑模式） */}
                {isEdit && (
                  <button
                    type="button"
                    onClick={fetchRemoteModels}
                    disabled={loadingModels}
                    className="text-sm text-cyan-600 hover:text-cyan-800 dark:text-cyan-400 font-medium disabled:opacity-50"
                  >
                    {loadingModels ? '获取中...' : '从远程获取'}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    if (newModel.trim() && !models.some(m => m.name === newModel.trim())) {
                      setModels([...models, {
                        name: newModel.trim(),
                        endpoints: ['chat'],
                        pricing: enableCost && newModelPricing.input ? {
                          input: parseFloat(newModelPricing.input) || 0,
                          output: parseFloat(newModelPricing.output) || 0,
                        } : undefined,
                        rpm: newModelRpm ? parseInt(newModelRpm) : undefined,
                      }]);
                      setNewModel('');
                      setNewModelPricing({ input: '', output: '' });
                      setNewModelRpm('');
                      setNewModelEndpoints(['chat']);
                    } else if (!newModel.trim()) {
                      setError('请输入模型名称');
                    } else {
                      setError('模型已存在');
                    }
                  }}
                  className="text-sm text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 font-medium"
                >
                  + 手动添加
                </button>
              </div>
            </div>

            {/* 快速添加（手动输入） */}
            <div className="flex gap-2 mb-3">
              <input
                type="text"
                value={newModel}
                onChange={(e) => { setNewModel(e.target.value); setError(null); }}
                onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), setModels([...models, {
                  name: newModel.trim(),
                  endpoints: newModelEndpoints,
                  pricing: enableCost && newModelPricing.input ? {
                    input: parseFloat(newModelPricing.input) || 0,
                    output: parseFloat(newModelPricing.output) || 0,
                  } : undefined,
                  rpm: newModelRpm ? parseInt(newModelRpm) : undefined,
                }]))}
                placeholder="输入模型名后点击添加"
                className="input-base font-mono text-sm flex-1"
              />
            </div>

            {/* 模型卡片列表 */}
            {models.length > 0 ? (
              <div className="space-y-2">
                {models.map((m) => (
                  <ModelCard
                    key={m.name}
                    model={m}
                    enableCost={enableCost}
                    providerId={provider?.id || ''}
                    onToggleEndpoint={(ep) => toggleModelEndpoint(m.name, ep)}
                    onUpdateRpm={(rpm) => setModels(models.map(x => x.name === m.name ? { ...x, rpm } : x))}
                    onUpdatePricing={(field, val) => updateModelPricing(m.name, field, val)}
                    onRemove={() => setModels(models.filter(x => x.name !== m.name))}
                  />
                ))}
              </div>
            ) : (
              <div className="text-sm text-gray-400 dark:text-gray-500 text-center py-6 bg-gray-50 dark:bg-gray-700/30 rounded-lg border-2 border-dashed border-gray-200 dark:border-gray-600">
                暂无模型配置
              </div>
            )}
          </div>

          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={isActive}
              onChange={(e) => setIsActive(e.target.checked)}
              className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500"
            />
            <span className="text-sm text-gray-900 dark:text-gray-300">启用此供应商</span>
          </label>

          {error && (
            <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 flex items-start gap-2">
              <svg className="w-5 h-5 text-red-500 dark:text-red-400 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <span className="text-sm text-red-700 dark:text-red-300">{error}</span>
            </div>
          )}
        </div>

        <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 flex justify-end space-x-3">
          <button onClick={onClose} className="btn-secondary">取消</button>
          <button onClick={handleSave} disabled={saving} className="btn-primary">
            {saving ? '保存中...' : '保存'}
          </button>
        </div>
      </div>

      {/* 模型选择弹窗 */}
      {showModelPicker && (
        <div
          className="fixed inset-0 bg-black/30 flex items-center justify-center z-[60] p-4"
          onClick={() => setShowModelPicker(false)}
        >
          <div
            className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-2xl max-h-[70vh] overflow-hidden flex flex-col"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-4 py-3 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between shrink-0">
              <h4 className="font-medium text-gray-900 dark:text-white">
                选择模型
                {remoteModels.length > 0 && (
                  <span className="ml-2 text-xs text-gray-400">({remoteModels.length} 个)</span>
                )}
              </h4>
              <button
                onClick={() => setShowModelPicker(false)}
                className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-200"
              >
                <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            </div>

            {/* 搜索框 */}
            <div className="px-4 py-2 border-b border-gray-200 dark:border-gray-700 shrink-0">
              <input
                type="text"
                value={modelSearchQuery}
                onChange={(e) => setModelSearchQuery(e.target.value)}
                placeholder="搜索模型..."
                className="w-full px-3 py-2 text-sm bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg focus:ring-2 focus:ring-indigo-500 focus:border-transparent"
                autoFocus
              />
            </div>

            {/* 模型列表 */}
            <div className="overflow-y-auto flex-1">
              {(() => {
                const query = modelSearchQuery.toLowerCase().trim();
                const filtered = query
                  ? remoteModels.filter(m =>
                      m.id.toLowerCase().includes(query) ||
                      m.owned_by?.toLowerCase().includes(query)
                    )
                  : remoteModels;

                if (filtered.length === 0) {
                  return (
                    <div className="p-8 text-center text-gray-500 dark:text-gray-400">
                      {query ? `未找到匹配 "${modelSearchQuery}" 的模型` : '未找到可用模型'}
                    </div>
                  );
                }

                return (
                  <div className="divide-y divide-gray-100 dark:divide-gray-700">
                    {filtered.map((m) => {
                      const alreadyAdded = models.some(x => x.name === m.id);
                      return (
                        <button
                          key={m.id}
                          onClick={() => {
                            if (!alreadyAdded) {
                              addModelFromRemote(m.id);
                            }
                          }}
                          disabled={alreadyAdded}
                          className={`w-full px-4 py-3 text-left hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors ${
                            alreadyAdded ? 'opacity-50 cursor-not-allowed' : ''
                          }`}
                        >
                          <div className="font-mono text-sm text-gray-900 dark:text-white">{m.id}</div>
                          {m.owned_by && (
                            <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{m.owned_by}</div>
                          )}
                          {alreadyAdded && (
                            <span className="text-xs text-green-600 dark:text-green-400">已添加</span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                );
              })()}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default Providers;
