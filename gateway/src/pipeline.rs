//! Assemblage : règles → Jev → Claude, puis décision finale.
//! n8n orchestre les mêmes étapes nœud par nœud ; `run_full` les enchaîne en mémoire
//! (tests, commande `eval`, endpoint `/triage/full`).

use crate::claude::ClaudeClient;
use crate::config::Config;
use crate::jev::JevClient;
use crate::model::{Case, Category, Email, FinalRecord, Route, RuleOutcome, Urgency};
use crate::rules::RulesConfig;
use anyhow::Result;

pub struct Engines {
    pub cfg: Config,
    pub rules: RulesConfig,
    pub jev: JevClient,
    pub claude: ClaudeClient,
}

impl Engines {
    pub fn new(cfg: Config) -> Result<Engines> {
        let rules = RulesConfig::load(&cfg.rules_file)?;
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()?;
        Ok(Engines {
            jev: JevClient::new(http.clone()),
            claude: ClaudeClient::new(http),
            rules,
            cfg,
        })
    }

    pub fn apply_rules(&self, case: &mut Case, already_processed: bool) {
        case.rules = Some(if already_processed {
            RuleOutcome {
                already_processed: true,
                reason: "déjà traité : ignoré".into(),
                ..RuleOutcome::default()
            }
        } else {
            self.rules.apply(&case.email)
        });
    }

    pub async fn apply_jev(&self, case: &mut Case) {
        case.jev = Some(self.jev.decide(&self.cfg, &case.email).await);
    }

    pub async fn apply_claude(&self, case: &mut Case) {
        case.claude = Some(self.claude.arbitrate(&self.cfg, case).await);
    }

    /// Chaîne complète en mémoire, mêmes embranchements que le workflow n8n.
    pub async fn run_full(&self, email: Email) -> FinalRecord {
        let mut case = Case {
            email,
            rules: None,
            jev: None,
            claude: None,
        };
        self.apply_rules(&mut case, false);
        let rules = case.rules.clone().unwrap_or_default();
        if !rules.decided {
            if rules.sensitive {
                self.apply_claude(&mut case).await;
            } else {
                self.apply_jev(&mut case).await;
                if case.jev.as_ref().is_some_and(|j| j.escalate) {
                    self.apply_claude(&mut case).await;
                }
            }
        }
        finalize(&case)
    }
}

/// Décision finale : le dernier étage atteint fait foi.
pub fn finalize(case: &Case) -> FinalRecord {
    let e = &case.email;
    let rules = case.rules.clone().unwrap_or_default();
    let jev_cost = case.jev.as_ref().map(|j| j.cost_eur).unwrap_or(0.0);
    let claude_cost = case.claude.as_ref().map(|c| c.cost_eur).unwrap_or(0.0);
    let simulated = case.jev.as_ref().is_some_and(|j| j.simulated)
        || case.claude.as_ref().is_some_and(|c| c.simulated);

    let (
        decided_by,
        category,
        urgency,
        confidence,
        summary,
        action,
        draft,
        mut needs_review,
        mut review_reason,
    ) = if let Some(c) = &case.claude {
        (
            Route::Claude,
            c.category,
            c.urgency,
            c.confidence,
            Some(c.summary.clone()),
            c.proposed_action.clone(),
            c.draft_reply.clone(),
            c.needs_human_review,
            c.review_reason.clone(),
        )
    } else if let Some(j) = &case.jev {
        (
            Route::Jev,
            j.category,
            j.urgency.unwrap_or(Urgency::Normale),
            j.confidence,
            None,
            default_action(j.category),
            None,
            false,
            None,
        )
    } else {
        let category = rules.category.unwrap_or(Category::Autre);
        (
            Route::Regle,
            category,
            rules.urgency.unwrap_or(Urgency::Normale),
            1.0,
            None,
            default_action(category),
            None,
            !rules.decided,
            (!rules.decided).then(|| rules.reason.clone()),
        )
    };
    if rules.sensitive && !needs_review {
        needs_review = true;
        review_reason = Some(format!(
            "contenu sensible : {}",
            rules.sensitive_reasons.join(", ")
        ));
    }

    FinalRecord {
        message_id: e.message_id.clone(),
        received_at: e.date.clone(),
        from_address: e.from_address.clone(),
        subject: e.subject.clone(),
        category,
        urgency,
        confidence,
        decided_by,
        rule_id: rules.rule_id.clone(),
        jev_category: case.jev.as_ref().map(|j| j.category),
        jev_confidence: case.jev.as_ref().map(|j| j.confidence),
        escalation_reason: if rules.sensitive {
            Some(rules.reason.clone())
        } else {
            case.jev.as_ref().and_then(|j| j.escalate_reason.clone())
        },
        sensitive: rules.sensitive,
        summary,
        proposed_action: action,
        draft_reply: draft,
        alert: urgency == Urgency::Haute || needs_review,
        needs_review,
        review_reason,
        jev_credits: case.jev.as_ref().map(|j| j.credits).unwrap_or(0),
        claude_input_tokens: case.claude.as_ref().map(|c| c.input_tokens).unwrap_or(0),
        claude_output_tokens: case.claude.as_ref().map(|c| c.output_tokens).unwrap_or(0),
        cost_eur: jev_cost + claude_cost,
        simulated,
        expected_category: e.expected.as_ref().map(|x| x.category),
    }
}

fn default_action(category: Category) -> String {
    match category {
        Category::Facture => "Transmettre à la comptabilité.",
        Category::Devis => "Qualifier la demande et préparer un devis.",
        Category::Support => "Ouvrir un ticket support.",
        Category::Rh => "Transmettre au responsable du recrutement.",
        Category::Administratif => "Classer dans « Administratif » et traiter avant échéance.",
        Category::Spam => "Archiver, aucune réponse.",
        Category::Autre => "Lire et traiter manuellement.",
    }
    .to_string()
}
