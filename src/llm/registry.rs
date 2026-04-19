use std::collections::HashMap;

use super::LlmProvider;

/// 一个最小的“多 provider 路由器”：按名字选择 provider。
pub struct ProviderRegistry {
    providers: HashMap<String, Box<dyn LlmProvider>>,
    default: String,
}

impl ProviderRegistry {
    pub fn new(default: impl Into<String>) -> Self {
        Self {
            providers: HashMap::new(),
            default: default.into(),
        }
    }

    pub fn register(mut self, name: impl Into<String>, provider: Box<dyn LlmProvider>) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    pub fn get(&self, name: Option<&str>) -> Option<&dyn LlmProvider> {
        let key = name.unwrap_or(self.default.as_str());
        self.providers.get(key).map(|p| p.as_ref())
    }
}
