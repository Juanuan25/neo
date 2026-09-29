use serde::Serialize;

/// Enabled / disabled services in `settings.toml` at one revision.
#[derive(Serialize, Clone, Debug)]
pub struct ServicesAtRev {
    pub rev: String,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}
