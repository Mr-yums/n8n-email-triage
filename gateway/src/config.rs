//! Configuration lue depuis l'environnement. Aucune clé n'a de valeur par défaut.

use anyhow::{Result, bail};
use std::env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Jev et Claude simulés de façon déterministe, aucun appel réseau sortant, aucune clé.
    Demo,
    /// Appels réels à Jev et à l'API Claude (clés requises).
    Live,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub listen_addr: String,
    pub database_url: Option<String>,
    pub rules_file: String,
    pub samples_file: String,
    pub jev_api_key: Option<String>,
    pub jev_base_url: String,
    pub jev_model: String,
    /// La voie directe Jev exige une confiance STRICTEMENT supérieure à ce seuil.
    pub jev_threshold: f64,
    pub jev_credit_eur: f64,
    pub anthropic_api_key: Option<String>,
    pub claude_model: String,
    pub claude_effort: String,
    pub claude_price_in_usd_mtok: f64,
    pub claude_price_out_usd_mtok: f64,
    pub eur_per_usd: f64,
}

fn var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn var_f64(name: &str, default: f64) -> Result<f64> {
    match var(name) {
        None => Ok(default),
        Some(v) => v
            .parse::<f64>()
            .map_err(|_| anyhow::anyhow!("{name} doit être un nombre (reçu « {v} »)")),
    }
}

/// Prix publics Anthropic (USD par million de tokens) des modèles proposés.
pub fn default_claude_prices(model: &str) -> (f64, f64) {
    match model {
        m if m.starts_with("claude-opus-5-5") => (4.0, 20.0),
        m if m.starts_with("claude-opus") => (5.0, 25.0),
        m if m.starts_with("claude-haiku") => (1.0, 5.0),
        _ => (2.0, 10.0), // claude-sonnet-5
    }
}

impl Config {
    pub fn from_env() -> Result<Config> {
        let mode = match var("TRIAGE_MODE").as_deref().unwrap_or("demo") {
            "demo" => Mode::Demo,
            "live" => Mode::Live,
            other => bail!("TRIAGE_MODE doit valoir « demo » ou « live » (reçu « {other} »)"),
        };
        let claude_model = var("CLAUDE_MODEL").unwrap_or_else(|| "claude-sonnet-5".into());
        let (p_in, p_out) = default_claude_prices(&claude_model);
        let cfg = Config {
            mode,
            listen_addr: var("LISTEN_ADDR").unwrap_or_else(|| "0.0.0.0:8080".into()),
            database_url: var("DATABASE_URL"),
            rules_file: var("RULES_FILE").unwrap_or_else(|| "../config/rules.json".into()),
            samples_file: var("SAMPLES_FILE").unwrap_or_else(|| "../samples/emails.json".into()),
            jev_api_key: var("JEV_API_KEY"),
            jev_base_url: var("JEV_BASE_URL").unwrap_or_else(|| "https://iajev.com".into()),
            jev_model: var("JEV_MODEL").unwrap_or_else(|| "jev".into()),
            jev_threshold: var_f64("JEV_CONFIDENCE_THRESHOLD", 0.85)?,
            // Offre Starter : 16,90 € / 15 000 crédits.
            jev_credit_eur: var_f64("JEV_CREDIT_EUR", 16.90 / 15_000.0)?,
            anthropic_api_key: var("ANTHROPIC_API_KEY"),
            claude_effort: var("CLAUDE_EFFORT").unwrap_or_else(|| "low".into()),
            claude_price_in_usd_mtok: var_f64("CLAUDE_PRICE_IN_USD_PER_MTOK", p_in)?,
            claude_price_out_usd_mtok: var_f64("CLAUDE_PRICE_OUT_USD_PER_MTOK", p_out)?,
            eur_per_usd: var_f64("EUR_PER_USD", 0.86)?,
            claude_model,
        };
        if cfg.mode == Mode::Live {
            if cfg.jev_api_key.is_none() {
                bail!("TRIAGE_MODE=live : JEV_API_KEY manquante");
            }
            if cfg.anthropic_api_key.is_none() {
                bail!("TRIAGE_MODE=live : ANTHROPIC_API_KEY manquante");
            }
        }
        if !(0.0..=1.0).contains(&cfg.jev_threshold) {
            bail!("JEV_CONFIDENCE_THRESHOLD doit être entre 0 et 1");
        }
        Ok(cfg)
    }

    /// Configuration démo pour les tests (aucune variable d'environnement lue).
    pub fn demo_for_tests(root: &str) -> Config {
        let (p_in, p_out) = default_claude_prices("claude-sonnet-5");
        Config {
            mode: Mode::Demo,
            listen_addr: "127.0.0.1:0".into(),
            database_url: None,
            rules_file: format!("{root}/config/rules.json"),
            samples_file: format!("{root}/samples/emails.json"),
            jev_api_key: None,
            jev_base_url: "https://iajev.com".into(),
            jev_model: "jev".into(),
            jev_threshold: 0.85,
            jev_credit_eur: 16.90 / 15_000.0,
            anthropic_api_key: None,
            claude_model: "claude-sonnet-5".into(),
            claude_effort: "low".into(),
            claude_price_in_usd_mtok: p_in,
            claude_price_out_usd_mtok: p_out,
            eur_per_usd: 0.86,
        }
    }
}
