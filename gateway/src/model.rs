//! Types partagés : email, résultats de chaque étage, enregistrement final.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Facture,
    Devis,
    Support,
    Rh,
    Administratif,
    Spam,
    Autre,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Facture,
        Category::Devis,
        Category::Support,
        Category::Rh,
        Category::Administratif,
        Category::Spam,
        Category::Autre,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::Facture => "facture",
            Category::Devis => "devis",
            Category::Support => "support",
            Category::Rh => "rh",
            Category::Administratif => "administratif",
            Category::Spam => "spam",
            Category::Autre => "autre",
        }
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            Category::Facture => "Facture",
            Category::Devis => "Devis / demande commerciale",
            Category::Support => "Support client",
            Category::Rh => "RH / candidature",
            Category::Administratif => "Administratif",
            Category::Spam => "Spam / prospection",
            Category::Autre => "Autre",
        }
    }

    /// Libellé envoyé à Jev comme choix (≤ 80 caractères, explicite pour le modèle).
    pub fn jev_choice(self) -> &'static str {
        match self {
            Category::Facture => {
                "facture : facture, avoir, relance de paiement, coordonnées de paiement"
            }
            Category::Devis => {
                "devis : demande de prix, de devis ou proposition commerciale d'un client"
            }
            Category::Support => {
                "support : client existant, commande, livraison, panne, réclamation"
            }
            Category::Rh => "rh : candidature, CV, stage, salarié, absence",
            Category::Administratif => {
                "administratif : organismes, banque, assurance, juridique, comptable"
            }
            Category::Spam => "spam : prospection non sollicitée, publicité, newsletter, arnaque",
            Category::Autre => "autre : rien de ce qui précède",
        }
    }

    pub fn from_jev_choice(choice: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| {
            choice == c.jev_choice() || choice.split(':').next().map(str::trim) == Some(c.id())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Haute,
    Normale,
    Basse,
}

impl Urgency {
    pub fn id(self) -> &'static str {
        match self {
            Urgency::Haute => "haute",
            Urgency::Normale => "normale",
            Urgency::Basse => "basse",
        }
    }
}

/// Qui a pris la décision finale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Regle,
    Jev,
    Claude,
}

impl Route {
    pub fn id(self) -> &'static str {
        match self {
            Route::Regle => "regle",
            Route::Jev => "jev",
            Route::Claude => "claude",
        }
    }
}

/// Résultat attendu (jeu d'exemples uniquement, sert aux tests et à la colonne « conforme »).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Expected {
    pub category: Category,
    pub route: Route,
    pub urgent: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Email {
    pub message_id: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub from_name: String,
    pub from_address: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub date: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub list_unsubscribe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Expected>,
}

impl Email {
    pub fn sender_domain(&self) -> String {
        self.from_address
            .rsplit('@')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuleOutcome {
    /// Une règle déterministe a tranché : aucune IA appelée.
    pub decided: bool,
    /// Message déjà présent dans la table de résultats : on ne le retraite pas.
    pub already_processed: bool,
    /// Contenu sensible (banque, juridique, santé, fraude…) : Claude + humain obligatoires.
    pub sensitive: bool,
    pub sensitive_reasons: Vec<String>,
    pub rule_id: Option<String>,
    pub category: Option<Category>,
    pub urgency: Option<Urgency>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JevOutcome {
    pub category: Category,
    pub confidence: f64,
    pub probabilities: BTreeMap<Category, f64>,
    pub urgency: Option<Urgency>,
    pub credits: u32,
    pub cost_eur: f64,
    /// Vrai si Claude doit trancher (confiance ≤ seuil, ou Jev indisponible).
    pub escalate: bool,
    pub escalate_reason: Option<String>,
    pub simulated: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClaudeOutcome {
    pub category: Category,
    pub urgency: Urgency,
    pub confidence: f64,
    pub summary: String,
    pub proposed_action: String,
    pub needs_human_review: bool,
    pub review_reason: Option<String>,
    pub draft_reply: Option<String>,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_eur: f64,
    pub simulated: bool,
    pub error: Option<String>,
}

/// Dossier qui circule entre les nœuds n8n : chaque étage complète sa partie.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
    pub email: Email,
    #[serde(default)]
    pub rules: Option<RuleOutcome>,
    #[serde(default)]
    pub jev: Option<JevOutcome>,
    #[serde(default)]
    pub claude: Option<ClaudeOutcome>,
}

/// Ligne de la table `triage_results`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FinalRecord {
    pub message_id: String,
    pub received_at: Option<String>,
    pub from_address: String,
    pub subject: String,
    pub category: Category,
    pub urgency: Urgency,
    pub confidence: f64,
    pub decided_by: Route,
    pub rule_id: Option<String>,
    pub jev_category: Option<Category>,
    pub jev_confidence: Option<f64>,
    pub escalation_reason: Option<String>,
    pub sensitive: bool,
    pub summary: Option<String>,
    pub proposed_action: String,
    pub draft_reply: Option<String>,
    pub needs_review: bool,
    pub review_reason: Option<String>,
    pub alert: bool,
    pub jev_credits: u32,
    pub claude_input_tokens: u64,
    pub claude_output_tokens: u64,
    pub cost_eur: f64,
    pub simulated: bool,
    pub expected_category: Option<Category>,
}
