//! Étage 1 : règles déterministes. Gratuites, explicables, appliquées avant toute IA.
//!
//! Ordre : (1) contenu sensible → jamais tranché par règle ; (2) expéditeur connu ;
//! (3) facture d'un fournisseur connu avec pièce jointe ; (4) envoi de masse (List-Unsubscribe).

use crate::model::{Category, Email, RuleOutcome, Urgency};
use crate::text::{contains_phrase, normalize};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
pub struct RulesConfig {
    #[serde(default)]
    pub known_senders: BTreeMap<String, Category>,
    #[serde(default)]
    pub invoice_suppliers: Vec<String>,
    #[serde(default)]
    pub invoice_attachment_keywords: Vec<String>,
    pub bulk_mail_category: Option<Category>,
    #[serde(default)]
    pub sensitive_patterns: BTreeMap<String, Vec<String>>,
}

impl RulesConfig {
    pub fn load(path: &str) -> Result<RulesConfig> {
        let raw = std::fs::read_to_string(path).with_context(|| format!("lecture de {path}"))?;
        let mut cfg: RulesConfig =
            serde_json::from_str(&raw).with_context(|| format!("analyse de {path}"))?;
        cfg.known_senders = cfg
            .known_senders
            .into_iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .collect();
        cfg.invoice_suppliers = cfg
            .invoice_suppliers
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        Ok(cfg)
    }

    /// Motifs sensibles trouvés dans l'objet, le corps ou les noms de pièces jointes.
    pub fn sensitive_reasons(&self, email: &Email) -> Vec<String> {
        let haystack = normalize(&format!(
            "{} {} {}",
            email.subject,
            email.body,
            email.attachments.join(" ")
        ));
        self.sensitive_patterns
            .iter()
            .filter(|(_, patterns)| patterns.iter().any(|p| contains_phrase(&haystack, p)))
            .map(|(family, _)| family.clone())
            .collect()
    }

    pub fn apply(&self, email: &Email) -> RuleOutcome {
        let sender = email.from_address.trim().to_lowercase();
        let reasons = self.sensitive_reasons(email);
        if !reasons.is_empty() {
            return RuleOutcome {
                sensitive: true,
                reason: format!(
                    "contenu sensible ({}) : arbitrage Claude + validation humaine",
                    reasons.join(", ")
                ),
                sensitive_reasons: reasons,
                ..RuleOutcome::default()
            };
        }

        if let Some(&category) = self.known_senders.get(&sender) {
            return decided(
                "expediteur_connu",
                category,
                Urgency::Normale,
                format!("expéditeur connu {sender}"),
            );
        }

        if self.invoice_suppliers.contains(&sender) && self.has_invoice_attachment(email) {
            return decided(
                "facture_fournisseur",
                Category::Facture,
                Urgency::Normale,
                format!("fournisseur connu {sender} + pièce jointe de facture"),
            );
        }

        if email.list_unsubscribe
            && let Some(category) = self.bulk_mail_category
        {
            return decided(
                "envoi_de_masse",
                category,
                Urgency::Basse,
                "envoi de masse (en-tête List-Unsubscribe)".into(),
            );
        }

        RuleOutcome {
            reason: "aucune règle applicable : décision Jev".into(),
            ..RuleOutcome::default()
        }
    }

    fn has_invoice_attachment(&self, email: &Email) -> bool {
        email.attachments.iter().any(|name| {
            let lower = name.to_lowercase();
            let n = normalize(&lower.replace(['_', '-'], " "));
            lower.ends_with(".pdf")
                && self
                    .invoice_attachment_keywords
                    .iter()
                    .any(|k| contains_phrase(&n, k))
        })
    }
}

fn decided(rule_id: &str, category: Category, urgency: Urgency, reason: String) -> RuleOutcome {
    RuleOutcome {
        decided: true,
        rule_id: Some(rule_id.into()),
        category: Some(category),
        urgency: Some(urgency),
        reason,
        ..RuleOutcome::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> RulesConfig {
        RulesConfig::load(concat!(env!("CARGO_MANIFEST_DIR"), "/../config/rules.json")).unwrap()
    }

    fn email(from: &str, subject: &str, body: &str, attachments: &[&str]) -> Email {
        Email {
            message_id: "<t@test.example>".into(),
            id: None,
            from_name: String::new(),
            from_address: from.into(),
            subject: subject.into(),
            date: None,
            body: body.into(),
            attachments: attachments.iter().map(|s| s.to_string()).collect(),
            list_unsubscribe: false,
            expected: None,
        }
    }

    #[test]
    fn known_supplier_with_invoice_pdf_is_decided() {
        let out = rules().apply(&email(
            "factures@papeterie-durand.example",
            "Facture",
            "Ci-joint.",
            &["Facture_F1.pdf"],
        ));
        assert!(out.decided);
        assert_eq!(out.category, Some(Category::Facture));
    }

    #[test]
    fn known_supplier_without_invoice_is_not_decided() {
        let out = rules().apply(&email(
            "factures@papeterie-durand.example",
            "Question",
            "Bonjour",
            &[],
        ));
        assert!(!out.decided);
    }

    #[test]
    fn sensitive_content_beats_known_sender() {
        let out = rules().apply(&email(
            "factures@papeterie-durand.example",
            "Facture",
            "Merci d'utiliser notre nouveau RIB.",
            &["Facture_F1.pdf"],
        ));
        assert!(!out.decided);
        assert!(out.sensitive);
    }
}
