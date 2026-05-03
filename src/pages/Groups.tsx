import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';

// 端点类型定义
interface EndpointInfo {
  id: string;
  label: string;
  path: string;
  description: string;
}

const ENDPOINTS: EndpointInfo[] = [
  { id: 'chat', label: 'Chat', path: '/v1/chat/completions', description: '标准对话 API' },
  { id: 'responses', label: 'Responses', path: '/v1/responses', description: '响应 API (Codex, OpenCode)' },
  { id: 'claude', label: 'Claude', path: '/v1/messages', description: 'Claude 消息 API' },
  { id: 'gemini', label: 'Gemini', path: '/v1beta/models/{model}:generateContent', description: 'Gemini API' },
  { id: 'embeddings', label: '嵌入', path: '/v1/embeddings', description: '向量嵌入' },
  { id: 'images', label: '图像', path: '/v1/images/generations', description: '图像生成' },
  { id: 'videos', label: '视频', path: '/v1/videos/generations', description: '视频生成' },
  { id: 'music', label: '音乐', path: '/v1/music/generations', description: '音乐生成' },
  { id: 'audio', label: '语音', path: '/v1/audio/transcriptions', description: '语音转文字' },
  { id: 'tts', label: '语音合成', path: '/v1/audio/speech', description: '文字转语音' },
  { id: 'moderation', label: '审核', path: '/v1/moderations', description: '内容审核' },
  { id: 'rerank', label: '重排', path: '/v1/rerank', description: '搜索结果重排' },
];

interface Group {
  id: string;
  name: string;
  description?: string;
  models: GroupModel[];
  strategy: string;
  config: {
    max_retries: number;
    retry_delay_ms: number;
  };
  endpoint_type?: string;
  is_active: boolean;
}

interface GroupModel {
  model: string;
  weight: number;
  priority: number;
  enabled: boolean;
}

interface ModelConfig {
  name: string;
  pricing?: { input: number; output: number; };
  endpoints?: string[];
  rpm?: number;
  tpm?: number;
}

interface Provider {
  id: string;
  name: string;
  prefix: string;
  models: ModelConfig[];
  is_active: boolean;
}

function Groups() {
  const [groups, setGroups] = useState<Group[]>([]);
  const [providers, setProviders] = useState<Provider[]>([]);
  const [selectedEndpoint, setSelectedEndpoint] = useState<string>('chat');
  const [showModal, setShowModal] = useState(false);
  const [editingGroup, setEditingGroup] = useState<Group | null>(null);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [groupsResult, providersResult] = await Promise.all([
        invoke<Group[]>('get_groups'),
        invoke<Provider[]>('get_providers'),
      ]);
      setGroups(groupsResult);
      setProviders(providersResult.filter(p => p.is_active));
    } catch (error) {
      console.error('Failed to load data:', error);
    }
  };

  // 按端点过滤 Groups
  const filteredGroups = useMemo(() => {
    return groups.filter(g => {
      // 没有 endpoint_type 的 Group 归类到 chat
      const epType = g.endpoint_type || 'chat';
      return epType === selectedEndpoint;
    });
  }, [groups, selectedEndpoint]);

  // 统计每个端点的 Group 数量
  const endpointCounts = useMemo(() => {
    const counts: Record<string, number> = {};
    groups.forEach(g => {
      const epType = g.endpoint_type || 'chat';
      counts[epType] = (counts[epType] || 0) + 1;
    });
    return counts;
  }, [groups]);

  const handleDelete = async (id: string) => {
    if (confirm('确定要删除这个 Group 吗？')) {
      try {
        await invoke('delete_group', { id });
        loadData();
      } catch (error) {
        alert('删除失败: ' + error);
      }
    }
  };

  const currentEndpoint = ENDPOINTS.find(e => e.id === selectedEndpoint)!;

  return (
    <div className="flex h-[calc(100vh-4rem)]">
      {/* 左侧边栏：端点列表 */}
      <div className="w-64 bg-white dark:bg-gray-800 border-r border-gray-200 dark:border-gray-700 flex flex-col">
        <div className="px-4 py-3 border-b border-gray-200 dark:border-gray-700">
          <h2 className="text-sm font-semibold text-gray-700 dark:text-gray-300">端点类型</h2>
        </div>
        <div className="flex-1 overflow-y-auto">
          {ENDPOINTS.map(endpoint => (
            <button
              key={endpoint.id}
              onClick={() => setSelectedEndpoint(endpoint.id)}
              className={`w-full px-4 py-2.5 text-left transition-colors ${
                selectedEndpoint === endpoint.id
                  ? 'bg-indigo-50 dark:bg-indigo-900/30 border-r-2 border-indigo-500'
                  : 'hover:bg-gray-50 dark:hover:bg-gray-700/50'
              }`}
            >
              <div className="flex items-center justify-between">
                <span className={`text-sm font-medium ${
                  selectedEndpoint === endpoint.id
                    ? 'text-indigo-700 dark:text-indigo-300'
                    : 'text-gray-700 dark:text-gray-300'
                }`}>
                  {endpoint.label}
                </span>
                {endpointCounts[endpoint.id] !== undefined && (
                  <span className="text-xs text-gray-400 dark:text-gray-500">
                    {endpointCounts[endpoint.id]}
                  </span>
                )}
              </div>
              <code className="text-xs text-gray-500 dark:text-gray-400 font-mono block mt-0.5">
                {endpoint.path}
              </code>
            </button>
          ))}
        </div>
      </div>

      {/* 右侧主内容区 */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* 头部信息 */}
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800">
          <div className="flex items-center justify-between">
            <div>
              <h1 className="text-lg font-semibold text-gray-900 dark:text-white">
                {currentEndpoint.label}
              </h1>
              <code className="text-sm text-indigo-600 dark:text-indigo-400 font-mono">
                {currentEndpoint.path}
              </code>
              <span className="text-sm text-gray-400 dark:text-gray-500 ml-2">
                {currentEndpoint.description}
              </span>
            </div>
            <button
              onClick={() => {
                setEditingGroup(null);
                setShowModal(true);
              }}
              className="btn-primary"
            >
              + 创建分组
            </button>
          </div>
        </div>

        {/* Groups 列表 */}
        <div className="flex-1 overflow-y-auto p-6">
          {filteredGroups.length === 0 ? (
            <div className="text-center py-12">
              <h3 className="text-lg font-medium text-gray-900 dark:text-white mb-2">
                暂无 {currentEndpoint.label} 分组
              </h3>
              <p className="text-gray-500 dark:text-gray-400 mb-4">
                创建一个分组来组合模型，实现故障转移和负载均衡
              </p>
              <button
                onClick={() => {
                  setEditingGroup(null);
                  setShowModal(true);
                }}
                className="btn-primary"
              >
                创建第一个分组
              </button>
            </div>
          ) : (
            <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
              {filteredGroups.map(group => (
                <div
                  key={group.id}
                  className={`bg-white dark:bg-gray-800 rounded-lg border ${
                    group.is_active
                      ? 'border-gray-200 dark:border-gray-700'
                      : 'border-gray-200 dark:border-gray-700 opacity-60'
                  } p-4 hover:shadow-md transition-shadow`}
                >
                  <div className="flex items-start justify-between mb-3">
                    <div className="flex items-center gap-2">
                      <h3 className="font-mono font-semibold text-gray-900 dark:text-white">
                        {group.name}
                      </h3>
                      {!group.is_active && (
                        <span className="text-xs px-1.5 py-0.5 bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400 rounded">
                          已禁用
                        </span>
                      )}
                    </div>
                    <div className="flex items-center gap-1">
                      <button
                        onClick={() => {
                          setEditingGroup(group);
                          setShowModal(true);
                        }}
                        className="text-gray-400 hover:text-indigo-600 p-1"
                      >
                        <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                        </svg>
                      </button>
                      <button
                        onClick={() => handleDelete(group.id)}
                        className="text-gray-400 hover:text-red-600 p-1"
                      >
                        <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                        </svg>
                      </button>
                    </div>
                  </div>

                  <div className="text-sm text-gray-500 dark:text-gray-400 mb-3">
                    {group.models.filter(m => m.enabled).length} 个模型 · {
                      { priority: '优先级', weighted: '权重', round_robin: '轮询', random: '随机' }[group.strategy] || group.strategy
                    }
                  </div>

                  <div className="flex flex-wrap gap-1">
                    {group.models.slice(0, 3).map((m, i) => (
                      <span
                        key={i}
                        className={`text-xs px-2 py-0.5 rounded font-mono ${
                          m.enabled
                            ? 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-300'
                            : 'bg-gray-50 dark:bg-gray-800 text-gray-400 dark:text-gray-500 line-through'
                        }`}
                      >
                        {m.model.split('/').pop()}
                      </span>
                    ))}
                    {group.models.length > 3 && (
                      <span className="text-xs px-2 py-0.5 bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400">
                        +{group.models.length - 3}
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* Group Modal */}
      {showModal && (
        <GroupModal
          group={editingGroup}
          providers={providers}
          endpointType={selectedEndpoint}
          onClose={() => {
            setShowModal(false);
            setEditingGroup(null);
            loadData();
          }}
        />
      )}

    </div>
  );
}

function GroupModal({
  group,
  providers,
  endpointType: _endpointType,
  onClose,
}: {
  group: Group | null;
  providers: Provider[];
  endpointType: string;
  onClose: () => void;
}) {
  const endpointType = _endpointType;
  const [name, setName] = useState(group?.name || '');
  const [strategy, setStrategy] = useState(group?.strategy || 'priority');
  const [isActive, setIsActive] = useState(group?.is_active ?? true);
  const [models, setModels] = useState<GroupModel[]>(group?.models || []);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEdit = !!group;
  const maxRetries = group?.config?.max_retries ?? 3;
  const retryDelay = group?.config?.retry_delay_ms ?? 1000;

  // 根据端点类型过滤供应商模型
  const filterModelsByEndpoint = (provider: Provider) => {
    return provider.models.filter(m =>
      m.endpoints?.includes(endpointType) ||
      (!m.endpoints && endpointType === 'chat') // 默认支持 chat
    );
  };

  const handleAddModel = () => {
    if (providers.length === 0) {
      setError('请先在 Providers 页面添加供应商');
      return;
    }

    // 自动选择第一个有模型的供应商
    const providerWithModels = providers.find(p => filterModelsByEndpoint(p).length > 0);
    if (!providerWithModels) {
      setError(`没有供应商支持 ${endpointType} 端点`);
      return;
    }

    const firstModel = filterModelsByEndpoint(providerWithModels)[0];
    setModels([
      ...models,
      { model: `${providerWithModels.prefix}/${firstModel.name}`, weight: 1, priority: models.length, enabled: true }
    ]);
    setError(null);
  };

  const handleRemoveModel = (index: number) => {
    setModels(models.filter((_, i) => i !== index));
  };

  const handleModelChange = (index: number, field: keyof GroupModel, value: string | number | boolean) => {
    const updated = [...models];
    updated[index] = { ...updated[index], [field]: value };
    setModels(updated);
  };

  const handleSave = async () => {
    if (!name.trim()) {
      setError('请输入 Group 名称');
      return;
    }

    const validModels = models.filter(m => m.model && m.model.trim());
    if (validModels.length === 0) {
      setError('请至少配置一个有效的模型');
      return;
    }

    setSaving(true);
    setError(null);
    try {
      if (isEdit) {
        const updated: Group = {
          ...group!,
          name,
          strategy,
          is_active: isActive,
          config: { max_retries: maxRetries, retry_delay_ms: retryDelay },
          models: validModels,
          endpoint_type: endpointType,
        };
        await invoke('update_group', { id: group.id, group: updated });
      } else {
        await invoke('create_group', {
          name,
          description: null,
          strategy,
          endpointType,
        });
        // 获取刚创建的 group 并添加模型
        const groups = await invoke<Group[]>('get_groups');
        const newGroup = groups.find(g => g.name === name);
        if (newGroup) {
          for (const m of validModels) {
            await invoke('add_model_to_group', {
              groupId: newGroup.id,
              model: m.model,
              priority: m.priority,
              weight: m.weight,
            });
          }
        }
      }
      onClose();
    } catch (e) {
      setError('保存失败: ' + e);
    } finally {
      setSaving(false);
    }
  };

  const currentEndpoint = ENDPOINTS.find(e => e.id === endpointType)!;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-4xl max-h-[90vh] flex flex-col overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700 shrink-0">
          <div className="flex items-center justify-between">
            <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
              {isEdit ? '编辑分组' : `创建 ${currentEndpoint.label} 分组`}
            </h3>
            <code className="text-sm text-indigo-600 dark:text-indigo-400 font-mono">
              {currentEndpoint.path}
            </code>
          </div>
        </div>

        <div className="px-6 py-5 space-y-5 overflow-y-auto flex-1">
          {providers.length === 0 && (
            <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-lg p-3 text-sm text-amber-700 dark:text-amber-300">
              请先在 <a href="/providers" className="underline font-medium">Providers 页面</a> 添加供应商
            </div>
          )}

          <div>
            <label className="label-base">Group 名称 <span className="text-red-500">*</span></label>
            <input
              type="text"
              value={name}
              onChange={(e) => { setName(e.target.value); setError(null); }}
              className="input-base font-mono"
            />
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              请求时使用：<code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">model=&quot;{name || 'xxx'}&quot;</code>
            </p>
          </div>

          <div>
            <label className="label-base">路由策略</label>
            <select value={strategy} onChange={(e) => setStrategy(e.target.value)} className="select-base">
              <option value="priority">优先级（按顺序尝试）</option>
              <option value="weighted">权重（按权重随机）</option>
              <option value="round_robin">轮询</option>
              <option value="random">随机</option>
            </select>
          </div>

          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="text-sm font-medium text-gray-700 dark:text-gray-300">
                模型列表 <span className="text-indigo-500">({currentEndpoint.label})</span>
              </label>
              <button
                type="button"
                onClick={handleAddModel}
                disabled={providers.length === 0}
                className="text-sm text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 font-medium disabled:opacity-50 disabled:cursor-not-allowed"
              >
                + 添加模型
              </button>
            </div>

            {models.length === 0 ? (
              <div className="text-sm text-gray-500 dark:text-gray-400 py-8 text-center bg-gray-50 dark:bg-gray-700/50 rounded-lg border-2 border-dashed border-gray-200 dark:border-gray-600">
                点击&quot;添加模型&quot;配置路由目标
              </div>
            ) : (
              <div className="space-y-3">
                {models.map((m, idx) => {
                  const [currentPrefix, ...modelParts] = m.model.split('/');
                  const currentModelName = modelParts.join('/') || '';
                  const selectedProvider = providers.find(p => p.prefix === currentPrefix);
                  const filteredModels = selectedProvider ? filterModelsByEndpoint(selectedProvider) : [];

                  return (
                    <div key={idx} className={`p-4 rounded-lg space-y-3 ${m.enabled ? 'bg-gray-50 dark:bg-gray-700/50' : 'bg-gray-100 dark:bg-gray-800/50 opacity-60'}`}>
                      <div className="flex items-center gap-3">
                        <div className="flex-1">
                          <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">供应商</label>
                          <select
                            value={currentPrefix}
                            onChange={(e) => {
                              const newPrefix = e.target.value;
                              const newProvider = providers.find(p => p.prefix === newPrefix);
                              const filtered = newProvider ? filterModelsByEndpoint(newProvider) : [];
                              const newModelName = filtered[0]?.name || '';
                              handleModelChange(idx, 'model', `${newPrefix}/${newModelName}`);
                            }}
                            className="select-base text-sm w-full"
                          >
                            <option value="">选择供应商</option>
                            {providers.map((p) => {
                              const filtered = filterModelsByEndpoint(p);
                              return (
                                <option key={p.id} value={p.prefix}>
                                  {p.name} ({p.prefix}) {filtered.length > 0 && `(${filtered.length} 模型)`}
                                </option>
                              );
                            })}
                          </select>
                        </div>

                        <div className="flex-1">
                          <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">模型</label>
                          {filteredModels.length > 0 ? (
                            <select
                              value={currentModelName}
                              onChange={(e) => handleModelChange(idx, 'model', `${currentPrefix}/${e.target.value}`)}
                              className="select-base text-sm w-full font-mono"
                            >
                              {filteredModels.map((model) => (
                                <option key={model.name} value={model.name}>
                                  {model.name}
                                </option>
                              ))}
                            </select>
                          ) : (
                            <input
                              type="text"
                              value={currentModelName}
                              onChange={(e) => handleModelChange(idx, 'model', `${currentPrefix}/${e.target.value}`)}
                              className="input-base text-sm font-mono w-full"
                            />
                          )}
                        </div>

                        <button
                          type="button"
                          onClick={() => handleRemoveModel(idx)}
                          className="mt-5 text-red-500 hover:text-red-700 p-1"
                        >
                          <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                          </svg>
                        </button>
                      </div>

                      <div className="flex items-center gap-3">
                        <div>
                          <label className="text-xs text-gray-500 dark:text-gray-400 mr-1">优先级</label>
                          <input
                            type="number"
                            value={m.priority}
                            onChange={(e) => handleModelChange(idx, 'priority', parseInt(e.target.value) || 0)}
                            className="input-base w-20 text-sm py-1"
                          />
                        </div>
                        <div>
                          <label className="text-xs text-gray-500 dark:text-gray-400 mr-1">权重</label>
                          <input
                            type="number"
                            value={m.weight}
                            onChange={(e) => handleModelChange(idx, 'weight', parseInt(e.target.value) || 1)}
                            className="input-base w-20 text-sm py-1"
                          />
                        </div>
                        <div className="flex items-center gap-1.5">
                          <label className="text-xs text-gray-500 dark:text-gray-400">启用</label>
                          <button
                            type="button"
                            onClick={() => handleModelChange(idx, 'enabled', !m.enabled)}
                            className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                              m.enabled ? 'bg-indigo-600' : 'bg-gray-300 dark:bg-gray-600'
                            }`}
                          >
                            <span className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                              m.enabled ? 'translate-x-4' : 'translate-x-1'
                            }`} />
                          </button>
                        </div>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          <div className="flex items-center gap-4 pt-2">
            <label className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={isActive}
                onChange={(e) => setIsActive(e.target.checked)}
                className="rounded border-gray-300 dark:border-gray-600 text-indigo-600 focus:ring-indigo-500"
              />
              <span className="text-sm text-gray-700 dark:text-gray-300">启用此分组</span>
            </label>
          </div>

          {error && (
            <div className="mx-6 mb-4 p-3 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg flex items-start gap-2">
              <svg className="w-5 h-5 text-red-500 dark:text-red-400 shrink-0 mt-0.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <span className="text-sm text-red-700 dark:text-red-300">{error}</span>
            </div>
          )}
        </div>

        <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 flex justify-end gap-3 shrink-0">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white"
          >
            取消
          </button>
          <button
            type="button"
            onClick={handleSave}
            disabled={saving}
            className="btn-primary"
          >
            {saving ? '保存中...' : (isEdit ? '保存' : '创建')}
          </button>
        </div>
      </div>
    </div>
  );
}

export default Groups;
