//! CLI 工具安装检测

use crate::cli_config::traits::CliConfigurator;
use crate::cli_config::types::CliToolStatus;

/// 检测所有已安装的 CLI 工具
pub fn detect_installed(configurators: &[Box<dyn CliConfigurator>]) -> Vec<CliToolStatus> {
    configurators
        .iter()
        .map(|c| CliToolStatus {
            tool_id: c.tool_id().to_string(),
            display_name: c.display_name().to_string(),
            description: c.description().to_string(),
            installed: c.is_installed(),
            taken_over: false,
            proxy_url: None,
            source_type: None,
            source_value: None,
            taken_over_at: None,
            config_path: c.config_path().to_string_lossy().to_string(),
            homepage: c.homepage(),
            required_endpoint_type: c.required_endpoint_type().map(|s| s.to_string()),
        })
        .collect()
}
