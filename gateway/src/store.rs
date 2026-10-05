//! Stockage PostgreSQL des décisions et des alertes.

use crate::model::FinalRecord;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_postgres::{Client, NoTls, Row};

const SCHEMA: &str = include_str!("../../db/schema.sql");

pub struct Store {
    client: Client,
}

impl Store {
    /// Connexion avec nouvelles tentatives (Postgres peut démarrer après la passerelle).
    pub async fn connect(url: &str) -> Result<Store> {
        let mut last_err = None;
        for _ in 0..30 {
            match tokio_postgres::connect(url, NoTls).await {
                Ok((client, connection)) => {
                    tokio::spawn(async move {
                        if let Err(e) = connection.await {
                            eprintln!("connexion PostgreSQL perdue : {e}");
                        }
                    });
                    client
                        .batch_execute(SCHEMA)
                        .await
                        .context("application du schéma")?;
                    return Ok(Store { client });
                }
                Err(e) => {
                    last_err = Some(e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        Err(last_err
            .map(anyhow::Error::from)
            .unwrap_or_else(|| anyhow::anyhow!("connexion impossible")))
        .context("connexion PostgreSQL")
    }

    pub async fn exists(&self, message_id: &str) -> Result<bool> {
        let row = self
            .client
            .query_opt(
                "SELECT 1 FROM triage_results WHERE message_id = $1",
                &[&message_id],
            )
            .await?;
        Ok(row.is_some())
    }

    /// Insère une décision. Un message déjà présent n'est pas écrasé (on ne retraite pas).
    pub async fn insert(&self, r: &FinalRecord) -> Result<bool> {
        let n = self
            .client
            .execute(
                "INSERT INTO triage_results (message_id, received_at, from_address, subject, category, urgency,
                    confidence, decided_by, rule_id, jev_category, jev_confidence, escalation_reason, sensitive,
                    summary, proposed_action, draft_reply, needs_review, review_reason, alert, jev_credits,
                    claude_input_tokens, claude_output_tokens, cost_eur, simulated, expected_category)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25)
                 ON CONFLICT (message_id) DO NOTHING",
                &[
                    &r.message_id,
                    &r.received_at,
                    &r.from_address,
                    &r.subject,
                    &r.category.id(),
                    &r.urgency.id(),
                    &r.confidence,
                    &r.decided_by.id(),
                    &r.rule_id,
                    &r.jev_category.map(|c| c.id()),
                    &r.jev_confidence,
                    &r.escalation_reason,
                    &r.sensitive,
                    &r.summary,
                    &r.proposed_action,
                    &r.draft_reply,
                    &r.needs_review,
                    &r.review_reason,
                    &r.alert,
                    &i32::try_from(r.jev_credits).unwrap_or(i32::MAX),
                    &i64::try_from(r.claude_input_tokens).unwrap_or(i64::MAX),
                    &i64::try_from(r.claude_output_tokens).unwrap_or(i64::MAX),
                    &r.cost_eur,
                    &r.simulated,
                    &r.expected_category.map(|c| c.id()),
                ],
            )
            .await?;
        Ok(n == 1)
    }

    pub async fn insert_alert(&self, payload: &Value) -> Result<()> {
        let message_id = payload
            .get("message_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        self.client
            .execute(
                "INSERT INTO triage_alerts (message_id, payload) VALUES ($1, $2::text::jsonb)",
                &[&message_id, &payload.to_string()],
            )
            .await?;
        Ok(())
    }

    pub async fn results(&self) -> Result<Value> {
        let rows = self
            .client
            .query(
                "SELECT row_to_json(t)::text FROM (SELECT * FROM triage_results ORDER BY received_at, message_id) t",
                &[],
            )
            .await?;
        rows_to_json(&rows)
    }

    pub async fn alerts(&self) -> Result<Value> {
        let rows = self
            .client
            .query(
                "SELECT row_to_json(t)::text FROM (SELECT * FROM triage_alerts ORDER BY id) t",
                &[],
            )
            .await?;
        rows_to_json(&rows)
    }

    pub async fn stats(&self) -> Result<Value> {
        let row = self
            .client
            .query_one(
                "SELECT count(*),
                        count(*) FILTER (WHERE decided_by = 'regle'),
                        count(*) FILTER (WHERE decided_by = 'jev'),
                        count(*) FILTER (WHERE decided_by = 'claude'),
                        coalesce(sum(cost_eur), 0)::float8,
                        count(*) FILTER (WHERE needs_review),
                        count(*) FILTER (WHERE urgency = 'haute'),
                        count(*) FILTER (WHERE expected_category IS NOT NULL),
                        count(*) FILTER (WHERE expected_category = category),
                        coalesce(sum(jev_credits), 0)::bigint,
                        coalesce(bool_or(simulated), false),
                        (SELECT count(*) FROM triage_alerts)
                 FROM triage_results",
                &[],
            )
            .await?;
        let total: i64 = row.get(0);
        let cost: f64 = row.get(4);
        Ok(json!({
            "total": total,
            "by_route": { "regle": row.get::<_, i64>(1), "jev": row.get::<_, i64>(2), "claude": row.get::<_, i64>(3) },
            "cost_eur": cost,
            "cost_per_1000_eur": if total > 0 { cost / total as f64 * 1000.0 } else { 0.0 },
            "needs_review": row.get::<_, i64>(5),
            "urgent": row.get::<_, i64>(6),
            "with_expected": row.get::<_, i64>(7),
            "matching_expected": row.get::<_, i64>(8),
            "jev_credits": row.get::<_, i64>(9),
            "simulated": row.get::<_, bool>(10),
            "alerts_received": row.get::<_, i64>(11),
        }))
    }
}

fn rows_to_json(rows: &[Row]) -> Result<Value> {
    rows.iter()
        .map(|r| serde_json::from_str::<Value>(r.get::<_, &str>(0)).context("ligne JSON"))
        .collect::<Result<Vec<_>>>()
        .map(Value::Array)
}
