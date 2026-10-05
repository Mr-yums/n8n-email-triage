//! Simulateurs du MODE DÉMO. Aucun réseau, aucune clé, résultat déterministe.
//!
//! Ce ne sont PAS des modèles : ce sont des classifieurs à mots-clés qui imitent la *forme*
//! des réponses de Jev (catégorie + probabilités) et de Claude (catégorie, résumé, action,
//! brouillon). Ils ont été calibrés sur le jeu d'exemples pour montrer la mécanique de
//! routage ; ils ne mesurent en rien la qualité réelle de Jev ou de Claude.

use crate::model::{Category, Email, Urgency};
use crate::text::{contains_phrase, first_sentence, guess_language, normalize};
use std::collections::BTreeMap;

type Lexicon = &'static [(&'static str, f64)];

fn lexicon(category: Category) -> Lexicon {
    match category {
        Category::Facture => &[
            ("facture", 3.0),
            ("factures", 3.0),
            ("invoice", 3.0),
            ("echeance", 2.0),
            ("montant ttc", 2.0),
            ("reglement", 2.0),
            ("paiement", 2.0),
            ("payment", 2.0),
            ("acompte", 2.0),
            ("impaye", 3.0),
            ("total", 1.0),
            ("charged", 1.0),
        ],
        Category::Devis => &[
            ("devis", 3.0),
            ("quote", 3.0),
            ("quotation", 3.0),
            ("tarifs", 2.0),
            ("pricing", 2.0),
            ("prix", 2.0),
            ("proposition chiffree", 3.0),
            ("chiffrage", 3.0),
            ("estimate", 1.0),
            ("sur mesure", 1.0),
            ("custom", 1.0),
            ("lead time", 1.0),
            ("delais", 1.0),
            ("budget", 1.0),
        ],
        Category::Support => &[
            ("commande", 2.0),
            ("order", 2.0),
            ("livraison", 1.0),
            ("delivery", 1.0),
            ("erreur", 2.0),
            ("error", 2.0),
            ("ne fonctionne plus", 3.0),
            ("not working", 3.0),
            ("bloquant", 2.0),
            ("panne", 3.0),
            ("probleme", 2.0),
            ("remboursement", 3.0),
            ("refund", 3.0),
            ("abimee", 3.0),
            ("damaged", 3.0),
            ("reclamation", 3.0),
            ("complaint", 3.0),
            ("assembly", 1.0),
        ],
        Category::Rh => &[
            ("candidature", 3.0),
            ("candidat", 2.0),
            ("cv", 3.0),
            ("resume", 3.0),
            ("poste", 2.0),
            ("position", 2.0),
            ("entretien", 2.0),
            ("interview", 2.0),
            ("apply", 2.0),
            ("hiring", 2.0),
            ("stage", 2.0),
            ("eleve", 1.0),
            ("recrutement", 2.0),
            ("lettre de motivation", 3.0),
        ],
        Category::Administratif => &[
            ("cotisations", 3.0),
            ("urssaf", 3.0),
            ("impots", 3.0),
            ("tva", 2.0),
            ("assurance", 3.0),
            ("attestation", 2.0),
            ("banque", 2.0),
            ("releve", 2.0),
            ("cloture", 2.0),
            ("justificatifs", 2.0),
            ("comptable", 2.0),
            ("juridique", 2.0),
            ("tribunal", 2.0),
            ("demeure", 2.0),
            ("rgpd", 2.0),
            ("convention", 3.0),
            ("signer", 2.0),
            ("signe", 1.0),
            ("document", 1.0),
            ("renouvellement", 2.0),
            ("contrat", 1.0),
        ],
        Category::Spam => &[
            ("seo", 3.0),
            ("referencement", 3.0),
            ("visibilite", 2.0),
            ("leads", 3.0),
            ("appel decouverte", 3.0),
            ("15 minutes", 2.0),
            ("15 minute call", 3.0),
            ("newsletter", 2.0),
            ("unsubscribe", 2.0),
            ("desabonner", 2.0),
            ("offre speciale", 3.0),
            ("promo", 2.0),
            ("outreach", 2.0),
            ("premiere page", 2.0),
            ("partenariat", 3.0),
            ("partenaire", 3.0),
            ("gift cards", 3.0),
        ],
        Category::Autre => &[
            ("dejeuner", 3.0),
            ("lunch", 3.0),
            ("cafe", 2.0),
            ("felicitations", 2.0),
            ("salut", 1.0),
        ],
    }
}

/// Signaux forts que le « Claude simulé » sait lire en plus (lecture fine du contexte).
fn strong_signals(category: Category) -> &'static [&'static str] {
    match category {
        Category::Facture => &[
            "facture d acompte",
            "pas encore recu la facture",
            "nouveau rib",
            "reglement de la facture",
        ],
        Category::Devis => &[
            "demande de devis",
            "faire une proposition chiffree",
            "request for quote",
        ],
        Category::Support => &[
            "remboursement",
            "refund",
            "est arrivee avec",
            "ne fonctionne plus",
        ],
        Category::Rh => &["convention de stage", "arret maladie", "candidature"],
        Category::Administratif => &[
            "mise en demeure",
            "mettons en demeure",
            "rgpd",
            "suppression de l ensemble de mes donnees",
        ],
        Category::Spam => &["gift cards", "carte cadeau", "do not tell anyone"],
        Category::Autre => &[],
    }
}

const URGENT_SIGNALS: Lexicon = &[
    ("urgent", 3.0),
    ("asap", 3.0),
    ("bloquant", 3.0),
    ("aujourd hui", 2.0),
    ("today", 2.0),
    ("sous huit jours", 3.0),
    ("depuis ce matin", 2.0),
    ("immediatement", 2.0),
    ("rapidement", 2.0),
];

fn score(lex: Lexicon, subject: &str, body: &str, subject_weight: f64) -> f64 {
    lex.iter()
        .map(|(phrase, w)| {
            let mut s = 0.0;
            if contains_phrase(subject, phrase) {
                s += w * subject_weight;
            }
            if contains_phrase(body, phrase) {
                s += w;
            }
            s
        })
        .sum()
}

fn attachment_bonus(email: &Email, category: Category) -> f64 {
    let names = normalize(&email.attachments.join(" ").replace(['_', '-'], " "));
    match category {
        Category::Facture
            if ["facture", "invoice", "fa", "inv"]
                .iter()
                .any(|k| contains_phrase(&names, k)) =>
        {
            2.0
        }
        Category::Rh if ["cv", "resume"].iter().any(|k| contains_phrase(&names, k)) => 2.0,
        _ => 0.0,
    }
}

/// Probabilités par catégorie : part de score, plus un a priori uniforme (évite les 100 %).
fn distribution(scores: &BTreeMap<Category, f64>, prior: f64) -> BTreeMap<Category, f64> {
    let total: f64 = scores.values().sum::<f64>() + prior;
    let share = prior / Category::ALL.len() as f64;
    Category::ALL
        .into_iter()
        .map(|c| (c, (scores.get(&c).copied().unwrap_or(0.0) + share) / total))
        .collect()
}

fn top(dist: &BTreeMap<Category, f64>) -> (Category, f64) {
    dist.iter()
        .fold((Category::Autre, f64::MIN), |best, (&c, &p)| {
            if p > best.1 { (c, p) } else { best }
        })
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub struct SimJev {
    pub category: Category,
    pub confidence: f64,
    pub probabilities: BTreeMap<Category, f64>,
}

/// « Jev simulé » : catégorie + probabilités à partir de mots-clés (objet ×2).
pub fn jev_category(email: &Email) -> SimJev {
    let subject = normalize(&email.subject);
    let body = normalize(&email.body);
    let scores: BTreeMap<Category, f64> = Category::ALL
        .into_iter()
        .map(|c| {
            (
                c,
                score(lexicon(c), &subject, &body, 2.0) + attachment_bonus(email, c),
            )
        })
        .collect();
    let total: f64 = scores.values().sum();
    if total < 3.0 {
        // Presque aucun signal : « autre », confiance basse → Claude tranchera.
        let mut probabilities: BTreeMap<Category, f64> =
            Category::ALL.into_iter().map(|c| (c, 0.08)).collect();
        probabilities.insert(Category::Autre, 0.52);
        return SimJev {
            category: Category::Autre,
            confidence: 0.52,
            probabilities,
        };
    }
    let probabilities: BTreeMap<Category, f64> = distribution(&scores, 1.0)
        .into_iter()
        .map(|(c, p)| (c, round3(p)))
        .collect();
    let (category, confidence) = top(&probabilities);
    SimJev {
        category,
        confidence,
        probabilities,
    }
}

/// « Jev simulé », seconde décision : urgence.
pub fn jev_urgency(email: &Email, category: Category) -> Urgency {
    urgency_from_signals(email, category, 3.0)
}

fn urgency_from_signals(email: &Email, category: Category, threshold: f64) -> Urgency {
    let s = score(
        URGENT_SIGNALS,
        &normalize(&email.subject),
        &normalize(&email.body),
        2.0,
    );
    if s >= threshold {
        Urgency::Haute
    } else if category == Category::Spam {
        Urgency::Basse
    } else {
        Urgency::Normale
    }
}

pub struct SimClaude {
    pub category: Category,
    pub urgency: Urgency,
    pub confidence: f64,
    pub summary: String,
    pub proposed_action: String,
    pub draft_reply: Option<String>,
}

/// « Claude simulé » : même lexique, objet ×3, plus des signaux forts ; produit résumé,
/// action proposée et brouillon (jamais envoyé).
pub fn claude(email: &Email, sensitive_reasons: &[String]) -> SimClaude {
    let subject = normalize(&email.subject);
    let body = normalize(&email.body);
    let all = format!("{subject}{body}");
    let scores: BTreeMap<Category, f64> = Category::ALL
        .into_iter()
        .map(|c| {
            let strong = strong_signals(c)
                .iter()
                .filter(|p| contains_phrase(&all, p))
                .count() as f64
                * 24.0;
            (
                c,
                score(lexicon(c), &subject, &body, 3.0) + attachment_bonus(email, c) + strong,
            )
        })
        .collect();
    let total: f64 = scores.values().sum();
    let (category, confidence) = if total < 3.0 {
        (Category::Autre, 0.55)
    } else {
        // Confiance = écart entre les deux meilleures catégories (0,5 si ex aequo, 0,97 au plus).
        let mut sorted: Vec<(Category, f64)> = scores.iter().map(|(&c, &v)| (c, v)).collect();
        sorted.sort_by(|a, b| b.1.total_cmp(&a.1));
        let (best, second) = (sorted[0], sorted[1].1);
        (
            best.0,
            round3((0.5 + 0.5 * (best.1 - second) / best.1).clamp(0.5, 0.97)),
        )
    };
    let urgency = urgency_from_signals(email, category, 2.0);
    let lang = guess_language(&format!("{} {}", email.subject, email.body));
    let summary = format!("{} — {}", email.from_name, first_sentence(&email.body, 180));
    SimClaude {
        category,
        urgency,
        confidence,
        proposed_action: proposed_action(category, !sensitive_reasons.is_empty(), lang),
        draft_reply: draft_reply(category, email, lang),
        summary,
    }
}

fn proposed_action(category: Category, sensitive: bool, lang: &str) -> String {
    let base = match category {
        Category::Facture if sensitive => {
            "Ne rien payer. Vérifier le changement par téléphone auprès du contact habituel (numéro déjà connu, pas celui du mail)."
        }
        Category::Facture => "Transmettre à la comptabilité et rapprocher du bon de commande.",
        Category::Devis => {
            "Qualifier le besoin et préparer un devis ; brouillon de réponse prêt à relire."
        }
        Category::Support => {
            "Ouvrir un ticket, vérifier la commande et répondre au client ; brouillon prêt à relire."
        }
        Category::Rh if sensitive => {
            "Transmettre au dirigeant / RH uniquement (donnée personnelle)."
        }
        Category::Rh => "Transmettre au responsable du recrutement ou des stages.",
        Category::Administratif if sensitive => {
            "Transmettre au dirigeant ; vérifier le délai légal de réponse."
        }
        Category::Administratif => "Classer et traiter avant l'échéance indiquée.",
        Category::Spam if sensitive => {
            "Tentative de fraude probable : ne rien acheter ni répondre, prévenir l'équipe."
        }
        Category::Spam => "Archiver, aucune réponse.",
        Category::Autre => "Lire et répondre personnellement si nécessaire.",
    };
    if lang == "en" {
        format!("{base} (email en anglais)")
    } else {
        base.to_string()
    }
}

fn draft_reply(category: Category, email: &Email, lang: &str) -> Option<String> {
    let name = email.from_name.split_whitespace().next().unwrap_or("");
    match (category, lang) {
        (Category::Devis, "en") => Some(format!(
            "Hello {name},\n\nThank you for your request. To prepare an accurate quote, could you confirm the dimensions, the preferred wood and your target date?\n\nBest regards,\nAtelier Lumen"
        )),
        (Category::Devis, _) => Some(format!(
            "Bonjour {name},\n\nMerci pour votre demande. Pour vous adresser un devis précis, pourriez-vous nous confirmer les dimensions, l'essence de bois souhaitée et le délai visé ?\n\nBien cordialement,\nAtelier Lumen"
        )),
        (Category::Support, "en") => Some(format!(
            "Hello {name},\n\nThank you for your message. We are looking into it and will get back to you within one business day.\n\nBest regards,\nAtelier Lumen"
        )),
        (Category::Support, _) => Some(format!(
            "Bonjour {name},\n\nMerci pour votre message, nous regardons cela et revenons vers vous sous un jour ouvré.\n\nBien cordialement,\nAtelier Lumen"
        )),
        _ => None,
    }
}
