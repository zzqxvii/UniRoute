import React from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { ProxyProvider } from './components/ProxyContext';
import App from './App';
import './index.css';
import './i18n';

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <BrowserRouter>
      <ProxyProvider>
        <App />
      </ProxyProvider>
    </BrowserRouter>
  </React.StrictMode>
);
