//! triage-gateway — sous-commandes :
//!   serve        API + tableau de bord (défaut)
//!   seed         déclenche le workflow n8n sur le jeu d'exemples (une fois) et attend la fin
//!   eval         rejoue le jeu d'exemples en mémoire et affiche la mesure coût/qualité
//!   healthcheck  sonde locale pour Docker

use anyhow::{Context, Result, bail};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use triage_gateway::config::{Config, Mode};
use triage_gateway::model::Route;
use triage_gateway::pipeline::Engines;
use triage_gateway::server::{AppState, router};
use triage_gateway::store::Store;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str).unwrap_or("serve") {
        "serve" => serve().await,
        "seed" => seed().await,
        "eval" => eval(args.iter().any(|a| a == "--live-costs-money")).await,
        "healthcheck" => healthcheck().await,
        other => Err(anyhow::anyhow!(
            "sous-commande inconnue : {other} (serve | seed | eval | healthcheck)"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erreur : {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn serve() -> Result<()> {
    let cfg = Config::from_env()?;
    let db = cfg.database_url.clone().context("DATABASE_URL manquante")?;
    let samples: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cfg.samples_file)?)?;
    let addr = cfg.listen_addr.clone();
    let mode = cfg.mode;
    let model = cfg.claude_model.clone();
    let state = AppState {
        engines: Engines::new(cfg)?,
        store: Store::connect(&db).await?,
        samples,
    };
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "triage-gateway en écoute sur {addr} — mode {} — modèle Claude {model}",
        if mode == Mode::Demo {
            "DÉMO (Jev et Claude simulés, aucune clé)"
        } else {
            "LIVE (appels réels facturés)"
        }
    );
    axum::serve(listener, router(Arc::new(state)))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

async fn healthcheck() -> Result<()> {
    let addr = std::env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let port = addr.rsplit(':').next().unwrap_or("8080");
    let resp = reqwest::get(format!("http://127.0.0.1:{port}/healthz")).await?;
    if !resp.status().is_success() {
        bail!("statut {}", resp.status());
    }
    Ok(())
}

async fn seed() -> Result<()> {
    if std::env::var("TRIAGE_MODE").as_deref() == Ok("live") {
        println!(
            "Mode live : le jeu d'exemples n'est pas envoyé (il consommerait des crédits). Rien à faire."
        );
        return Ok(());
    }
    let gateway = std::env::var("GATEWAY_URL").unwrap_or_else(|_| "http://gateway:8080".into());
    let webhook = std::env::var("N8N_DEMO_WEBHOOK")
        .unwrap_or_else(|_| "http://n8n:5678/webhook/email-triage-demo".into());
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let stats = |http: reqwest::Client, gateway: String| async move {
        http.get(format!("{gateway}/api/stats"))
            .send()
            .await?
            .error_for_status()?
            .json::<serde_json::Value>()
            .await
    };

    let mut current = None;
    for _ in 0..90 {
        if let Ok(s) = stats(http.clone(), gateway.clone()).await {
            current = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let current = current.context("passerelle injoignable")?;
    if current["total"].as_i64().unwrap_or(0) > 0 {
        println!(
            "Jeu d'exemples déjà traité ({} emails) : rien à faire. `docker compose down -v` pour repartir de zéro.",
            current["total"]
        );
        return Ok(());
    }

    let expected = {
        let samples: serde_json::Value = http
            .get(format!("{gateway}/samples"))
            .send()
            .await?
            .json()
            .await?;
        samples["emails"].as_array().map(Vec::len).unwrap_or(0) as i64
    };
    println!("Déclenchement du workflow n8n ({expected} emails d'exemple)…");
    let mut triggered = false;
    for _ in 0..120 {
        match http
            .post(&webhook)
            .json(&serde_json::json!({ "source": "demo-seed" }))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                triggered = true;
                break;
            }
            _ => tokio::time::sleep(Duration::from_secs(2)).await,
        }
    }
    if !triggered {
        bail!("le webhook n8n {webhook} ne répond pas (workflow non publié ?)");
    }

    for _ in 0..150 {
        let s = stats(http.clone(), gateway.clone()).await?;
        if s["total"].as_i64().unwrap_or(0) >= expected {
            println!(
                "Terminé : {} emails — règles {} · Jev {} · Claude {} — coût {:.4} € (≈ {:.2} € / 1 000 emails) — {} à vérifier, {} alertes.",
                s["total"],
                s["by_route"]["regle"],
                s["by_route"]["jev"],
                s["by_route"]["claude"],
                s["cost_eur"].as_f64().unwrap_or(0.0),
                s["cost_per_1000_eur"].as_f64().unwrap_or(0.0),
                s["needs_review"],
                s["alerts_received"]
            );
            let url = std::env::var("DASHBOARD_PUBLIC_URL")
                .unwrap_or_else(|_| "http://localhost:18090/".into());
            println!("Tableau de bord : {url}");
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    bail!("délai dépassé : le workflow n'a pas enregistré tous les emails")
}

async fn eval(allow_live: bool) -> Result<()> {
    let cfg = Config::from_env()?;
    if cfg.mode == Mode::Live && !allow_live {
        bail!(
            "TRIAGE_MODE=live : ajoutez --live-costs-money pour accepter des appels Jev et Claude facturés"
        );
    }
    let samples = triage_gateway::load_samples(&cfg.samples_file)?;
    let engines = Engines::new(cfg)?;
    let (mut n, mut ok, mut cost, mut credits, mut review) = (0usize, 0usize, 0.0f64, 0u32, 0usize);
    let mut routes = [0usize; 3];
    println!("| id | attendu | obtenu | tranché par | confiance | urgence | à vérifier | coût € |");
    println!("|---|---|---|---|---|---|---|---|");
    for email in samples {
        let id = email.id.clone().unwrap_or_else(|| email.message_id.clone());
        let expected = email.expected.clone();
        let r = engines.run_full(email).await;
        n += 1;
        cost += r.cost_eur;
        credits += r.jev_credits;
        review += usize::from(r.needs_review);
        routes[match r.decided_by {
            Route::Regle => 0,
            Route::Jev => 1,
            Route::Claude => 2,
        }] += 1;
        let good = expected.as_ref().is_some_and(|x| x.category == r.category);
        ok += usize::from(good);
        println!(
            "| {id} | {} | {}{} | {} | {:.2} | {} | {} | {:.5} |",
            expected.map(|x| x.category.id()).unwrap_or("-"),
            r.category.id(),
            if good { "" } else { " ✗" },
            r.decided_by.id(),
            r.confidence,
            r.urgency.id(),
            if r.needs_review { "oui" } else { "" },
            r.cost_eur
        );
    }
    // Référence (démo seulement, gratuite) : coût si Claude lisait chaque email.
    let mut baseline = None;
    if engines.cfg.mode == Mode::Demo {
        let mut total = 0.0;
        for email in triage_gateway::load_samples(&engines.cfg.samples_file)? {
            let case = triage_gateway::model::Case {
                email,
                rules: None,
                jev: None,
                claude: None,
            };
            total += engines.claude.arbitrate(&engines.cfg, &case).await.cost_eur;
        }
        baseline = Some(total / n.max(1) as f64 * 1000.0);
    }
    let pct = |k: usize| 100.0 * k as f64 / n.max(1) as f64;
    println!();
    println!(
        "Emails : {n} — règles {} ({:.0} %) · Jev {} ({:.0} %) · Claude {} ({:.0} %)",
        routes[0],
        pct(routes[0]),
        routes[1],
        pct(routes[1]),
        routes[2],
        pct(routes[2])
    );
    println!(
        "Catégorie conforme à l'attendu : {ok}/{n} ({:.0} %)",
        pct(ok)
    );
    println!("À vérifier par un humain : {review}");
    println!(
        "Crédits Jev : {credits} — coût total : {cost:.4} € — soit ≈ {:.2} € pour 1 000 emails",
        cost / n.max(1) as f64 * 1000.0
    );
    if let Some(b) = baseline {
        println!(
            "Référence « tout par Claude » ({}) : ≈ {b:.2} € pour 1 000 emails",
            engines.cfg.claude_model
        );
    }
    Ok(())
}
