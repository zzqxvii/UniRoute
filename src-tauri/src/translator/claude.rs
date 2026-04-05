use super::{Translator, TranslatorError};
use crate::models::ApiFormat;
use crate::models::ChatRequest;
use async_trait::async_trait;
use serde_json::Value;

/// Claude format translator
pub struct ClaudeTranslator {}

impl ClaudeTranslator {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Translator for ClaudeTranslator {
    fn translate_request(
        &self,
        source: &ApiFormat,
        target: &ApiFormat,
        _request: &ChatRequest,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::Claude, ApiFormat::Claude) => {
                Ok(serde_json::to_value(_request)?)
            }
            _ => Err(TranslatorError::UnsupportedConversion(
                format!("{:?}", source),
                format!("{:?}", target),
            )),
        }
    }

    fn translate_response(
        &self,
        source: &ApiFormat,
        target: &ApiFormat,
        response: &Value,
    ) -> Result<Value, TranslatorError> {
        match (source, target) {
            (ApiFormat::Claude, ApiFormat::Claude) => Ok(response.clone()),
            _ => Err(TranslatorError::UnsupportedConversion(
                format!("{:?}", source),
                format!("{:?}", target),
            )),
        }
    }
}

impl Default for ClaudeTranslator {
    fn default() -> Self {
        Self::new()
    }
}
