//! 路由策略模块
//!
//! 根据 Group 的策略选择模型排序。

use crate::models::{Group, GroupModel, GroupStrategy};
use rand::Rng;

use super::Router;

/// 根据策略选择模型
pub fn select_model_by_strategy(router: &Router, group: &Group) -> Vec<GroupModel> {
    // 过滤掉禁用的模型
    let models: Vec<_> = group.models.iter().filter(|m| m.enabled).cloned().collect();

    if models.is_empty() {
        return Vec::new();
    }

    match group.strategy {
        GroupStrategy::Priority => {
            // 按优先级排序
            let mut sorted = models;
            sorted.sort_by_key(|m| m.priority);
            sorted
        }
        GroupStrategy::RoundRobin => {
            // 轮询：选择下一个模型
            let index = router.state.group_strategy_state
                .next_round_robin_index(&group.id, models.len());

            // 返回以选定模型为首的列表（后续用于故障转移）
            let mut result = Vec::with_capacity(models.len());
            for i in 0..models.len() {
                result.push(models[(index + i) % models.len()].clone());
            }
            result
        }
        GroupStrategy::Random => {
            // 随机选择一个模型
            let mut rng = rand::thread_rng();
            let index = rng.gen_range(0..models.len());

            // 返回以随机模型为首的列表
            let mut result = Vec::with_capacity(models.len());
            for i in 0..models.len() {
                result.push(models[(index + i) % models.len()].clone());
            }
            result
        }
        GroupStrategy::Weighted => {
            // 根据权重随机选择
            let total_weight: u32 = models.iter().map(|m| m.weight).sum();
            if total_weight == 0 {
                return models;
            }

            let mut rng = rand::thread_rng();
            let mut random = rng.gen_range(0..total_weight);

            let mut selected_index = 0;
            for (i, m) in models.iter().enumerate() {
                if random < m.weight {
                    selected_index = i;
                    break;
                }
                random -= m.weight;
            }

            // 返回以选定模型为首的列表
            let mut result = Vec::with_capacity(models.len());
            for i in 0..models.len() {
                result.push(models[(selected_index + i) % models.len()].clone());
            }
            result
        }
        GroupStrategy::LeastUsed => {
            let mut models_with_usage: Vec<_> = models
                .iter()
                .map(|m| {
                    let usage = router.state.group_strategy_state
                        .get_model_usage(&group.id, &m.model);
                    (m.clone(), usage)
                })
                .collect();

            models_with_usage.sort_by_key(|(_, usage)| *usage);

            models_with_usage.into_iter().map(|(m, _)| m).collect()
        }
        GroupStrategy::CostOptimized => {
            let pricing_manager = &router.state.pricing_manager;
            let pm = pricing_manager.read();

            let mut models_with_cost: Vec<_> = models
                .iter()
                .map(|m| {
                    let cost = estimate_model_cost(router, &pm, &m.model);
                    (m.clone(), cost)
                })
                .collect();

            models_with_cost.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

            models_with_cost.into_iter().map(|(m, _)| m).collect()
        }
    }
}

/// 估算模型成本（每百万 token 的输入+输出平均价格）
fn estimate_model_cost(router: &Router, pm: &crate::pricing::PricingManager, model_key: &str) -> f64 {
    // 尝试从 provider 的模型列表中获取定价
    let providers = router.state.get_providers();
    for provider in &providers {
        if let Some(pricing) = provider.get_model_pricing(model_key) {
            return (pricing.input + pricing.output) / 2.0;
        }
        // 也尝试从全局定价中获取
        if let Some(pricing) = pm.get_pricing(&provider.prefix, model_key) {
            return (pricing.input + pricing.output) / 2.0;
        }
    }
    // 默认返回一个高值，让有定价的模型优先
    100.0
}
