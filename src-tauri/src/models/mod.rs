//! UniRoute 核心数据模型
//!
//! 架构：Provider（供应商）只管认证，ProviderEndpoint（端点）管协议和模型
//! 请求模型名 → Group → 端点列表 → 选择端点 → 通过 Provider 认证 → 发送请求

mod entities;
mod requests;
mod responses;
mod templates;

pub use entities::*;
pub use requests::*;
pub use responses::*;
pub use templates::*;

// ============ Tests ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_type_can_convert() {
        assert!(EndpointType::Chat.can_convert_to(&EndpointType::Responses));
        assert!(EndpointType::Messages.can_convert_to(&EndpointType::Chat));
        assert!(!EndpointType::Chat.can_convert_to(&EndpointType::Embeddings));
    }

    #[test]
    fn test_provider_auth_value() {
        let mut p = Provider::new("Test".into(), "test".into());
        p.api_key = Some("sk-123".into());
        assert_eq!(p.get_auth_value(), Some("Bearer sk-123".to_string()));

        p.auth_header = "x-api-key".into();
        p.auth_prefix = None;
        assert_eq!(p.get_auth_value(), Some("sk-123".to_string()));
    }

    #[test]
    fn test_quota_status() {
        let limit = QuotaLimit { daily_limit: Some(10.0), monthly_limit: None, warning_threshold: 0.8 };
        let status = QuotaStatus::compute(8.0, 0.0, &limit);
        assert!(status.is_warning);
        assert!(!status.is_exceeded);
    }
}
