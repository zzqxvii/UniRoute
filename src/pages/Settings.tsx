import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useTranslation } from 'react-i18next';

interface QuotaLimit {
  daily_limit: number | null;
  monthly_limit: number | null;
  warning_threshold: number;
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

interface AppSettings {
  proxy_port: number;
  auto_start_proxy: boolean;
  log_level: string;
  quota: QuotaLimit;
}

function Settings() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<AppSettings>({
    proxy_port: 8080,
    auto_start_proxy: false,
    log_level: 'info',
    quota: { daily_limit: null, monthly_limit: null, warning_threshold: 0.8 },
  });
  const [quotaStatus, setQuotaStatus] = useState<QuotaStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    loadSettings();
    loadQuotaStatus();
  }, []);

  const loadSettings = async () => {
    try {
      const result = await invoke<AppSettings>('get_settings');
      setSettings(result);
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
  };

  const loadQuotaStatus = async () => {
    try {
      const result = await invoke<QuotaStatus>('get_quota_status');
      setQuotaStatus(result);
    } catch (error) {
      console.error('Failed to load quota status:', error);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await invoke('update_settings', { settings });
      // 同时更新配额设置
      await invoke('update_quota_limit', {
        dailyLimit: settings.quota.daily_limit,
        monthlyLimit: settings.quota.monthly_limit,
        warningThreshold: settings.quota.warning_threshold,
      });
      loadQuotaStatus();
      setSaved(true);
      setTimeout(() => setSaved(false), 2000);
    } catch (error) {
      alert(t('providers.loadFailed') + ': ' + error);
    } finally {
      setSaving(false);
    }
  };

  const handleReset = () => {
    setSettings({
      proxy_port: 8080,
      auto_start_proxy: false,
      log_level: 'info',
      quota: { daily_limit: null, monthly_limit: null, warning_threshold: 0.8 },
    });
  };

  // 配额进度条颜色
  const getProgressColor = (percent: number | null) => {
    if (percent === null) return 'bg-gray-300';
    if (percent >= 100) return 'bg-red-500';
    if (percent >= 80) return 'bg-amber-500';
    return 'bg-green-500';
  };

  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-semibold text-gray-900 dark:text-white">
          {t('settings.title')}
        </h1>
        {saved && (
          <span className="text-sm text-green-600 dark:text-green-400">
            {t('settings.saved')}
          </span>
        )}
      </div>

      <div className="space-y-6">
        {/* Proxy Settings */}
        <div className="bg-white dark:bg-gray-800 shadow rounded-lg">
          <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="text-lg font-medium text-gray-900 dark:text-white">
              {t('settings.proxySettings')}
            </h2>
          </div>
          <div className="px-6 py-4 space-y-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {t('settings.proxyPort')}
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                {t('settings.proxyPortDesc')}
              </p>
              <input
                type="number"
                value={settings.proxy_port}
                onChange={(e) =>
                  setSettings({ ...settings, proxy_port: parseInt(e.target.value) || 8080 })
                }
                min={1024}
                max={65535}
                className="mt-2 block w-32 rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border"
              />
            </div>

            <div className="flex items-center">
              <input
                type="checkbox"
                id="auto_start"
                checked={settings.auto_start_proxy}
                onChange={(e) =>
                  setSettings({ ...settings, auto_start_proxy: e.target.checked })
                }
                className="h-4 w-4 rounded border-gray-300 text-indigo-600 focus:ring-indigo-500"
              />
              <label htmlFor="auto_start" className="ml-2 block text-sm text-gray-900 dark:text-white">
                {t('settings.autoStartProxy')}
              </label>
            </div>
          </div>
        </div>

        {/* Quota Settings */}
        <div className="bg-white dark:bg-gray-800 shadow rounded-lg">
          <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="text-lg font-medium text-gray-900 dark:text-white">
              成本配额
            </h2>
          </div>
          <div className="px-6 py-4 space-y-4">
            {/* 当前使用状态 */}
            {quotaStatus && (
              <div className="p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
                <div className="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">当前使用</div>
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">今日</div>
                    <div className="text-lg font-bold text-gray-900 dark:text-white">
                      ${quotaStatus.daily_used.toFixed(4)}
                    </div>
                    {quotaStatus.daily_percent !== null && (
                      <div className="mt-1">
                        <div className="h-2 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                          <div
                            className={`h-full ${getProgressColor(quotaStatus.daily_percent)}`}
                            style={{ width: `${Math.min(quotaStatus.daily_percent, 100)}%` }}
                          />
                        </div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                          {quotaStatus.daily_percent.toFixed(1)}%
                        </div>
                      </div>
                    )}
                  </div>
                  <div>
                    <div className="text-xs text-gray-500 dark:text-gray-400">本月</div>
                    <div className="text-lg font-bold text-gray-900 dark:text-white">
                      ${quotaStatus.monthly_used.toFixed(4)}
                    </div>
                    {quotaStatus.monthly_percent !== null && (
                      <div className="mt-1">
                        <div className="h-2 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                          <div
                            className={`h-full ${getProgressColor(quotaStatus.monthly_percent)}`}
                            style={{ width: `${Math.min(quotaStatus.monthly_percent, 100)}%` }}
                          />
                        </div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
                          {quotaStatus.monthly_percent.toFixed(1)}%
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              </div>
            )}

            {/* 每日限额 */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                每日限额 ($)
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                设置为 0 表示不限制
              </p>
              <input
                type="number"
                step="0.01"
                value={settings.quota.daily_limit || ''}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    quota: { ...settings.quota, daily_limit: parseFloat(e.target.value) || null },
                  })
                }
                placeholder="无限制"
                className="mt-2 block w-32 rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border"
              />
            </div>

            {/* 每月限额 */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                每月限额 ($)
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                设置为 0 表示不限制
              </p>
              <input
                type="number"
                step="0.01"
                value={settings.quota.monthly_limit || ''}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    quota: { ...settings.quota, monthly_limit: parseFloat(e.target.value) || null },
                  })
                }
                placeholder="无限制"
                className="mt-2 block w-32 rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border"
              />
            </div>

            {/* 警告阈值 */}
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                警告阈值 ({(settings.quota.warning_threshold * 100).toFixed(0)}%)
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                使用量超过此阈值时提醒
              </p>
              <input
                type="range"
                min="0.5"
                max="1"
                step="0.05"
                value={settings.quota.warning_threshold}
                onChange={(e) =>
                  setSettings({
                    ...settings,
                    quota: { ...settings.quota, warning_threshold: parseFloat(e.target.value) },
                  })
                }
                className="mt-2 block w-48"
              />
            </div>
          </div>
        </div>

        {/* Logging Settings */}
        <div className="bg-white dark:bg-gray-800 shadow rounded-lg">
          <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="text-lg font-medium text-gray-900 dark:text-white">
              {t('settings.loggingSettings')}
            </h2>
          </div>
          <div className="px-6 py-4 space-y-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 dark:text-gray-300">
                {t('settings.logLevel')}
              </label>
              <p className="text-xs text-gray-500 dark:text-gray-400 mt-1">
                {t('settings.logLevelDesc')}
              </p>
              <select
                value={settings.log_level}
                onChange={(e) => setSettings({ ...settings, log_level: e.target.value })}
                className="mt-2 block w-40 rounded-md border-gray-300 dark:border-gray-600 dark:bg-gray-700 dark:text-white shadow-sm focus:border-indigo-500 focus:ring-indigo-500 sm:text-sm px-3 py-2 border"
              >
                <option value="debug">{t('settings.logLevels.debug')}</option>
                <option value="info">{t('settings.logLevels.info')}</option>
                <option value="warn">{t('settings.logLevels.warn')}</option>
                <option value="error">{t('settings.logLevels.error')}</option>
              </select>
            </div>
          </div>
        </div>

        {/* Data Management */}
        <div className="bg-white dark:bg-gray-800 shadow rounded-lg">
          <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="text-lg font-medium text-gray-900 dark:text-white">
              {t('settings.dataManagement')}
            </h2>
          </div>
          <div className="px-6 py-4 space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium text-gray-900 dark:text-white">
                  {t('settings.resetAllSettings')}
                </p>
                <p className="text-xs text-gray-500 dark:text-gray-400">
                  {t('settings.resetAllSettingsDesc')}
                </p>
              </div>
              <button
                onClick={handleReset}
                className="px-3 py-1.5 text-sm text-gray-600 border border-gray-600 rounded-md hover:bg-gray-50 dark:hover:bg-gray-700"
              >
                {t('settings.reset')}
              </button>
            </div>
          </div>
        </div>

        {/* About */}
        <div className="bg-white dark:bg-gray-800 shadow rounded-lg">
          <div className="px-6 py-4 border-b border-gray-200 dark:border-gray-700">
            <h2 className="text-lg font-medium text-gray-900 dark:text-white">
              {t('settings.about')}
            </h2>
          </div>
          <div className="px-6 py-4">
            <div className="flex items-center space-x-4">
              <div className="w-12 h-12 bg-indigo-600 rounded-lg flex items-center justify-center">
                <span className="text-white text-xl font-bold">OR</span>
              </div>
              <div>
                <h3 className="text-lg font-medium text-gray-900 dark:text-white">
                  {t('app.name')}
                </h3>
                <p className="text-sm text-gray-500 dark:text-gray-400">
                  {t('app.title')}
                </p>
              </div>
            </div>
            <div className="mt-4 grid grid-cols-2 gap-4 text-sm">
              <div>
                <span className="text-gray-500 dark:text-gray-400">{t('settings.version')}:</span>
                <span className="ml-2 text-gray-900 dark:text-white">0.1.0</span>
              </div>
              <div>
                <span className="text-gray-500 dark:text-gray-400">{t('settings.framework')}:</span>
                <span className="ml-2 text-gray-900 dark:text-white">Tauri 2 + React</span>
              </div>
              <div>
                <span className="text-gray-500 dark:text-gray-400">{t('settings.backend')}:</span>
                <span className="ml-2 text-gray-900 dark:text-white">Rust + Axum</span>
              </div>
            </div>
          </div>
        </div>

        {/* Save Button */}
        <div className="flex justify-end space-x-3">
          <button
            onClick={handleReset}
            className="px-4 py-2 text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 rounded-md hover:bg-gray-200 dark:hover:bg-gray-600"
          >
            {t('settings.resetSettings')}
          </button>
          <button
            onClick={handleSave}
            disabled={saving}
            className="px-4 py-2 bg-indigo-600 text-white rounded-md hover:bg-indigo-700 disabled:opacity-50"
          >
            {saving ? t('providers.saving') : t('settings.saveSettings')}
          </button>
        </div>
      </div>
    </div>
  );
}

export default Settings;
