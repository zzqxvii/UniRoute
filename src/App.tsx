import { lazy, Suspense } from 'react';
import { Routes, Route, Link, useLocation } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useProxy } from './components/ProxyContext';
import { DashboardSkeleton } from './components/Skeleton';

const Dashboard = lazy(() => import('./pages/Dashboard'));
const Providers = lazy(() => import('./pages/Providers'));
const Groups = lazy(() => import('./pages/Groups'));
const Logs = lazy(() => import('./pages/Logs'));
const Settings = lazy(() => import('./pages/Settings'));
const CliTools = lazy(() => import('./pages/CliTools'));

const PageLoader = () => (
  <div className="px-4 py-6 sm:px-0">
    <DashboardSkeleton />
  </div>
);

function App() {
  const location = useLocation();
  const { i18n } = useTranslation();
  const { proxyStatus, proxyLoading, toggleProxy } = useProxy();

  const navItems = [
    { path: '/', label: '仪表盘' },
    { path: '/providers', label: '供应商' },
    { path: '/groups', label: '组合' },
    { path: '/cli-tools', label: 'CLI工具' },
    { path: '/logs', label: '日志' },
    { path: '/settings', label: '设置' },
  ];

  const toggleLanguage = () => {
    const newLang = i18n.language === 'zh' ? 'en' : 'zh';
    i18n.changeLanguage(newLang);
  };

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-900">
      {/* Header */}
      <header className="bg-white dark:bg-gray-800 shadow">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex">
              <div className="flex-shrink-0 flex items-center">
                <h1 className="text-xl font-bold text-indigo-600 dark:text-indigo-400">
                  UniRoute
                </h1>
              </div>
              <nav className="ml-6 flex space-x-8">
                {navItems.map((item) => (
                  <Link
                    key={item.path}
                    to={item.path}
                    className={`inline-flex items-center px-1 pt-1 border-b-2 text-sm font-medium ${
                      location.pathname === item.path
                        ? 'border-indigo-500 text-gray-900 dark:text-white'
                        : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                    }`}
                  >
                    {item.label}
                  </Link>
                ))}
              </nav>
            </div>
            <div className="flex items-center gap-3">
              {/* Proxy Toggle Button */}
              <button
                onClick={toggleProxy}
                disabled={proxyLoading}
                className={`flex items-center gap-2 px-4 py-2 rounded-lg font-medium text-sm transition-all duration-200 ${
                  proxyStatus.is_running
                    ? 'bg-green-500 text-white hover:bg-green-600 shadow-md shadow-green-500/30'
                    : 'bg-gray-200 dark:bg-gray-700 text-gray-700 dark:text-gray-300 hover:bg-gray-300 dark:hover:bg-gray-600'
                } disabled:opacity-50`}
              >
                <span className={`w-2 h-2 rounded-full ${proxyStatus.is_running ? 'bg-white animate-pulse' : 'bg-gray-400'}`}></span>
                {proxyLoading ? '...' : proxyStatus.is_running ? `运行中 :${proxyStatus.port}` : '启动代理'}
              </button>

              {/* Language Toggle */}
              <button
                onClick={toggleLanguage}
                className="px-3 py-1.5 text-sm bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-md hover:bg-gray-200 dark:hover:bg-gray-600"
              >
                {i18n.language === 'zh' ? 'EN' : '中文'}
              </button>
            </div>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
        <Suspense fallback={<PageLoader />}>
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/providers" element={<Providers />} />
            <Route path="/groups" element={<Groups />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/cli-tools" element={<CliTools />} />
          </Routes>
        </Suspense>
      </main>
    </div>
  );
}

export default App;
