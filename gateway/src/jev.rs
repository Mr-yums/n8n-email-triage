//! Étage 2 : Jev, décision typée bon marché (catégorie + probabilités, puis urgence).
//!
//! API REST : `POST {JEV_BASE_URL}/api/v1/decisions/run` (doc publique iajev.com/docs/api),
//! en-têtes `Authorization: Bearer`, `Idempotency-Key`. 1 crédit par tranche de 1 000 tokens
//! d'entrée commencée. En cas d'erreur Jev, on escalade vers Claude au lieu d'échouer.

use crate::config::{Config, Mode};
use crate::demo;
use crate::model::{Category, Email, JevOutcome, Urgency};
use crate::text::estimate_tokens;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;

const QUESTION_CATEGORY: &str =
    "Dans quelle catégorie une PME doit-elle classer cet email entrant ?";
const QUESTION_URGENCY: &str =
    "Quel est le niveau d'urgence de cet email pour la PME qui le reçoit ?";
const URGENCY_CHOICES: [(&str, Urgency); 3] = [
    (
        "haute : à traiter aujourd'hui (blocage, délai court, risque)",
        Urgency::Haute,
    ),
    ("normale : à traiter dans la semaine", Urgency::Normale),
    (
        "basse : information, publicité, aucune action",
        Urgency::Basse,
    ),
];

#[derive(Debug, Deserialize)]
struct JevResponse {
    decision: serde_json::Value,
    confidence: Option<f64>,
    #[serde(default)]
    results: Vec<JevResult>,
    billing: Option<JevBilling>,
}

#[derive(Debug, Deserialize)]
struct JevResult {
    value: serde_json::Value,
    probability: f64,
}

#[derive(Debug, Deserialize)]
struct JevBilling {
    credits: u32,
}

/// État transmis à Jev : `message` ≤ 8 000 caractères (requête ≤ 16 000 octets), autres champs ≤ 300.
fn jev_state(email: &Email) -> serde_json::Value {
    let message: String = format!("Objet : {}\n\n{}", email.subject, email.body)
        .chars()
        .take(6_000)
        .collect();
    let attachments: String = email.attachments.join(", ").chars().take(300).collect();
    json!({
        "message": message,
        "sender_domain": email.sender_domain().chars().take(300).collect::<String>(),
        "has_attachments": !email.attachments.is_empty(),
        "attachment_names": attachments,
        "bulk_mail": email.list_unsubscribe,
    })
}

/// Clé d'idempotence stable : même email + même question = même décision, facturée une fois.
fn idempotency_key(email: &Email, suffix: &str) -> String {
    // FNV-1a 64 bits : suffisant pour une clé de déduplication, sans dépendance.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in email.message_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("email-triage-{hash:016x}-{suffix}")
}

pub struct JevClient {
    http: reqwest::Client,
}

impl JevClient {
    pub fn new(http: reqwest::Client) -> JevClient {
        JevClient { http }
    }

    pub async fn decide(&self, cfg: &Config, email: &Email) -> JevOutcome {
        let result = match cfg.mode {
            Mode::Demo => Ok(simulate(cfg, email)),
            Mode::Live => self.live(cfg, email).await,
        };
        match result {
            Ok(out) => out,
            Err(err) => JevOutcome {
                category: Category::Autre,
                confidence: 0.0,
                probabilities: BTreeMap::new(),
                urgency: None,
                credits: 0,
                cost_eur: 0.0,
                escalate: true,
                escalate_reason: Some(format!("Jev indisponible : {err:#}")),
                simulated: false,
                error: Some(format!("{err:#}")),
            },
        }
    }

    async fn run(
        &self,
        cfg: &Config,
        email: &Email,
        question: &str,
        choices: &[&str],
        key: &str,
    ) -> Result<JevResponse> {
        let body = json!({
            "model": cfg.jev_model,
            "state": jev_state(email),
            "decision": { "type": "choice", "question": question, "choices": choices },
        });
        let api_key = cfg
            .jev_api_key
            .as_deref()
            .context("JEV_API_KEY manquante")?;
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/decisions/run",
                cfg.jev_base_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .header("Idempotency-Key", idempotency_key(email, key))
            .json(&body)
            .send()
            .await
            .context("appel Jev")?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!(
                "HTTP {status} : {}",
                text.chars().take(300).collect::<String>()
            );
        }
        resp.json::<JevResponse>()
            .await
            .context("réponse Jev illisible")
    }

    async fn live(&self, cfg: &Config, email: &Email) -> Result<JevOutcome> {
        let choices: Vec<&str> = Category::ALL.iter().map(|c| c.jev_choice()).collect();
        let resp = self
            .run(cfg, email, QUESTION_CATEGORY, &choices, "category")
            .await?;
        let decision = resp
            .decision
            .as_str()
            .context("décision Jev non textuelle")?;
        let category = Category::from_jev_choice(decision)
            .with_context(|| format!("choix Jev inconnu : {decision}"))?;
        let probabilities: BTreeMap<Category, f64> = resp
            .results
            .iter()
            .filter_map(|r| {
                r.value
                    .as_str()
                    .and_then(Category::from_jev_choice)
                    .map(|c| (c, r.probability))
            })
            .collect();
        let confidence = resp
            .confidence
            .or_else(|| probabilities.get(&category).copied())
            .unwrap_or(0.0);
        let mut credits = resp.billing.map(|b| b.credits).unwrap_or(1);
        let (escalate, escalate_reason) = escalation(cfg, confidence);

        // L'urgence n'est demandée à Jev que si Jev garde la main (sinon Claude la fixe).
        let urgency = if escalate {
            None
        } else {
            let labels: Vec<&str> = URGENCY_CHOICES.iter().map(|(l, _)| *l).collect();
            let u = self
                .run(cfg, email, QUESTION_URGENCY, &labels, "urgency")
                .await?;
            credits += u.billing.map(|b| b.credits).unwrap_or(1);
            let label = u.decision.as_str().unwrap_or_default();
            URGENCY_CHOICES
                .iter()
                .find(|(l, _)| *l == label)
                .map(|(_, u)| *u)
        };

        Ok(JevOutcome {
            category,
            confidence,
            probabilities,
            urgency,
            credits,
            cost_eur: f64::from(credits) * cfg.jev_credit_eur,
            escalate,
            escalate_reason,
            simulated: false,
            error: None,
        })
    }
}

/// Seuil métier : voie directe Jev seulement si confiance STRICTEMENT supérieure au seuil.
pub fn escalation(cfg: &Config, confidence: f64) -> (bool, Option<String>) {
    if confidence > cfg.jev_threshold {
        (false, None)
    } else {
        (
            true,
            Some(format!(
                "confiance Jev {confidence:.2} ≤ seuil {:.2}",
                cfg.jev_threshold
            )),
        )
    }
}

/// Crédits estimés d'un appel : 1 par tranche de 1 000 tokens d'entrée commencée (minimum 1).
fn estimated_credits(email: &Email) -> u32 {
    let tokens = estimate_tokens(&jev_state(email).to_string()) + 120; // + question et choix
    u32::try_from(tokens.div_ceil(1000).max(1)).unwrap_or(u32::MAX)
}

fn simulate(cfg: &Config, email: &Email) -> JevOutcome {
    let sim = demo::jev_category(email);
    let (escalate, escalate_reason) = escalation(cfg, sim.confidence);
    let per_call = estimated_credits(email);
    let (urgency, credits) = if escalate {
        (None, per_call)
    } else {
        (Some(demo::jev_urgency(email, sim.category)), per_call * 2)
    };
    JevOutcome {
        category: sim.category,
        confidence: sim.confidence,
        probabilities: sim.probabilities,
        urgency,
        credits,
        cost_eur: f64::from(credits) * cfg.jev_credit_eur,
        escalate,
        escalate_reason,
        simulated: true,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_fit_jev_limits() {
        for c in Category::ALL {
            assert!(c.jev_choice().chars().count() <= 80, "{}", c.jev_choice());
            assert_eq!(Category::from_jev_choice(c.jev_choice()), Some(c));
        }
        for (label, _) in URGENCY_CHOICES {
            assert!(label.chars().count() <= 80);
        }
    }

    #[test]
    fn threshold_is_strict() {
        let cfg = Config::demo_for_tests(".");
        assert!(
            escalation(&cfg, 0.85).0,
            "0,85 exactement doit partir chez Claude"
        );
        assert!(!escalation(&cfg, 0.851).0);
    }
}
