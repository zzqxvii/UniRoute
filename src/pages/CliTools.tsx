import { useState, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useProxy } from '../components/ProxyContext';
import { sortDefaultFirst } from './Groups';

interface CliToolStatus {
  toolId: string;
  displayName: string;
  description: string;
  installed: boolean;
  takenOver: boolean;
  proxyUrl?: string;
  sourceType?: string;
  sourceValue?: string;
  takenOverAt?: string;
  configPath: string;
  homepage?: string;
  requiredEndpointType?: string;
}

interface CliToolConfig {
  toolId: string;
  enabled: boolean;
  autoTakeover: boolean;
  sourceType: string;
  sourceValue: string;
}

interface TakeoverResult {
  toolId: string;
  success: boolean;
  message: string;
}

interface SnapshotInfo {
  id: string;
  toolId: string;
  createdAt: string;
  sizeBytes: number;
}

interface ConfigFileEntry {
  filename: string;
  content: string;
}

interface Group {
  id: string;
  name: string;
  description?: string;
  strategy: string;
  models: { model: string; weight: number; priority: number; enabled: boolean }[];
  endpoint_type?: string;
  is_active: boolean;
}

function defaultConfig(toolId: string): CliToolConfig {
  return { toolId, enabled: true, autoTakeover: true, sourceType: 'group', sourceValue: 'free' };
}

function CliTools() {
  const { proxyStatus } = useProxy();
  const [tools, setTools] = useState<CliToolStatus[]>([]);
  const [groups, setGroups] = useState<Group[]>([]);
  const [toolConfigs, setToolConfigs] = useState<Record<string, CliToolConfig>>({});
  const [loading, setLoading] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [selectedTool, setSelectedTool] = useState<string | null>(null);
  const [showSourcePicker, setShowSourcePicker] = useState(false);
  const [showConfigViewer, setShowConfigViewer] = useState(false);
  const [configFiles, setConfigFiles] = useState<ConfigFileEntry[]>([]);
  const [configViewerTitle, setConfigViewerTitle] = useState('');
  const [showSnapshotManager, setShowSnapshotManager] = useState(false);
  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([]);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [statuses, groupsResult, configs] = await Promise.all([
        invoke<CliToolStatus[]>('get_cli_tools_status'),
        invoke<Group[]>('get_groups'),
        invoke<Record<string, CliToolConfig>>('get_all_cli_tool_configs'),
      ]);
      setTools(statuses);
      setGroups(groupsResult.filter(g => g.is_active).sort(sortDefaultFirst));
      setToolConfigs(configs);
    } catch (e) {
      console.error('Failed to load CLI tools:', e);
    }
  };

  const proxyUrl = useMemo(() => {
    const port = proxyStatus.port || 8080;
    return `http://127.0.0.1:${port}/v1`;
  }, [proxyStatus.port]);

  const showMessage = (msg: string) => {
    setActionMessage(msg);
    setTimeout(() => setActionMessage(null), 3000);
  };

  const handleTakeover = async (toolId: string) => {
    if (!proxyStatus.is_running) {
      showMessage('请先启动代理服务器');
      return;
    }
    setLoading(true);
    try {
      const config = toolConfigs[toolId] || defaultConfig(toolId);
      const result = await invoke<TakeoverResult>('takeover_cli_tool', {
        toolId,
        proxyUrl,
        sourceType: config.sourceType,
        sourceValue: config.sourceValue,
      });
      showMessage(result.message);
      loadData();
    } catch (e) {
      showMessage('接管失败: ' + e);
    } finally {
      setLoading(false);
    }
  };

  const handleRestore = async (toolId: string) => {
    setLoading(true);
    try {
      const result = await invoke<TakeoverResult>('restore_cli_tool', { toolId });
      showMessage(result.message);
      loadData();
    } catch (e) {
      showMessage('恢复失败: ' + e);
    } finally {
      setLoading(false);
    }
  };

  const handleOpenSourcePicker = (toolId: string) => {
    setSelectedTool(toolId);
    setShowSourcePicker(true);
  };

  const handleSourceSelect = async (sourceType: string, sourceValue: string) => {
    if (!selectedTool) return;
    setShowSourcePicker(false);

    const newConfig: CliToolConfig = {
      ...(toolConfigs[selectedTool] || defaultConfig(selectedTool)),
      sourceType,
      sourceValue,
    };

    try {
      await invoke('save_cli_tool_config', { config: newConfig });

      // If already taken over, update the model
      const tool = tools.find(t => t.toolId === selectedTool);
      if (tool?.takenOver) {
        await invoke('update_cli_tool_model', { toolId: selectedTool, sourceType, sourceValue });
        showMessage('模型源已更新');
      } else {
        showMessage('配置已保存');
      }
      loadData();
    } catch (e) {
      showMessage('更新失败: ' + e);
    }
  };

  const handleToggleEnabled = async (toolId: string, enabled: boolean) => {
    const newConfig: CliToolConfig = {
      ...(toolConfigs[toolId] || defaultConfig(toolId)),
      enabled,
    };
    try {
      await invoke('save_cli_tool_config', { config: newConfig });
      setToolConfigs(prev => ({ ...prev, [toolId]: newConfig }));
      showMessage(enabled ? '已启用' : '已禁用');
    } catch (e) {
      showMessage('保存失败: ' + e);
    }
  };

  const handleOpenConfigDir = async (toolId: string) => {
    try {
      await invoke('open_cli_config_dir', { toolId });
    } catch (e) {
      showMessage('打开失败: ' + e);
    }
  };

  const handleViewConfig = async (toolId: string) => {
    try {
      const files = await invoke<ConfigFileEntry[]>('get_cli_tool_current_config', { toolId });
      const tool = tools.find(t => t.toolId === toolId);
      setConfigFiles(files);
      setConfigViewerTitle(tool?.displayName || toolId);
      setShowConfigViewer(true);
    } catch (e) {
      showMessage('读取配置失败: ' + e);
    }
  };

  const handleViewSnapshots = async (toolId: string) => {
    try {
      const list = await invoke<SnapshotInfo[]>('list_cli_tool_snapshots', { toolId });
      setSnapshots(list);
      setSelectedTool(toolId);
      setShowSnapshotManager(true);
    } catch (e) {
      showMessage('加载快照失败: ' + e);
    }
  };

  const handleRestoreFromSnapshot = async (snapshotId: string) => {
    if (!selectedTool) return;
    setLoading(true);
    try {
      const result = await invoke<TakeoverResult>('restore_cli_tool_from_snapshot', {
        toolId: selectedTool,
        snapshotId,
      });
      showMessage(result.message);
      setShowSnapshotManager(false);
      loadData();
    } catch (e) {
      showMessage('恢复失败: ' + e);
    } finally {
      setLoading(false);
    }
  };

  const handleViewSnapshotContent = async (snapshotId: string) => {
    try {
      const files = await invoke<ConfigFileEntry[]>('get_cli_tool_snapshot_content', { snapshotId });
      setConfigFiles(files);
      setConfigViewerTitle(`快照 ${snapshotId}`);
      setShowConfigViewer(true);
    } catch (e) {
      showMessage('加载快照内容失败: ' + e);
    }
  };

  const getToolIcon = (toolId: string) => {
    switch (toolId) {
      case 'claude': return '🤖';
      case 'codex': return '⚡';
      case 'pi': return '🔧';
      case 'droid': return '🤖';
      case 'gsd': return '🛠';
      default: return '📦';
    }
  };

  const getSourceLabel = (toolId: string) => {
    const config = toolConfigs[toolId];
    if (!config) return '未配置';
    if (config.sourceType === 'group') return `Group: ${config.sourceValue}`;
    return `Model: ${config.sourceValue}`;
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <div>
          <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">CLI 工具配置</h1>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            管理本地 AI CLI 工具的代理配置
          </p>
        </div>
        {actionMessage && (
          <span className="text-sm px-3 py-1 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 rounded-full">
            {actionMessage}
          </span>
        )}
      </div>

      {/* Tool Cards */}
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-2 xl:grid-cols-4">
        {tools.map(tool => {
          const config = toolConfigs[tool.toolId];
          const isPending = !tool.installed;

          return (
            <div
              key={tool.toolId}
              className={`bg-white dark:bg-gray-800 rounded-lg border p-4 transition-all ${tool.takenOver
                ? 'border-green-300 dark:border-green-700 shadow-md shadow-green-500/10'
                : 'border-gray-200 dark:border-gray-700'
              } ${isPending ? 'opacity-60' : ''}`}
            >
              {/* Header */}
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <span className="text-2xl">{getToolIcon(tool.toolId)}</span>
                  <div>
                    <h3 className="font-semibold text-gray-900 dark:text-white text-sm">
                      {tool.displayName}
                    </h3>
                    <p className="text-xs text-gray-500 dark:text-gray-400 font-mono truncate max-w-[140px]" title={tool.configPath}>
                      {tool.configPath.split('/').slice(-2).join('/')}
                    </p>
                  </div>
                </div>

                {/* Status Badge */}
                <div className="flex items-center gap-1">
                  {tool.takenOver && (
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-medium bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-300 rounded-full">
                      <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
                      已接管
                    </span>
                  )}
                  {!tool.installed && (
                    <span className="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-medium bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300 rounded-full">
                      ⚠ 未安装
                    </span>
                  )}
                </div>
              </div>

              {/* Description */}
              <p className="text-xs text-gray-500 dark:text-gray-400 mb-3">
                {tool.description}
              </p>

              {/* Source Info */}
              <div className="mb-3 p-2 bg-gray-50 dark:bg-gray-700/50 rounded text-xs">
                <div className="flex items-center justify-between">
                  <span className="text-gray-500 dark:text-gray-400">模型源:</span>
                  <button
                    onClick={() => handleOpenSourcePicker(tool.toolId)}
                    className="text-indigo-600 dark:text-indigo-400 font-medium hover:underline"
                  >
                    {getSourceLabel(tool.toolId)}
                  </button>
                </div>
                {tool.takenOver && tool.proxyUrl && (
                  <div className="flex items-center justify-between mt-1">
                    <span className="text-gray-500 dark:text-gray-400">代理:</span>
                    <span className="text-gray-700 dark:text-gray-300 font-mono">{tool.proxyUrl}</span>
                  </div>
                )}
              </div>

              {/* Actions */}
              <div className="flex flex-wrap gap-2">
                {tool.installed ? (
                  <>
                    {tool.takenOver ? (
                      <>
                        <button
                          onClick={() => handleRestore(tool.toolId)}
                          disabled={loading}
                          className="flex-1 px-3 py-1.5 text-xs font-medium text-amber-700 dark:text-amber-300 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-md hover:bg-amber-100 dark:hover:bg-amber-900/40"
                        >
                          恢复原始配置
                        </button>
                        <button
                          onClick={() => handleOpenSourcePicker(tool.toolId)}
                          className="px-3 py-1.5 text-xs font-medium text-gray-600 dark:text-gray-400 border border-gray-200 dark:border-gray-600 rounded-md hover:bg-gray-50 dark:hover:bg-gray-700"
                        >
                          换模型
                        </button>
                      </>
                    ) : (
                      <button
                        onClick={() => handleTakeover(tool.toolId)}
                        disabled={loading || !proxyStatus.is_running}
                        className="flex-1 px-3 py-1.5 text-xs font-medium text-white bg-indigo-600 rounded-md hover:bg-indigo-700 disabled:opacity-50 disabled:cursor-not-allowed"
                        title={!proxyStatus.is_running ? '需要先启动代理' : undefined}
                      >
                        一键接管
                      </button>
                    )}
                    <div className="flex items-center gap-1">
                      <span className="text-xs text-gray-400">启用</span>
                      <button
                        onClick={() => handleToggleEnabled(tool.toolId, !(config?.enabled ?? true))}
                        className={`relative inline-flex h-5 w-8 items-center rounded-full transition-colors ${(config?.enabled ?? true) ? 'bg-indigo-600' : 'bg-gray-300 dark:bg-gray-600'}`}
                      >
                        <span className={`inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform ${(config?.enabled ?? true) ? 'translate-x-3.5' : 'translate-x-0.5'}`} />
                      </button>
                    </div>
                    <button
                      onClick={() => handleViewConfig(tool.toolId)}
                      className="px-2 py-1.5 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                      title="查看当前配置"
                    >
                      👁
                    </button>
                    <button
                      onClick={() => handleViewSnapshots(tool.toolId)}
                      className="px-2 py-1.5 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                      title="管理快照"
                    >
                      📸
                    </button>
                    <button
                      onClick={() => handleOpenConfigDir(tool.toolId)}
                      className="px-2 py-1.5 text-xs text-gray-400 hover:text-gray-600 dark:hover:text-gray-300"
                      title="打开配置目录"
                    >
                      📂
                    </button>
                  </>
                ) : (
                  <>
                    <span className="text-xs text-gray-400 italic flex-1">工具未安装</span>
                    {tool.homepage && (
                      <a
                        href={tool.homepage}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="px-2 py-1.5 text-xs font-medium text-indigo-600 dark:text-indigo-400 hover:underline"
                      >
                        安装指南 →
                      </a>
                    )}
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* Source Picker Modal */}
      {showSourcePicker && selectedTool && (
        <SourcePickerModal
          toolId={selectedTool}
          groups={groups}
          requiredEndpointType={tools.find(t => t.toolId === selectedTool)?.requiredEndpointType}
          currentSourceType={toolConfigs[selectedTool]?.sourceType || 'group'}
          currentSourceValue={toolConfigs[selectedTool]?.sourceValue || 'free'}
          onSelect={handleSourceSelect}
          onClose={() => setShowSourcePicker(false)}
        />
      )}

      {/* Config Viewer Modal */}
      {showConfigViewer && (
        <ConfigViewerModal
          title={configViewerTitle}
          files={configFiles}
          onClose={() => setShowConfigViewer(false)}
        />
      )}

      {/* Snapshot Manager Modal */}
      {showSnapshotManager && selectedTool && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && setShowSnapshotManager(false)}>
          <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-lg overflow-hidden">
            <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
              <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
                {tools.find(t => t.toolId === selectedTool)?.displayName || selectedTool} - 快照管理
              </h3>
              <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
                接管时自动保存的配置备份，可从中恢复
              </p>
            </div>
            <div className="px-6 py-4 max-h-[50vh] overflow-auto">
              {snapshots.length === 0 ? (
                <p className="text-sm text-gray-500 dark:text-gray-400 text-center py-8">
                  暂无快照。接管工具时会自动创建快照。
                </p>
              ) : (
                <div className="space-y-2">
                  {snapshots.map(s => (
                    <div key={s.id} className="p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
                      <div className="flex items-center justify-between">
                        <div>
                          <div className="text-sm font-medium text-gray-900 dark:text-white font-mono">
                            {s.createdAt}
                          </div>
                          <div className="text-xs text-gray-500 dark:text-gray-400">
                            {s.id} · {(s.sizeBytes / 1024).toFixed(1)} KB
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => handleViewSnapshotContent(s.id)}
                            className="px-2 py-1.5 text-xs text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300"
                            title="查看配置内容"
                          >
                            👁
                          </button>
                          <button
                            onClick={() => handleRestoreFromSnapshot(s.id)}
                            disabled={loading}
                            className="px-3 py-1.5 text-xs font-medium text-amber-700 dark:text-amber-300 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 rounded-md hover:bg-amber-100 dark:hover:bg-amber-900/40 disabled:opacity-50"
                          >
                            恢复此版本
                          </button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
            <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 flex justify-end">
              <button onClick={() => setShowSnapshotManager(false)} className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white">关闭</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function ConfigViewerModal({
  title,
  files,
  onClose,
}: {
  title: string;
  files: ConfigFileEntry[];
  onClose: () => void;
}) {
  const [activeTab, setActiveTab] = useState(0);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            {title} - 当前配置
          </h3>
          <button onClick={onClose} className="text-gray-400 hover:text-gray-600 dark:hover:text-gray-300">✕</button>
        </div>

        {files.length === 0 ? (
          <div className="flex-1 p-8 text-center text-gray-500 dark:text-gray-400">
            未找到配置文件
          </div>
        ) : (
          <>
            {/* File Tabs */}
            {files.length > 1 && (
              <div className="px-6 pt-3 flex gap-1 border-b border-gray-200 dark:border-gray-700">
                {files.map((f, i) => (
                  <button
                    key={f.filename}
                    onClick={() => setActiveTab(i)}
                    className={`px-3 py-1.5 text-xs font-mono rounded-t-md transition-colors ${
                      activeTab === i
                        ? 'bg-gray-100 dark:bg-gray-700 text-gray-900 dark:text-white border-b-2 border-indigo-500'
                        : 'text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                    }`}
                  >
                    {f.filename}
                  </button>
                ))}
              </div>
            )}

            {/* File Content */}
            <div className="flex-1 overflow-auto p-4">
              <pre className="text-xs font-mono text-gray-700 dark:text-gray-300 bg-gray-50 dark:bg-gray-900 rounded-lg p-4 whitespace-pre-wrap break-all">
                {files[activeTab]?.content || ''}
              </pre>
            </div>
          </>
        )}

        <div className="px-6 py-3 border-t border-gray-200 dark:border-gray-700 flex justify-end">
          <button onClick={onClose} className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white">关闭</button>
        </div>
      </div>
    </div>
  );
}

function SourcePickerModal({
  toolId,
  groups,
  requiredEndpointType,
  currentSourceType,
  currentSourceValue,
  onSelect,
  onClose,
}: {
  toolId: string;
  groups: Group[];
  requiredEndpointType?: string;
  currentSourceType: string;
  currentSourceValue: string;
  onSelect: (sourceType: string, sourceValue: string) => void;
  onClose: () => void;
}) {
  // Filter groups by required endpoint type
  const filteredGroups = useMemo(() => {
    if (!requiredEndpointType) return groups;
    return groups.filter(g => (g.endpoint_type || 'chat') === requiredEndpointType);
  }, [groups, requiredEndpointType]);

  const [sourceType, setSourceType] = useState<'group' | 'model'>(
    currentSourceType === 'model' ? 'model' : 'group',
  );
  const [selectedGroup, setSelectedGroup] = useState<string>(
    sourceType === 'group' ? currentSourceValue : (filteredGroups[0]?.name || ''),
  );
  const [selectedModel, setSelectedModel] = useState<string>(
    sourceType === 'model' ? currentSourceValue : '',
  );

  // Collect all models from filtered groups
  const allModels = useMemo(() => {
    const models = new Map<string, { groupName: string }>();
    for (const g of filteredGroups) {
      for (const m of g.models) {
        if (m.enabled && !models.has(m.model)) {
          models.set(m.model, { groupName: g.name });
        }
      }
    }
    return Array.from(models.entries()).map(([model, info]) => ({ model, ...info }));
  }, [filteredGroups]);

  const selectedGroupData = filteredGroups.find(g => g.name === selectedGroup);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-md overflow-hidden">
        <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white">
            选择模型源
          </h3>
          <p className="text-sm text-gray-500 dark:text-gray-400 mt-1">
            为 {toolId} 选择请求路由方式
          </p>
        </div>

        <div className="px-6 py-4 space-y-4">
          {/* Source Type Selection */}
          <div className="space-y-3">
            <label
              className={`flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-colors ${sourceType === 'group'
                ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20'
                : 'border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700/50'
              }`}
              onClick={() => setSourceType('group')}
            >
              <input
                type="radio"
                name="sourceType"
                checked={sourceType === 'group'}
                onChange={() => setSourceType('group')}
                className="mt-1 text-indigo-600 focus:ring-indigo-500"
              />
              <div>
                <div className="font-medium text-sm text-gray-900 dark:text-white">使用 Group（路由）</div>
                <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  通过 Group 的路由策略（优先级/权重/轮询）自动选择模型
                </div>
              </div>
            </label>

            <label
              className={`flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-colors ${sourceType === 'model'
                ? 'border-indigo-500 bg-indigo-50 dark:bg-indigo-900/20'
                : 'border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-700/50'
              }`}
              onClick={() => setSourceType('model')}
            >
              <input
                type="radio"
                name="sourceType"
                checked={sourceType === 'model'}
                onChange={() => setSourceType('model')}
                className="mt-1 text-indigo-600 focus:ring-indigo-500"
              />
              <div>
                <div className="font-medium text-sm text-gray-900 dark:text-white">指定具体模型（直连）</div>
                <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                  直接使用指定模型，不经过 Group 路由
                </div>
              </div>
            </label>
          </div>

          {/* Source Value Selection */}
          {sourceType === 'group' ? (
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">
                选择 Group
                {requiredEndpointType && (
                  <span className="ml-2 text-xs font-normal text-indigo-500 dark:text-indigo-400">
                    (仅 {requiredEndpointType} 端点)
                  </span>
                )}
              </label>
              <select
                value={selectedGroup}
                onChange={(e) => setSelectedGroup(e.target.value)}
                className="w-full rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border"
              >
                {filteredGroups.map(g => (
                  <option key={g.id} value={g.name}>{g.name} ({g.strategy}, {g.models.filter(m => m.enabled).length} 模型)</option>
                ))}
              </select>
              {filteredGroups.length === 0 && (
                <p className="mt-1 text-xs text-amber-600 dark:text-amber-400">
                  没有匹配 "{requiredEndpointType}" 端点的 Group，请先在 Groups 页面创建
                </p>
              )}
              {selectedGroupData && (
                <div className="mt-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded text-xs text-gray-500 dark:text-gray-400">
                  <div>策略: {selectedGroupData.strategy}</div>
                  <div>模型: {selectedGroupData.models.filter(m => m.enabled).map(m => m.model.split('/').pop()).join(', ')}</div>
                </div>
              )}
            </div>
          ) : (
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1">选择模型</label>
              <select
                value={selectedModel}
                onChange={(e) => setSelectedModel(e.target.value)}
                className="w-full rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border font-mono"
              >
                <option value="">-- 选择模型 --</option>
                {allModels.map(m => (
                  <option key={m.model} value={m.model}>{m.model.split('/').pop()} ({m.model})</option>
                ))}
              </select>
            </div>
          )}
        </div>

        <div className="px-6 py-4 border-t border-gray-200 dark:border-gray-700 flex justify-end gap-3">
          <button
            onClick={onClose}
            className="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:text-gray-900 dark:hover:text-white"
          >
            取消
          </button>
          <button
            onClick={() => onSelect(sourceType, sourceType === 'group' ? selectedGroup : selectedModel)}
            disabled={sourceType === 'model' && !selectedModel}
            className="px-4 py-2 text-sm font-medium text-white bg-indigo-600 hover:bg-indigo-700 rounded-lg disabled:opacity-50"
          >
            确认
          </button>
        </div>
      </div>
    </div>
  );
}

export default CliTools;
