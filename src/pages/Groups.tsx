import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

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
  is_active: boolean;
}

interface GroupModel {
  model: string;
  weight: number;
  priority: number;
}

interface ModelConfig {
  name: string;
  pricing?: {
    input: number;
    output: number;
  };
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

  const strategyLabels: Record<string, string> = {
    priority: '优先级',
    weighted: '权重',
    round_robin: '轮询',
    random: '随机',
    least_used: '最少使用',
    cost_optimized: '成本优化',
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">
            Group 路由组
          </h1>
          <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">
            配置模型路由规则，实现故障转移和负载均衡
          </p>
        </div>
        <button
          onClick={() => {
            setEditingGroup(null);
            setShowModal(true);
          }}
          className="btn-primary"
        >
          创建 Group
        </button>
      </div>

      {/* 说明 */}
      <div className="mb-6 bg-gradient-to-r from-indigo-50 to-purple-50 dark:from-indigo-900/20 dark:to-purple-900/20 border border-indigo-200 dark:border-indigo-800 rounded-xl p-4">
        <h3 className="text-sm font-semibold text-indigo-800 dark:text-indigo-200 mb-2">
          工作原理
        </h3>
        <div className="text-sm text-indigo-700 dark:text-indigo-300 space-y-1">
          <p>1. 请求 <code className="bg-indigo-100 dark:bg-indigo-800 px-1.5 py-0.5 rounded font-mono text-xs">model="free"</code></p>
          <p>2. 查找名为 "free" 的 Group</p>
          <p>3. 按策略选择模型（如 <code className="bg-indigo-100 dark:bg-indigo-800 px-1.5 py-0.5 rounded font-mono text-xs">ds/deepseek-chat</code>）</p>
          <p>4. 通过前缀 "ds" 找到供应商，发送请求</p>
        </div>
      </div>

      {/* 可用供应商前缀 */}
      {providers.length > 0 && (
        <div className="mb-6 bg-gray-50 dark:bg-gray-800/50 border border-gray-200 dark:border-gray-700 rounded-lg p-4">
          <p className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">可用供应商前缀</p>
          <div className="flex flex-wrap gap-2">
            {providers.map((p) => (
              <span
                key={p.id}
                className="inline-flex items-center px-2.5 py-1 bg-white dark:bg-gray-700 text-gray-700 dark:text-gray-300 text-xs rounded border border-gray-200 dark:border-gray-600"
              >
                <span className="font-mono text-purple-600 dark:text-purple-400">{p.prefix}</span>
                <span className="mx-1 text-gray-400">=</span>
                <span>{p.name}</span>
              </span>
            ))}
          </div>
        </div>
      )}

      {groups.length === 0 ? (
        <div className="card-base p-8 text-center">
          <p className="text-gray-500 dark:text-gray-400">暂无 Group</p>
          {providers.length === 0 ? (
            <p className="mt-2 text-sm text-amber-600 dark:text-amber-400">
              请先在 Providers 页面添加供应商
            </p>
          ) : (
            <button
              onClick={() => setShowModal(true)}
              className="mt-4 text-indigo-600 hover:text-indigo-900 dark:text-indigo-400"
            >
              创建第一个 Group
            </button>
          )}
        </div>
      ) : (
        <div className="space-y-4">
          {groups.map((group) => (
            <div key={group.id} className="card-base overflow-hidden">
              <div className="px-6 py-4 border-b border-gray-100 dark:border-gray-700 flex items-center justify-between">
                <div className="flex items-center space-x-4">
                  <code className="px-3 py-1.5 bg-indigo-100 dark:bg-indigo-900/40 text-indigo-700 dark:text-indigo-300 rounded-lg font-mono text-sm font-medium">
                    {group.name}
                  </code>
                  <span className={`px-2.5 py-1 text-xs rounded-full font-medium ${
                    group.is_active
                      ? 'bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400'
                      : 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400'
                  }`}>
                    {group.is_active ? '启用' : '禁用'}
                  </span>
                  <span className="text-sm text-gray-500 dark:text-gray-400 bg-gray-100 dark:bg-gray-700 px-2 py-0.5 rounded">
                    {strategyLabels[group.strategy] || group.strategy}
                  </span>
                </div>
                <div className="flex space-x-3">
                  <button
                    onClick={() => {
                      setEditingGroup(group);
                      setShowModal(true);
                    }}
                    className="text-sm text-indigo-600 hover:text-indigo-800 dark:text-indigo-400 font-medium transition-colors"
                  >
                    编辑
                  </button>
                  <button
                    onClick={() => handleDelete(group.id)}
                    className="text-sm text-red-600 hover:text-red-800 dark:text-red-400 font-medium transition-colors"
                  >
                    删除
                  </button>
                </div>
              </div>

              <div className="px-6 py-4">
                <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">
                  模型列表 ({group.models.length})
                </h4>
                {group.models.length === 0 ? (
                  <p className="text-sm text-gray-400 italic">未配置模型</p>
                ) : (
                  <div className="space-y-2">
                    {group.models.map((m, idx) => (
                      <div key={idx} className="flex items-center justify-between bg-gray-50 dark:bg-gray-700/50 rounded-lg p-3">
                        <div className="flex items-center space-x-3">
                          <span className="px-2 py-0.5 bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 text-xs rounded font-medium">
                            #{m.priority}
                          </span>
                          <code className="text-sm font-mono text-gray-900 dark:text-white">
                            {m.model}
                          </code>
                        </div>
                        <span className="text-sm text-gray-500 dark:text-gray-400">
                          权重: {m.weight}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="px-6 py-3 bg-gray-50 dark:bg-gray-700/30 border-t border-gray-100 dark:border-gray-700 text-sm text-gray-500 dark:text-gray-400">
                重试: {group.config.max_retries} 次，间隔: {group.config.retry_delay_ms}ms
              </div>
            </div>
          ))}
        </div>
      )}

      {showModal && (
        <GroupModal
          group={editingGroup}
          providers={providers}
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
  onClose,
}: {
  group: Group | null;
  providers: Provider[];
  onClose: () => void;
}) {
  const [name, setName] = useState(group?.name || '');
  const [strategy, setStrategy] = useState(group?.strategy || 'priority');
  const [isActive, setIsActive] = useState(group?.is_active ?? true);
  const [maxRetries, setMaxRetries] = useState(group?.config?.max_retries ?? 3);
  const [retryDelay, setRetryDelay] = useState(group?.config?.retry_delay_ms ?? 1000);
  const [models, setModels] = useState<GroupModel[]>(group?.models || []);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEdit = !!group;

  const handleAddModel = () => {
    if (providers.length === 0) {
      setError('请先在 Providers 页面添加供应商');
      return;
    }

    setModels([
      ...models,
      { model: '', weight: 1, priority: models.length }
    ]);
    setError(null);
  };

  const handleRemoveModel = (index: number) => {
    setModels(models.filter((_, i) => i !== index));
  };

  const handleModelChange = (index: number, field: keyof GroupModel, value: string | number) => {
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
        };
        await invoke('update_group', { id: group.id, group: updated });
      } else {
        await invoke('create_group', {
          name,
          description: null,
          strategy,
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

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-lg max-h-[90vh] flex flex-col overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700 shrink-0">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            {isEdit ? '编辑 Group' : '创建 Group'}
          </h3>
        </div>

        <div className="px-6 py-5 space-y-5 overflow-y-auto flex-1">
          {/* 提示：需要先添加供应商 */}
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
              placeholder="如: free、gpt-4、default"
              className="input-base font-mono"
              disabled={isEdit}
            />
            <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
              请求时使用此名称：<code className="bg-gray-100 dark:bg-gray-700 px-1 rounded">model="{name || 'xxx'}"</code>
            </p>
          </div>

          {/* 策略选择 */}
          <div>
            <label className="label-base">路由策略</label>
            <select
              value={strategy}
              onChange={(e) => setStrategy(e.target.value)}
              className="select-base"
            >
              <option value="priority">优先级（按顺序尝试）</option>
              <option value="weighted">权重（按权重随机）</option>
              <option value="round_robin">轮询</option>
              <option value="random">随机</option>
            </select>
          </div>

          {/* 模型列表 */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <label className="text-sm font-medium text-gray-700 dark:text-gray-300">模型列表</label>
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
                点击"添加模型"配置路由目标
              </div>
            ) : (
              <div className="space-y-3">
                {models.map((m, idx) => {
                  // 解析当前模型的前缀和模型名
                  const [currentPrefix, ...modelParts] = m.model.split('/');
                  const currentModelName = modelParts.join('/') || '';
                  const selectedProvider = providers.find(p => p.prefix === currentPrefix);

                  return (
                    <div key={idx} className="bg-gray-50 dark:bg-gray-700/50 p-4 rounded-lg space-y-3">
                      <div className="flex items-center gap-3">
                        {/* 选择供应商 */}
                        <div className="flex-1">
                          <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">供应商</label>
                          <select
                            value={currentPrefix}
                            onChange={(e) => {
                              const newPrefix = e.target.value;
                              const newProvider = providers.find(p => p.prefix === newPrefix);
                              // 如果选择了新供应商，尝试选择第一个模型
                              const newModelName = newProvider?.models?.[0]?.name || currentModelName;
                              handleModelChange(idx, 'model', `${newPrefix}/${newModelName}`);
                            }}
                            className="select-base text-sm w-full"
                          >
                            <option value="">选择供应商</option>
                            {providers.map((p) => (
                              <option key={p.id} value={p.prefix}>
                                {p.name} ({p.prefix})
                              </option>
                            ))}
                          </select>
                        </div>

                        {/* 选择模型 */}
                        <div className="flex-1">
                          <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">模型</label>
                          {selectedProvider?.models?.length && !selectedProvider.models.some(m => m.name === '*') ? (
                            <select
                              value={currentModelName}
                              onChange={(e) => handleModelChange(idx, 'model', `${currentPrefix}/${e.target.value}`)}
                              className="select-base text-sm w-full font-mono"
                            >
                              <option value="">选择模型</option>
                              {selectedProvider.models.map((model) => (
                                <option key={model.name} value={model.name}>{model.name}</option>
                              ))}
                            </select>
                          ) : (
                            <input
                              type="text"
                              value={currentModelName}
                              onChange={(e) => handleModelChange(idx, 'model', `${currentPrefix}/${e.target.value}`)}
                              placeholder="输入模型名"
                              className="input-base text-sm font-mono w-full"
                            />
                          )}
                        </div>

                        {/* 删除按钮 */}
                        <button
                          type="button"
                          onClick={() => handleRemoveModel(idx)}
                          className="mt-5 text-red-500 hover:text-red-700 p-1 transition-colors"
                        >
                          <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                          </svg>
                        </button>
                      </div>

                      <div className="flex items-center justify-between">
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
                        </div>
                        {m.model && (
                          <code className="text-xs bg-indigo-100 dark:bg-indigo-900/30 text-indigo-700 dark:text-indigo-300 px-2 py-1 rounded">
                            {m.model}
                          </code>
                        )}
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>

          {/* 配置 */}
          <div className="border-t border-gray-200 dark:border-gray-700 pt-5">
            <h4 className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">故障转移配置</h4>
            <div className="flex gap-4">
              <div>
                <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">最大重试</label>
                <input
                  type="number"
                  value={maxRetries}
                  onChange={(e) => setMaxRetries(parseInt(e.target.value) || 0)}
                  className="input-base w-24 text-sm py-1.5"
                />
              </div>
              <div>
                <label className="text-xs text-gray-500 dark:text-gray-400 block mb-1">重试间隔 (ms)</label>
                <input
                  type="number"
                  value={retryDelay}
                  onChange={(e) => setRetryDelay(parseInt(e.target.value) || 0)}
                  className="input-base w-28 text-sm py-1.5"
                />
              </div>
            </div>
          </div>

          {/* 启用状态 */}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={isActive}
              onChange={(e) => setIsActive(e.target.checked)}
              className="w-4 h-4 text-indigo-600 border-gray-300 rounded focus:ring-indigo-500"
            />
            <span className="text-sm text-gray-900 dark:text-gray-300">启用此 Group</span>
          </label>

          {/* 错误提示 */}
          {error && (
            <div className="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-3 text-sm text-red-700 dark:text-red-300">
              {error}
            </div>
          )}
        </div>

        <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-800/50 flex justify-end gap-3 shrink-0">
          <button onClick={onClose} className="btn-secondary">
            取消
          </button>
          <button onClick={handleSave} disabled={saving} className="btn-primary">
            {saving ? '保存中...' : '保存'}
          </button>
        </div>
      </div>
    </div>
  );
}

export default Groups;
