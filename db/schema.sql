-- Schéma du tri d'emails. Appliqué (idempotent) par la passerelle au démarrage.

CREATE TABLE IF NOT EXISTS triage_results (
    message_id           text PRIMARY KEY,
    received_at          text,
    from_address         text NOT NULL,
    subject              text NOT NULL,
    category             text NOT NULL,
    urgency              text NOT NULL CHECK (urgency IN ('haute', 'normale', 'basse')),
    confidence           double precision NOT NULL,
    decided_by           text NOT NULL CHECK (decided_by IN ('regle', 'jev', 'claude')),
    rule_id              text,
    jev_category         text,
    jev_confidence       double precision,
    escalation_reason    text,
    sensitive            boolean NOT NULL DEFAULT false,
    summary              text,
    proposed_action      text NOT NULL,
    draft_reply          text,
    needs_review         boolean NOT NULL,
    review_reason        text,
    alert                boolean NOT NULL,
    jev_credits          integer NOT NULL DEFAULT 0,
    claude_input_tokens  bigint NOT NULL DEFAULT 0,
    claude_output_tokens bigint NOT NULL DEFAULT 0,
    cost_eur             double precision NOT NULL DEFAULT 0,
    simulated            boolean NOT NULL,
    expected_category    text,
    processed_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS triage_alerts (
    id          bigserial PRIMARY KEY,
    message_id  text,
    payload     jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now()
);

-- Synthèse coût / répartition : qui a tranché, combien, pour quel coût.
CREATE OR REPLACE VIEW triage_summary AS
SELECT decided_by,
       count(*)                                         AS emails,
       round(avg(confidence)::numeric, 3)               AS confiance_moyenne,
       sum(jev_credits)                                 AS credits_jev,
       sum(claude_input_tokens + claude_output_tokens)  AS tokens_claude,
       round(sum(cost_eur)::numeric, 5)                 AS cout_eur,
       count(*) FILTER (WHERE needs_review)             AS a_verifier,
       count(*) FILTER (WHERE urgency = 'haute')        AS urgents
FROM triage_results
GROUP BY decided_by;
