//! Passerelle IA du tri d'emails : règles déterministes → Jev → Claude.

pub mod claude;
pub mod config;
pub mod demo;
pub mod jev;
pub mod model;
pub mod pipeline;
pub mod rules;
pub mod server;
pub mod store;
pub mod text;

use anyhow::{Context, Result};
use model::Email;

/// Charge le jeu d'exemples (`{"emails": [...]}`).
pub fn load_samples(path: &str) -> Result<Vec<Email>> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("lecture de {path}"))?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    serde_json::from_value(v.get("emails").cloned().context("clé « emails » absente")?)
        .context("format des emails")
}
