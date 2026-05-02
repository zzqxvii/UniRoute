import { createContext, useContext, useState, useEffect, useCallback, ReactNode } from 'react';
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

interface ProxyContextValue {
  proxyStatus: ProxyStatus;
  settings: AppSettings | null;
  proxyLoading: boolean;
  refreshProxyStatus: () => Promise<void>;
  toggleProxy: () => Promise<void>;
}

const ProxyContext = createContext<ProxyContextValue | null>(null);

export function ProxyProvider({ children }: { children: ReactNode }) {
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus>({ is_running: false, port: null });
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [proxyLoading, setProxyLoading] = useState(false);

  const refreshProxyStatus = useCallback(async () => {
    try {
      const [status, settingsResult] = await Promise.all([
        invoke<ProxyStatus>('get_proxy_status'),
        invoke<AppSettings>('get_settings'),
      ]);
      setProxyStatus(status);
      setSettings(settingsResult);
    } catch (error) {
      console.error('Failed to load proxy status:', error);
    }
  }, []);

  const toggleProxy = useCallback(async () => {
    setProxyLoading(true);
    try {
      if (proxyStatus.is_running) {
        await invoke('stop_proxy');
      } else {
        const port = settings?.proxy_port || 8080;
        await invoke('start_proxy', { port });
      }
      setTimeout(refreshProxyStatus, 500);
    } catch (error) {
      console.error('Proxy toggle failed:', error);
    } finally {
      setProxyLoading(false);
    }
  }, [proxyStatus.is_running, settings?.proxy_port, refreshProxyStatus]);

  useEffect(() => {
    refreshProxyStatus();
    const interval = setInterval(refreshProxyStatus, 5000);
    return () => clearInterval(interval);
  }, [refreshProxyStatus]);

  return (
    <ProxyContext.Provider value={{ proxyStatus, settings, proxyLoading, refreshProxyStatus, toggleProxy }}>
      {children}
    </ProxyContext.Provider>
  );
}

export function useProxy() {
  const ctx = useContext(ProxyContext);
  if (!ctx) throw new Error('useProxy must be used within ProxyProvider');
  return ctx;
}
