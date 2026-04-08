//! 模型定价和成本计算模块
//!
//! 定价单位: 美元/百万token ($/1M tokens)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 模型定价条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingEntry {
    /// 输入 token 价格 ($/1M)
    pub input: f64,
    /// 输出 token 价格 ($/1M)
    pub output: f64,
    /// 缓存 token 价格 ($/1M)
    #[serde(default)]
    pub cached: f64,
    /// 推理 token 价格 ($/1M)
    #[serde(default)]
    pub reasoning: f64,
    /// 缓存创建价格 ($/1M)
    #[serde(default)]
    pub cache_creation: f64,
}

impl Default for PricingEntry {
    fn default() -> Self {
        Self {
            input: 1.0,
            output: 3.0,
            cached: 0.0,
            reasoning: 0.0,
            cache_creation: 0.0,
        }
    }
}

impl PricingEntry {
    /// 创建新的定价条目
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cached: input * 0.5,       // 默认缓存价格为输入价格的一半
            reasoning: output * 1.5,   // 默认推理价格为输出价格的1.5倍
            cache_creation: input,     // 默认缓存创建价格等于输入价格
        }
    }

    /// 创建带缓存价格的定价
    pub fn with_cached(mut self, cached: f64) -> Self {
        self.cached = cached;
        self
    }

    /// 创建带推理价格的定价
    pub fn with_reasoning(mut self, reasoning: f64) -> Self {
        self.reasoning = reasoning;
        self
    }
}

/// 按供应商分组的定价表
pub type PricingByProvider = HashMap<String, HashMap<String, PricingEntry>>;

/// 定价管理器
#[derive(Debug, Clone)]
pub struct PricingManager {
    /// 默认定价数据
    default_pricing: PricingByProvider,
    /// 用户自定义定价
    user_pricing: PricingByProvider,
}

impl Default for PricingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PricingManager {
    pub fn new() -> Self {
        Self {
            default_pricing: get_default_pricing(),
            user_pricing: HashMap::new(),
        }
    }

    /// 从 JSON 字符串加载用户定价
    pub fn load_user_pricing(&mut self, json: &str) -> Result<(), String> {
        if json.trim().is_empty() {
            return Ok(());
        }
        let pricing: PricingByProvider = serde_json::from_str(json)
            .map_err(|e| format!("解析用户定价失败: {}", e))?;
        self.user_pricing = pricing;
        Ok(())
    }

    /// 导出用户定价为 JSON 字符串
    pub fn export_user_pricing(&self) -> String {
        serde_json::to_string(&self.user_pricing).unwrap_or_else(|_| "{}".to_string())
    }

    /// 获取模型的定价信息
    /// 参数: provider_prefix - 供应商前缀 (如 "oai", "ds", "ant")
    /// 参数: model - 模型名称 (如 "gpt-4o", "deepseek-chat")
    pub fn get_pricing(&self, provider_prefix: &str, model: &str) -> Option<PricingEntry> {
        // 1. 先查用户自定义定价
        if let Some(provider_pricing) = self.user_pricing.get(provider_prefix) {
            if let Some(pricing) = provider_pricing.get(model) {
                return Some(pricing.clone());
            }
            // 尝试模糊匹配
            for (pattern, pricing) in provider_pricing {
                if Self::model_matches_pattern(model, pattern) {
                    return Some(pricing.clone());
                }
            }
        }

        // 2. 查默认定价
        if let Some(provider_pricing) = self.default_pricing.get(provider_prefix) {
            if let Some(pricing) = provider_pricing.get(model) {
                return Some(pricing.clone());
            }
            // 尝试模糊匹配
            for (pattern, pricing) in provider_pricing {
                if Self::model_matches_pattern(model, pattern) {
                    return Some(pricing.clone());
                }
            }
        }

        None
    }

    /// 模型名称匹配模式
    fn model_matches_pattern(model: &str, pattern: &str) -> bool {
        if pattern == "*" {
            return true;
        }
        if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len() - 1];
            return model.starts_with(prefix);
        }
        if pattern.starts_with('*') {
            let suffix = &pattern[1..];
            return model.ends_with(suffix);
        }
        model == pattern
    }

    /// 设置用户自定义定价
    pub fn set_user_pricing(&mut self, provider: String, model: String, pricing: PricingEntry) {
        self.user_pricing
            .entry(provider)
            .or_insert_with(HashMap::new)
            .insert(model, pricing);
    }

    /// 清除用户自定义定价
    pub fn clear_user_pricing(&mut self, provider: Option<&str>, model: Option<&str>) {
        match (provider, model) {
            (Some(p), Some(m)) => {
                if let Some(provider_pricing) = self.user_pricing.get_mut(p) {
                    provider_pricing.remove(m);
                }
            }
            (Some(p), None) => {
                self.user_pricing.remove(p);
            }
            (None, None) => {
                self.user_pricing.clear();
            }
            (None, Some(_)) => {}
        }
    }

    /// 获取所有定价（合并默认和用户定价）
    pub fn get_all_pricing(&self) -> PricingByProvider {
        let mut result = self.default_pricing.clone();

        for (provider, models) in &self.user_pricing {
            let entry = result.entry(provider.clone()).or_insert_with(HashMap::new);
            for (model, pricing) in models {
                entry.insert(model.clone(), pricing.clone());
            }
        }

        result
    }
}

/// 计算请求成本
///
/// # 参数
/// - prompt_tokens: 输入 token 数量
/// - completion_tokens: 输出 token 数量
/// - cached_tokens: 缓存 token 数量 (可选)
/// - reasoning_tokens: 推理 token 数量 (可选)
/// - pricing: 定价信息
///
/// # 返回
/// 成本（美元）
pub fn calculate_cost(
    prompt_tokens: i32,
    completion_tokens: i32,
    cached_tokens: Option<i32>,
    reasoning_tokens: Option<i32>,
    pricing: &PricingEntry,
) -> f64 {
    let prompt_tokens = prompt_tokens as f64;
    let completion_tokens = completion_tokens as f64;
    let cached_tokens = cached_tokens.unwrap_or(0) as f64;
    let reasoning_tokens = reasoning_tokens.unwrap_or(0) as f64;

    // 成本 = (非缓存输入 × 输入价格) + (缓存 × 缓存价格) + (输出 × 输出价格) + (推理 × 推理价格)
    // 价格单位是 $/1M tokens，所以除以 1_000_000
    let non_cached_input = prompt_tokens - cached_tokens;

    let mut cost = 0.0;
    cost += non_cached_input * pricing.input / 1_000_000.0;
    cost += cached_tokens * pricing.cached / 1_000_000.0;
    cost += completion_tokens * pricing.output / 1_000_000.0;
    cost += reasoning_tokens * pricing.reasoning / 1_000_000.0;

    cost
}

/// 规范化模型名称（去除供应商前缀）
/// 
/// 例如：
/// - "ds/deepseek-chat" + prefix="ds" -> "deepseek-chat"
/// - "Pro/deepseek-ai/DeepSeek-V3.2" + prefix=None -> "Pro/deepseek-ai/DeepSeek-V3.2" (保持不变)
/// 
/// 注意：如果模型名本身包含 "/"（如 SiliconFlow 的模型），需要提供 provider_prefix
/// 才能正确识别并去掉 provider prefix，否则会保留原样
pub fn normalize_model_name(model: &str) -> &str {
    // 查找第一个 "/"，如果存在则去掉前缀
    // 这是一个简化版本，适用于大多数情况
    if let Some(pos) = model.find('/') {
        &model[pos + 1..]
    } else {
        model
    }
}

/// 规范化模型名称（带 provider prefix 版本）
/// 
/// 这才是正确的用法：明确知道 provider prefix，只去掉真正的 prefix
/// - "sf/Pro/deepseek-ai/DeepSeek-V3.2" + prefix="sf" -> "Pro/deepseek-ai/DeepSeek-V3.2"
/// - "ds/deepseek-chat" + prefix="ds" -> "deepseek-chat"
pub fn normalize_model_name_with_prefix<'a>(model: &'a str, provider_prefix: Option<&str>) -> &'a str {
    match provider_prefix {
        Some(prefix) if model.starts_with(prefix) && model.len() > prefix.len() && model.chars().nth(prefix.len()) == Some('/') => {
            // 匹配到 provider prefix，去掉它
            &model[prefix.len() + 1..]
        }
        _ => {
            // 没有匹配到 prefix，保持原样不变
            // 注意：不要尝试智能去掉任何部分，因为模型名本身可能包含 "/"
            model
        }
    }
}

/// 获取默认定价数据
fn get_default_pricing() -> PricingByProvider {
    let mut pricing: PricingByProvider = HashMap::new();

    // ============ OpenAI ============
    let mut openai: HashMap<String, PricingEntry> = HashMap::new();
    openai.insert("gpt-4o".into(), PricingEntry::new(2.5, 10.0).with_cached(1.25).with_reasoning(15.0));
    openai.insert("gpt-4o-mini".into(), PricingEntry::new(0.15, 0.6).with_cached(0.075).with_reasoning(0.9));
    openai.insert("gpt-4-turbo".into(), PricingEntry::new(10.0, 30.0).with_cached(5.0).with_reasoning(45.0));
    openai.insert("gpt-4".into(), PricingEntry::new(30.0, 60.0).with_cached(15.0).with_reasoning(90.0));
    openai.insert("gpt-3.5-turbo".into(), PricingEntry::new(0.5, 1.5).with_cached(0.25).with_reasoning(2.25));
    openai.insert("o1".into(), PricingEntry::new(15.0, 60.0).with_cached(7.5).with_reasoning(90.0));
    openai.insert("o1-mini".into(), PricingEntry::new(3.0, 12.0).with_cached(1.5).with_reasoning(18.0));
    openai.insert("o1-preview".into(), PricingEntry::new(15.0, 60.0).with_cached(7.5).with_reasoning(90.0));
    pricing.insert("oai".into(), openai);

    // ============ Anthropic Claude ============
    let mut anthropic: HashMap<String, PricingEntry> = HashMap::new();
    // Claude 4 系列
    anthropic.insert("claude-opus-4-6".into(), PricingEntry::new(5.0, 25.0).with_cached(2.5).with_reasoning(37.5));
    anthropic.insert("claude-sonnet-4-6".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(22.5));
    anthropic.insert("claude-opus-4".into(), PricingEntry::new(15.0, 75.0).with_cached(7.5).with_reasoning(112.5));
    anthropic.insert("claude-sonnet-4".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(15.0));
    // Claude 3.5 系列
    anthropic.insert("claude-3-5-sonnet".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(15.0));
    anthropic.insert("claude-3-5-sonnet-20241022".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(15.0));
    anthropic.insert("claude-3-5-haiku".into(), PricingEntry::new(0.8, 4.0).with_cached(0.4).with_reasoning(6.0));
    anthropic.insert("claude-3-haiku-20240307".into(), PricingEntry::new(0.25, 1.25).with_cached(0.125).with_reasoning(1.875));
    anthropic.insert("claude-3-opus-20240229".into(), PricingEntry::new(15.0, 75.0).with_cached(7.5).with_reasoning(112.5));
    pricing.insert("ant".into(), anthropic);

    // ============ DeepSeek ============
    let mut deepseek: HashMap<String, PricingEntry> = HashMap::new();
    deepseek.insert("deepseek-chat".into(), PricingEntry::new(0.28, 0.42).with_cached(0.014).with_reasoning(0.42));
    deepseek.insert("deepseek-v3".into(), PricingEntry::new(0.28, 0.42).with_cached(0.014).with_reasoning(0.42));
    deepseek.insert("deepseek-reasoner".into(), PricingEntry::new(0.55, 2.19).with_cached(0.14).with_reasoning(2.19));
    deepseek.insert("deepseek-r1".into(), PricingEntry::new(0.55, 2.19).with_cached(0.14).with_reasoning(2.19));
    deepseek.insert("deepseek-coder".into(), PricingEntry::new(0.28, 0.42).with_cached(0.014).with_reasoning(0.42));
    pricing.insert("ds".into(), deepseek);

    // ============ Google Gemini ============
    let mut gemini: HashMap<String, PricingEntry> = HashMap::new();
    gemini.insert("gemini-2.5-pro".into(), PricingEntry::new(2.0, 12.0).with_cached(0.25).with_reasoning(18.0));
    gemini.insert("gemini-2.5-flash".into(), PricingEntry::new(0.3, 2.5).with_cached(0.03).with_reasoning(3.75));
    gemini.insert("gemini-2.5-flash-lite".into(), PricingEntry::new(0.1, 0.4).with_cached(0.025).with_reasoning(0.6));
    gemini.insert("gemini-2.0-flash".into(), PricingEntry::new(0.1, 0.4).with_cached(0.025).with_reasoning(0.6));
    gemini.insert("gemini-1.5-pro".into(), PricingEntry::new(1.25, 5.0).with_cached(0.15625).with_reasoning(7.5));
    gemini.insert("gemini-1.5-flash".into(), PricingEntry::new(0.075, 0.3).with_cached(0.01875).with_reasoning(0.45));
    pricing.insert("gc".into(), gemini);

    // ============ Moonshot (Kimi) ============
    let mut moonshot: HashMap<String, PricingEntry> = HashMap::new();
    moonshot.insert("moonshot-v1-8k".into(), PricingEntry::new(0.12, 0.12).with_cached(0.06).with_reasoning(0.18));
    moonshot.insert("moonshot-v1-32k".into(), PricingEntry::new(0.24, 0.24).with_cached(0.12).with_reasoning(0.36));
    moonshot.insert("moonshot-v1-128k".into(), PricingEntry::new(0.6, 0.6).with_cached(0.3).with_reasoning(0.9));
    pricing.insert("ms".into(), moonshot);

    // ============ 智谱 AI (GLM) ============
    let mut zhipu: HashMap<String, PricingEntry> = HashMap::new();
    zhipu.insert("glm-4".into(), PricingEntry::new(0.1, 0.1).with_cached(0.05).with_reasoning(0.15));
    zhipu.insert("glm-4-flash".into(), PricingEntry::new(0.001, 0.001).with_cached(0.0005).with_reasoning(0.0015));
    zhipu.insert("glm-4-plus".into(), PricingEntry::new(0.05, 0.05).with_cached(0.025).with_reasoning(0.075));
    zhipu.insert("glm-4-air".into(), PricingEntry::new(0.001, 0.001).with_cached(0.0005).with_reasoning(0.0015));
    pricing.insert("zp".into(), zhipu);

    // ============ 通义千问 ============
    let mut qwen: HashMap<String, PricingEntry> = HashMap::new();
    qwen.insert("qwen-turbo".into(), PricingEntry::new(0.002, 0.006).with_cached(0.001).with_reasoning(0.009));
    qwen.insert("qwen-plus".into(), PricingEntry::new(0.004, 0.012).with_cached(0.002).with_reasoning(0.018));
    qwen.insert("qwen-max".into(), PricingEntry::new(0.02, 0.06).with_cached(0.01).with_reasoning(0.09));
    qwen.insert("qwen-long".into(), PricingEntry::new(0.0005, 0.002).with_cached(0.00025).with_reasoning(0.003));
    pricing.insert("qw".into(), qwen);

    // ============ 百度千帆 ============
    let mut qianfan: HashMap<String, PricingEntry> = HashMap::new();
    qianfan.insert("ernie-4.0-8k".into(), PricingEntry::new(0.12, 0.12).with_cached(0.06).with_reasoning(0.18));
    qianfan.insert("ernie-3.5-8k".into(), PricingEntry::new(0.012, 0.012).with_cached(0.006).with_reasoning(0.018));
    qianfan.insert("ernie-speed-8k".into(), PricingEntry::new(0.001, 0.001).with_cached(0.0005).with_reasoning(0.0015));
    pricing.insert("qf".into(), qianfan);

    // ============ SiliconFlow ============
    let mut siliconflow: HashMap<String, PricingEntry> = HashMap::new();
    siliconflow.insert("deepseek-ai/DeepSeek-V3".into(), PricingEntry::new(0.14, 0.28).with_cached(0.07).with_reasoning(0.42));
    siliconflow.insert("deepseek-ai/DeepSeek-R1".into(), PricingEntry::new(0.55, 2.19).with_cached(0.14).with_reasoning(2.19));
    siliconflow.insert("Qwen/Qwen2.5-72B-Instruct".into(), PricingEntry::new(0.6, 0.6).with_cached(0.3).with_reasoning(0.9));
    siliconflow.insert("Qwen/Qwen2.5-32B-Instruct".into(), PricingEntry::new(0.2, 0.2).with_cached(0.1).with_reasoning(0.3));
    pricing.insert("sf".into(), siliconflow);

    // ============ OpenRouter ============
    let mut openrouter: HashMap<String, PricingEntry> = HashMap::new();
    openrouter.insert("openai/gpt-4o".into(), PricingEntry::new(2.5, 10.0).with_cached(1.25).with_reasoning(15.0));
    openrouter.insert("anthropic/claude-sonnet-4".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(15.0));
    openrouter.insert("google/gemini-2.5-pro".into(), PricingEntry::new(2.0, 12.0).with_cached(0.25).with_reasoning(18.0));
    openrouter.insert("deepseek/deepseek-chat".into(), PricingEntry::new(0.28, 0.42).with_cached(0.014).with_reasoning(0.42));
    pricing.insert("or".into(), openrouter);

    // ============ Groq ============
    let mut groq: HashMap<String, PricingEntry> = HashMap::new();
    groq.insert("llama-3.3-70b-versatile".into(), PricingEntry::new(0.59, 0.79).with_cached(0.295).with_reasoning(1.185));
    groq.insert("llama-3.1-8b-instant".into(), PricingEntry::new(0.02, 0.02).with_cached(0.01).with_reasoning(0.03));
    groq.insert("mixtral-8x7b-32768".into(), PricingEntry::new(0.27, 0.27).with_cached(0.135).with_reasoning(0.405));
    pricing.insert("gq".into(), groq);

    // ============ Mistral AI ============
    let mut mistral: HashMap<String, PricingEntry> = HashMap::new();
    mistral.insert("mistral-large-latest".into(), PricingEntry::new(2.0, 6.0).with_cached(1.0).with_reasoning(9.0));
    mistral.insert("codestral-latest".into(), PricingEntry::new(0.3, 0.9).with_cached(0.15).with_reasoning(1.35));
    mistral.insert("mistral-small-latest".into(), PricingEntry::new(0.1, 0.3).with_cached(0.05).with_reasoning(0.45));
    pricing.insert("mr".into(), mistral);

    // ============ API2D ============
    let mut api2d: HashMap<String, PricingEntry> = HashMap::new();
    api2d.insert("gpt-4o".into(), PricingEntry::new(2.5, 10.0).with_cached(1.25).with_reasoning(15.0));
    api2d.insert("gpt-4".into(), PricingEntry::new(30.0, 60.0).with_cached(15.0).with_reasoning(90.0));
    api2d.insert("claude-3-5-sonnet".into(), PricingEntry::new(3.0, 15.0).with_cached(1.5).with_reasoning(15.0));
    pricing.insert("a2d".into(), api2d);

    pricing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_cost() {
        let pricing = PricingEntry::new(2.5, 10.0);
        let cost = calculate_cost(1000, 500, None, None, &pricing);
        // (1000 * 2.5 + 500 * 10.0) / 1_000_000 = 0.0075
        assert!((cost - 0.0075).abs() < 0.0001);
    }

    #[test]
    fn test_calculate_cost_with_cached() {
        let pricing = PricingEntry::new(2.5, 10.0).with_cached(1.25);
        let cost = calculate_cost(1000, 500, Some(500), None, &pricing);
        // ((1000-500) * 2.5 + 500 * 1.25 + 500 * 10.0) / 1_000_000 = 0.006875
        assert!((cost - 0.006875).abs() < 0.0001);
    }

    #[test]
    fn test_get_pricing() {
        let manager = PricingManager::new();

        // Test OpenAI pricing
        let pricing = manager.get_pricing("oai", "gpt-4o");
        assert!(pricing.is_some());
        let p = pricing.unwrap();
        assert!((p.input - 2.5).abs() < 0.01);
        assert!((p.output - 10.0).abs() < 0.01);

        // Test DeepSeek pricing
        let pricing = manager.get_pricing("ds", "deepseek-chat");
        assert!(pricing.is_some());
        let p = pricing.unwrap();
        assert!((p.input - 0.28).abs() < 0.01);
    }

    #[test]
    fn test_normalize_model_name() {
        assert_eq!(normalize_model_name("ds/deepseek-chat"), "deepseek-chat");
        assert_eq!(normalize_model_name("deepseek-chat"), "deepseek-chat");
        assert_eq!(normalize_model_name("oai/gpt-4o"), "gpt-4o");
    }
}
