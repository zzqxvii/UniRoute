use crate::models::{Combo, ComboConnection, ComboStrategy};
use std::sync::Arc;
use tokio::sync::RwLock;

/// ComboManager handles combo-based routing
pub struct ComboManager {
    combos: Arc<RwLock<Vec<Combo>>>,
}

impl ComboManager {
    pub fn new() -> Self {
        Self {
            combos: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Select the next connection based on strategy
    pub async fn select_connection(
        &self,
        combo_id: &str,
        model: &str,
    ) -> Option<ComboConnection> {
        let combos = self.combos.read().await;
        let combo = combos.iter().find(|c| c.id == combo_id)?;

        if combo.connections.is_empty() {
            return None;
        }

        // Filter connections by model
        let matching: Vec<_> = combo
            .connections
            .iter()
            .filter(|c| c.model == model || c.model == "*")
            .collect();

        if matching.is_empty() {
            return None;
        }

        match combo.strategy {
            ComboStrategy::RoundRobin => {
                // Simple round-robin: return first available
                matching.first().cloned().cloned()
            }
            ComboStrategy::Priority => {
                // Return highest priority (lowest number)
                matching
                    .iter()
                    .min_by_key(|c| c.priority)
                    .cloned()
                    .cloned()
            }
            ComboStrategy::Random => {
                // Random selection
                use rand::seq::SliceRandom;
                matching.choose(&mut rand::thread_rng()).cloned().cloned()
            }
            ComboStrategy::LeastLatency => {
                // TODO: Track latency and select lowest
                matching.first().cloned().cloned()
            }
            ComboStrategy::CostOptimized => {
                // TODO: Track costs and select cheapest
                matching.first().cloned().cloned()
            }
        }
    }

    pub async fn add_combo(&self, combo: Combo) {
        self.combos.write().await.push(combo);
    }

    pub async fn get_combos(&self) -> Vec<Combo> {
        self.combos.read().await.clone()
    }
}

impl Default for ComboManager {
    fn default() -> Self {
        Self::new()
    }
}
