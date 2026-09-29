//! Process-local cache of option schemas for helper script resolution.
//! Populated on pane load; helper runs use the cache on hit (no nix evaluator mutex).

use std::collections::HashMap;

use super::types::{AppConfig, OptionPaneContext, OptionSchema};

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct SchemaCacheKey {
    pub is_core: bool,
    pub name: String,
}

#[derive(Default, Debug)]
pub struct SchemaCache {
    entries: HashMap<SchemaCacheKey, Vec<OptionSchema>>,
}

impl SchemaCache {
    pub fn get(&self, is_core: bool, name: &str) -> Option<Vec<OptionSchema>> {
        let key = SchemaCacheKey {
            is_core,
            name: name.to_string(),
        };
        self.entries.get(&key).cloned()
    }

    pub fn put(&mut self, is_core: bool, name: &str, options: Vec<OptionSchema>) {
        let key = SchemaCacheKey {
            is_core,
            name: name.to_string(),
        };
        self.entries.insert(key, options);
    }

    pub fn invalidate_all(&mut self) {
        self.entries.clear();
    }
}

/// Extract a service (or core section) pane from the flake. Not cached.
pub async fn extract_pane(config: &AppConfig, is_core: bool, name: &str) -> OptionPaneContext {
    let mut ev = config.evaluator.lock().await;
    if is_core {
        ev.extract_neo_section(name).await
    } else {
        ev.extract_service_options(name).await
    }
}

/// Extract a pane for rendering and remember its options for helper runs.
pub async fn load_pane(config: &AppConfig, is_core: bool, name: &str) -> OptionPaneContext {
    let pane = extract_pane(config, is_core, name).await;
    config
        .schema_cache
        .write()
        .await
        .put(is_core, name, pane.options.clone());
    pane
}

/// Option schemas for helper resolution: cache hit, else extract (cached on success).
pub async fn load_options(
    config: &AppConfig,
    is_core: bool,
    name: &str,
) -> Result<Vec<OptionSchema>, String> {
    if let Some(opts) = config.schema_cache.read().await.get(is_core, name) {
        return Ok(opts);
    }
    let pane = extract_pane(config, is_core, name).await;
    if let Some(err) = pane.eval_error.error {
        return Err(err);
    }
    config
        .schema_cache
        .write()
        .await
        .put(is_core, name, pane.options.clone());
    Ok(pane.options)
}
