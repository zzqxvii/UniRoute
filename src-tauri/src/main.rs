// Prevents additional console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::Arc;
use tauri::Manager;
use uniroute_lib::{commands::*, state::AppState};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let state = Arc::new(AppState::new());
            app.manage(state.clone());

            // 检查是否需要自动启动代理
            let settings = state.get_settings();
            if settings.auto_start_proxy {
                let port = settings.proxy_port;
                tracing::info!("自动启动代理服务器，端口: {}", port);

                let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
                let state_for_proxy = Arc::clone(&state);
                let state_handle = Arc::clone(&state);

                // 使用 tauri 的异步运行时
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = uniroute_lib::proxy::start_proxy_server(port, state_for_proxy, shutdown_rx).await {
                        tracing::error!("代理服务器错误: {}", e);
                    }
                });

                let handle = uniroute_lib::state::ProxyServerHandle { port, shutdown_tx };
                *state_handle.proxy_server.write() = Some(handle);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Proxy
            start_proxy,
            stop_proxy,
            get_proxy_status,
            // Providers
            get_providers,
            get_builtin_templates,
            create_provider,
            update_provider,
            delete_provider,
            test_provider,
            // Groups
            get_groups,
            get_group,
            create_group,
            update_group,
            delete_group,
            add_model_to_group,
            remove_model_from_group,
            // Model Mappings
            get_model_mappings,
            create_model_mapping,
            delete_model_mapping,
            // Settings
            get_settings,
            update_settings,
            // Data
            export_data,
            import_data,
            get_db_path,
            // Request Logs
            get_request_logs,
            get_request_stats,
            clear_request_logs,
            // Cost Statistics
            get_cost_by_model,
            get_cost_by_provider,
            get_daily_cost,
            get_pricing,
            set_pricing,
            delete_pricing,
            reset_pricing,
            // Quota
            get_quota_limit,
            update_quota_limit,
            get_quota_status,
            // OAuth
            start_oauth_flow,
            poll_oauth_token,
            refresh_oauth_token,
            cancel_oauth_flow,
            check_oauth_status,
            // Benchmark
            benchmark_provider,
            // Diagnostic
            diagnose_route,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
