import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface ModelPricing {
  input: number;
  output: number;
}

interface ModelConfig {
  name: string;
  pricing?: ModelPricing;
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

      {/* 说明 */}
      <div className="mb-6 bg-gradient-to-r from-blue-50 to-indigo-50 dark:from-blue-900/20 dark:to-indigo-900/20 border border-blue-200 dark:border-blue-800 rounded-xl p-4">
        <h3 className="text-sm font-semibold text-blue-800 dark:text-blue-200 mb-2">
          使用方式
        </h3>
        <div className="text-sm text-blue-700 dark:text-blue-300 space-y-1">
          <p>1. 添加供应商，设置 <strong>前缀</strong>（如 ds、qf）</p>
          <p>2. 在 Group 中配置：<code className="bg-blue-100 dark:bg-blue-800 px-1 rounded">前缀/模型名</code></p>
          <p>3. 请求时使用 Group 名称，系统自动路由到对应供应商</p>
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
                    provider.models.slice(0, 8).map((model) => (
                      <span
                        key={model.name}
                        className="px-2.5 py-1 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs rounded-full font-mono"
                      >
                        {model.name}
                        {model.pricing && model.pricing.input > 0 && (
                          <span className="ml-1 text-amber-600 dark:text-amber-400">${model.pricing.input}/${model.pricing.output}</span>
                        )}
                      </span>
                    ))
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
          <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-2xl max-h-[80vh] overflow-hidden">
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

  const addModel = () => {
    if (newModel.trim() && !models.some(m => m.name === newModel.trim())) {
      const modelConfig: ModelConfig = { name: newModel.trim() };
      if (enableCost && newModelPricing.input && newModelPricing.output) {
        modelConfig.pricing = {
          input: parseFloat(newModelPricing.input) || 0,
          output: parseFloat(newModelPricing.output) || 0,
        };
      }
      setModels([...models, modelConfig]);
      setNewModel('');
      setNewModelPricing({ input: '', output: '' });
    }
  };

  const removeModel = (modelName: string) => {
    setModels(models.filter(m => m.name !== modelName));
  };

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
          base_url: baseUrl.trim(),
          api_key: isOAuth ? null : apiKey.trim(),
          models: models.length > 0 ? models.map(m => m.name) : null,
          auth_type: isOAuth ? 'oauth' : 'api_key',
          oauth: isOAuth ? provider?.oauth : null,
          headers: provider?.headers || null,
          auth_header: provider?.auth_header || null,
          auth_prefix: provider?.auth_prefix || null,
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
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-md overflow-hidden max-h-[90vh] overflow-y-auto">
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
            <label className="label-base">支持的模型</label>
            <div className="space-y-2">
              {/* 添加新模型 */}
              <div className="flex flex-col gap-2 p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={newModel}
                    onChange={(e) => setNewModel(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && (e.preventDefault(), addModel())}
                    placeholder="模型名，如 deepseek-chat"
                    className="input-base font-mono text-sm flex-1"
                  />
                  <button
                    type="button"
                    onClick={addModel}
                    className="px-3 py-2 bg-indigo-500 text-white rounded-md hover:bg-indigo-600 text-sm"
                  >
                    添加
                  </button>
                </div>
                {enableCost && (
                  <div className="flex gap-2 text-xs">
                    <div className="flex-1">
                      <label className="text-gray-500 dark:text-gray-400">输入 $/1M</label>
                      <input
                        type="number"
                        step="0.01"
                        value={newModelPricing.input}
                        onChange={(e) => setNewModelPricing({ ...newModelPricing, input: e.target.value })}
                        placeholder="0.00"
                        className="w-full px-2 py-1 bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded text-sm"
                      />
                    </div>
                    <div className="flex-1">
                      <label className="text-gray-500 dark:text-gray-400">输出 $/1M</label>
                      <input
                        type="number"
                        step="0.01"
                        value={newModelPricing.output}
                        onChange={(e) => setNewModelPricing({ ...newModelPricing, output: e.target.value })}
                        placeholder="0.00"
                        className="w-full px-2 py-1 bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded text-sm"
                      />
                    </div>
                  </div>
                )}
              </div>

              {/* 已添加的模型列表 */}
              {models.length > 0 && (
                <div className="space-y-1.5">
                  {models.map((m) => (
                    <div key={m.name} className="flex items-center gap-2 p-2 bg-gray-50 dark:bg-gray-700/30 rounded-lg">
                      <span className="font-mono text-sm text-gray-900 dark:text-white flex-1">{m.name}</span>
                      {enableCost && (
                        <div className="flex gap-1.5 items-center">
                          <input
                            type="number"
                            step="0.01"
                            value={m.pricing?.input || ''}
                            onChange={(e) => updateModelPricing(m.name, 'input', e.target.value)}
                            placeholder="输入"
                            className="w-16 px-1.5 py-0.5 text-xs bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded"
                          />
                          <span className="text-gray-400">/</span>
                          <input
                            type="number"
                            step="0.01"
                            value={m.pricing?.output || ''}
                            onChange={(e) => updateModelPricing(m.name, 'output', e.target.value)}
                            placeholder="输出"
                            className="w-16 px-1.5 py-0.5 text-xs bg-white dark:bg-gray-600 border border-gray-200 dark:border-gray-500 rounded"
                          />
                          <span className="text-[10px] text-gray-400">$/1M</span>
                        </div>
                      )}
                      <button
                        type="button"
                        onClick={() => removeModel(m.name)}
                        className="text-red-400 hover:text-red-600 text-lg leading-none"
                      >
                        ×
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {models.length === 0 && (
                <p className="text-xs text-gray-400 italic text-center py-2">未配置模型（将显示为"所有模型"）</p>
              )}
            </div>
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
            <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 text-sm text-red-700 dark:text-red-300">
              {error}
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
    </div>
  );
}

export default Providers;
