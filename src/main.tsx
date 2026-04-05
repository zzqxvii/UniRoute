import React, { useEffect } from 'react';
import ReactDOM from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import App from './App';
import './index.css';
import './i18n';

function HideLoadingScreen() {
  useEffect(() => {
    const hide = () => {
      const el = document.getElementById('loading-screen');
      if (el) {
        el.classList.add('hidden');
        setTimeout(() => el.remove(), 300);
      }
    };
    hide();
  }, []);
  return null;
}

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <BrowserRouter>
      <HideLoadingScreen />
      <App />
    </BrowserRouter>
  </React.StrictMode>
);
