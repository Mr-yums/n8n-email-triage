//! Jeu d'exemples → catégories, routage et urgence attendus, en mode démo (sans clé, sans réseau).

use triage_gateway::config::Config;
use triage_gateway::model::{Category, Route, Urgency};
use triage_gateway::pipeline::Engines;

fn root() -> String {
    format!("{}/..", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn every_sample_is_routed_and_classified_as_expected() {
    let cfg = Config::demo_for_tests(&root());
    let samples = triage_gateway::load_samples(&cfg.samples_file).expect("jeu d'exemples");
    assert!(
        samples.len() >= 30,
        "le jeu doit contenir au moins 30 emails"
    );
    let engines = Engines::new(cfg).expect("moteurs");

    let mut failures = Vec::new();
    for email in samples {
        let id = email.id.clone().unwrap_or_default();
        let expected = email.expected.clone().expect("bloc expected");
        let r = engines.run_full(email).await;
        if r.category != expected.category {
            failures.push(format!(
                "{id}: catégorie {:?} au lieu de {:?}",
                r.category, expected.category
            ));
        }
        if r.decided_by != expected.route {
            failures.push(format!(
                "{id}: tranché par {:?} au lieu de {:?}",
                r.decided_by, expected.route
            ));
        }
        if (r.urgency == Urgency::Haute) != expected.urgent {
            failures.push(format!(
                "{id}: urgence {:?} (urgent attendu : {})",
                r.urgency, expected.urgent
            ));
        }
        if r.sensitive && !r.needs_review {
            failures.push(format!("{id}: cas sensible sans validation humaine"));
        }
        if r.alert != (r.urgency == Urgency::Haute || r.needs_review) {
            failures.push(format!("{id}: règle d'alerte incohérente"));
        }
        if r.decided_by == Route::Claude
            && matches!(r.category, Category::Devis | Category::Support)
            && r.draft_reply.is_none()
        {
            failures.push(format!("{id}: brouillon de réponse manquant"));
        }
        if r.decided_by == Route::Regle && r.cost_eur != 0.0 {
            failures.push(format!("{id}: une règle ne doit rien coûter"));
        }
    }
    assert!(failures.is_empty(), "écarts :\n{}", failures.join("\n"));
}

#[tokio::test]
async fn demo_is_deterministic() {
    let cfg = Config::demo_for_tests(&root());
    let samples = triage_gateway::load_samples(&cfg.samples_file).unwrap();
    let engines = Engines::new(cfg).unwrap();
    for email in samples.into_iter().take(30) {
        let a = serde_json::to_string(&engines.run_full(email.clone()).await).unwrap();
        let b = serde_json::to_string(&engines.run_full(email).await).unwrap();
        assert_eq!(a, b);
    }
}

#[tokio::test]
async fn jev_outage_escalates_to_claude_instead_of_failing() {
    // Mode live avec une URL Jev injoignable et sans clé Claude : la chaîne ne plante pas,
    // le cas part chez un humain. Aucun appel réseau sortant réel (127.0.0.1:9).
    let mut cfg = Config::demo_for_tests(&root());
    cfg.mode = triage_gateway::config::Mode::Live;
    cfg.jev_base_url = "http://127.0.0.1:9".into();
    cfg.jev_api_key = Some("test-sans-valeur".into());
    let samples = triage_gateway::load_samples(&cfg.samples_file).unwrap();
    let email = samples
        .into_iter()
        .find(|e| e.id.as_deref() == Some("e15"))
        .unwrap();
    let engines = Engines::new(cfg).unwrap();
    let r = engines.run_full(email).await;
    assert_eq!(r.decided_by, Route::Claude);
    assert!(r.needs_review);
    assert!(
        r.escalation_reason
            .unwrap_or_default()
            .contains("Jev indisponible")
    );
}
