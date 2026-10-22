# Tri automatique des emails d'une PME — n8n + Jev + Claude

[![CI](https://github.com/Mr-yums/n8n-email-triage/actions/workflows/ci.yml/badge.svg)](https://github.com/Mr-yums/n8n-email-triage/actions/workflows/ci.yml)
![Licence MIT](https://img.shields.io/badge/licence-MIT-blue)

> Démo d'un tri d'emails pour PME : des règles simples, Jev, puis Claude, et un humain qui envoie.
> Lancement en une commande, **mode démo sans aucune clé**, données 100 % fictives.
> *English summary at the end.*

![Tableau de bord du tri en mode démo](docs/dashboard.png)

## Le problème

Une PME reçoit chaque jour des emails de toute sorte : un client mécontent, une demande de contrat, une demande de devis, une facture, une candidature, un courrier administratif, de la prospection… Quelqu'un les lit un par un pour savoir quoi en faire. C'est lent, des urgences passent à la trappe, et les arnaques (faux changement de RIB, fraude au président) arrivent au milieu du reste.

Des règles de messagerie classiques trient l'évident (une facture d'un fournisseur connu, une newsletter). Envoyer tout le reste à un grand modèle d'IA marche, mais revient cher à l'échelle et confie à une IA des décisions qui doivent rester humaines.

## Le pipeline

1. **Réception.** Les emails arrivent dans n8n (IMAP, Gmail, ou le jeu d'exemples en démo) : plaintes, demandes de contrat, devis, factures, candidatures, administratif, prospection.
2. **Règles simples** (gratuites, explicables) : expéditeurs connus, factures de fournisseurs connus avec PDF, envois de masse (`List-Unsubscribe`). Un contenu **sensible** (coordonnées bancaires, juridique, santé, RGPD, fraude) n'est jamais tranché par une règle : il part **directement chez Claude**.
3. **[Jev](https://iajev.com) classe avec une confiance.** Décision typée via son API (`POST /api/v1/decisions/run`) : une catégorie parmi 7 avec des probabilités, puis l'urgence. Jev garde la main **seulement si sa confiance dépasse le seuil de 0,85**.
4. **Sous le seuil, Claude relit et tranche.** Claude (API Messages d'Anthropic, `claude-sonnet-5` par défaut) reprend l'email avec tout le contexte, tranche, rédige un résumé, propose une action et un **brouillon de réponse**. Les cas sensibles de l'étape 2 suivent le même chemin.
5. **Brouillon, jamais envoyé seul.** Les brouillons sont stockés en base et attendent un humain. Tout cas sensible est marqué « à vérifier » par un garde-fou indépendant du modèle.
6. **Alertes et tableau de bord.** Les emails urgents ou à vérifier déclenchent une alerte webhook (Slack, Teams, ntfy, autre workflow n8n…). Le tableau de bord montre qui a tranché quoi, avec quelle confiance, et l'action proposée.

```mermaid
flowchart LR
    IN[Réception<br/>IMAP / Gmail<br/>ou jeu d'exemples] --> R{Règles<br/>simples}
    R -- expéditeur connu,<br/>facture fournisseur,<br/>envoi de masse --> DB[(Postgres<br/>triage_results)]
    R -- contenu sensible<br/>RIB, juridique, santé,<br/>RGPD, fraude --> C[Claude<br/>relit, tranche<br/>+ brouillon]
    R -- sinon --> J[Jev<br/>catégorie<br/>+ confiance]
    J -- confiance &gt; 0,85 --> DB
    J -- confiance ≤ 0,85<br/>ou Jev indisponible --> C
    C --> DB
    DB --> A{Urgent ou<br/>à vérifier ?}
    A -- oui --> W[Alerte webhook]
    DB --> D[Tableau de bord]
    DB -. optionnel .-> L[Libellé Gmail]
```

Catégories : facture · devis / demande commerciale · support client · RH / candidature · administratif · spam / prospection · autre.

## Ce que des règles de messagerie ne savent pas faire

Le jeu d'exemples contient des cas choisis pour ça (entreprise fictive « Atelier Lumen », mobilier sur mesure) :

- **Comprendre un client mécontent sans mot-clé.** « La table livrée hier est arrivée avec un pied fendu… nous recevons du monde samedi » ne contient ni « réclamation » ni « urgent ». Le tri le classe en support client, urgent, alerte l'équipe et prépare un brouillon de réponse au client.
- **Repérer une fraude.** Un « changement de coordonnées bancaires » envoyé depuis un domaine presque identique à celui d'un fournisseur connu, ou un « je suis en réunion, achète des cartes cadeaux et garde ça pour toi » (fraude au président) : aucune règle ne les classe d'office, Claude les signale et l'action proposée est de **ne rien payer** et de vérifier par un canal connu.
- **Préparer une réponse adaptée.** Pour un devis, une plainte ou une question client, Claude rédige un brouillon dans la langue de l'expéditeur, avec les informations à demander. Un humain le relit et l'envoie lui-même.
- **Savoir dire « je ne sais pas ».** Un « Could someone call me back about the thing we discussed? » reste en « autre », marqué à vérifier, plutôt que d'être rangé au hasard.

## Démarrage en 3 commandes

Prérequis : Docker avec Compose v2.

```bash
git clone https://github.com/Mr-yums/n8n-email-triage.git
cd n8n-email-triage
docker compose up --build
```

- Le premier lancement compile la passerelle Rust (≈ 4 min) ; les suivants démarrent en quelques secondes.
- Le service `demo-seed` envoie les 30 emails d'exemple au workflow n8n, attend la fin et affiche le bilan.
- **Tableau de bord** : <http://localhost:18090> — **n8n** (workflow importé et publié automatiquement) : <http://localhost:15678> (n8n demande de créer un compte local à la première visite).
- Repartir de zéro : `docker compose down -v`.

Ports modifiables via `DASHBOARD_PORT` et `N8N_PORT` ; tout est publié sur `127.0.0.1` uniquement.

## Résultat (mode démo)

Mesure sur le jeu d'exemples (30 emails FR/EN fictifs), identique en mémoire (`eval`) et via le workflow n8n réel :

| Tranché par | Emails | Part |
|---|---:|---:|
| Règles | 9 | 30 % |
| Jev (confiance > 0,85) | 11 | 37 % |
| Claude (incertain ou sensible) | 10 | 33 % |
| **Total** | **30** | |

- 30/30 catégories conformes à l'attendu, 6 emails marqués « à vérifier », 8 alertes envoyées.
- Le tableau de bord affiche aussi un **coût d'API estimé** par email (tarifs publics des API Jev et Anthropic, tokens estimés à 4 caractères par token). Les hypothèses sont dans `gateway/src/config.rs` et réglables par variables d'environnement.

**À lire honnêtement :**

- En mode démo, **Jev et Claude sont simulés** par des heuristiques déterministes calibrées sur ce jeu d'exemples. Le 30/30 prouve que la mécanique (routage, seuil, garde-fous, alertes, stockage) fonctionne, **pas** la qualité réelle des modèles. Pour la mesurer : `TRIAGE_MODE=live` sur un échantillon étiqueté de la PME (voir plus bas).
- L'essentiel de l'économie vient des **règles** : tout ce qu'elles tranchent ne coûte rien. Le seuil de Jev et la liste des règles se règlent après mesure sur de vrais emails.
- Le prompt système porte un `cache_control`, mais il fait ≈ 650 tokens, sous le minimum de mise en cache de Sonnet 5 (1 024) : il ne sera mis en cache que si vous l'enrichissez (exemples propres à la PME, par exemple).

## Configuration réelle

Copiez `.env.example` en `.env` (jamais versionné) et remplissez :

| Variable | Rôle |
|---|---|
| `TRIAGE_MODE` | `demo` (défaut) ou `live` (appels facturés) |
| `JEV_API_KEY` | clé Jev (iajev.com) |
| `JEV_CONFIDENCE_THRESHOLD` | seuil de la voie directe Jev (défaut 0,85, strictement supérieur) |
| `ANTHROPIC_API_KEY` | clé de l'API Claude |
| `CLAUDE_MODEL` | `claude-sonnet-5` (défaut) ou `claude-opus-5-5` |
| `CLAUDE_EFFORT` | `low` (défaut), `medium`, `high` |
| `ALERT_WEBHOOK_URL` | webhook d'alerte (Slack, Teams, ntfy, autre workflow n8n…) |
| `POSTGRES_PASSWORD` | mot de passe de la base (à changer hors démo) |

Les clés ne sont lues **que par la passerelle** : n8n ne les voit jamais.

**Boîte mail.** Dans n8n, ouvrez le workflow, ajoutez un identifiant IMAP sur le nœud « Boîte mail IMAP (à configurer) » (pour Gmail : IMAP activé et mot de passe d'application) puis activez le nœud. Le nœud « Gmail : appliquer le libellé » est fourni désactivé ; il demande un identifiant Gmail OAuth.

**Règles.** `config/rules.json` (monté en lecture seule, modifiable sans recompiler) : expéditeurs connus, fournisseurs dont les factures PDF sont classées d'office, motifs sensibles.

**Mesurer la qualité réelle avant d'activer.** Remplacez `samples/emails.json` par un échantillon étiqueté (bloc `expected`) puis :

```bash
cd gateway
TRIAGE_MODE=live JEV_API_KEY=… ANTHROPIC_API_KEY=… \
  RULES_FILE=../config/rules.json SAMPLES_FILE=../samples/emails.json \
  cargo run --release -- eval --live-costs-money
```

## Ce qu'il y a dans le dépôt

```
docker-compose.yml        postgres + passerelle + n8n + demo-seed (one-shot)
workflows/                workflow n8n exporté (JSON versionné, sans identifiants)
n8n/start-n8n.sh          import + publication du workflow au démarrage
gateway/                  passerelle Rust (axum) : règles, Jev, Claude, API, tableau de bord
  src/demo.rs             simulateurs du mode démo (isolés du code réel)
  tests/demo_dataset.rs   jeu d'exemples → catégories, routage, urgence attendus
config/rules.json         règles déterministes
samples/emails.json       30 emails fictifs FR/EN (domaines .example)
db/schema.sql             table, alertes, vue triage_summary
scripts/test.sh           fmt + clippy + tests + mesure (sans Docker)
scripts/e2e.sh            bout en bout via docker compose
.github/workflows/ci.yml  CI GitHub Actions
```

Pourquoi une passerelle Rust plutôt que tout dans n8n ? n8n orchestre (déclencheurs, embranchements, alertes, visibilité pour le client) ; la logique de décision vit dans un service typé, testé et qui détient seul les clés. Rust parce que c'est un service qui tourne en continu : binaire de quelques Mo, mémoire stable.

## Tests

```bash
scripts/test.sh   # sans Docker : fmt, clippy, 12 tests, mesure coût/qualité
scripts/e2e.sh    # docker compose : 30/30 via n8n, routage 9/11/10, 8 alertes, puis down -v
```

Si votre Docker n'a pas de DNS pendant les builds : `BUILD_NETWORK=host scripts/e2e.sh`.

## Limites

- Mode démo : simulateurs, pas de mesure de qualité réelle (voir plus haut).
- Coûts estimés (tokens ≈ caractères / 4, taux de change fixé) ; en mode réel, les crédits Jev et l'usage Claude viennent des réponses des API.
- Nœuds IMAP et Gmail non testés contre une vraie boîte dans ce dépôt ; la normalisation IMAP vise le format « resolved » de n8n.
- Pièces jointes : seuls les noms sont utilisés, pas le contenu des PDF.
- Pas d'interface de correction : un humain corrige dans sa messagerie ; la table garde la décision (un email déjà traité n'est jamais retraité).
- `N8N_BLOCK_ENV_ACCESS_IN_NODE=false` permet au workflow de lire `GATEWAY_URL` et `ALERT_WEBHOOK_URL` ; aucune clé n'est dans l'environnement de n8n.
- Jev limite le débit (HTTP 429) : les nœuds Jev et Claude traitent 2 emails toutes les 0,5 s, et une erreur Jev renvoie le cas à Claude plutôt que d'échouer.

## D'où ça vient

Le principe a d'abord été testé en lecture seule sur une quarantaine d'emails réels d'une boîte personnelle : aucun email déplacé, l'administratif jamais classé d'office, l'authentification de l'expéditeur (DMARC) vérifiée avant tout classement automatique. Aucune de ces données n'est dans ce dépôt ; la version publique tourne uniquement sur des emails inventés.

## Auteur

Gabriel Bernal ([Mr-yums](https://github.com/Mr-yums)), développeur freelance à Bordeaux — [LinkedIn](https://fr.linkedin.com/in/gabriel-bernal-b37a0b18a).

Licence [MIT](LICENSE).

---

## English summary

**Problem.** Small businesses drown in mixed inbound email (invoices, quote requests, support, job applications, admin, cold outreach, fraud attempts). Sending everything to a large model is expensive and hands human decisions to an AI.

**Solution.** An n8n workflow with a cost cascade: deterministic rules first (free) → **Jev** typed decision (category + probabilities) when confidence is strictly above 0.85 → **Claude** (`claude-sonnet-5` by default) for uncertain or sensitive cases, producing a summary, a proposed action and a reply draft. Sensitive content (bank details, legal, health, GDPR, fraud) goes straight to Claude. Results go to Postgres, a webhook alert fires for urgent or needs-review items, and nothing is ever sent automatically. A Rust gateway holds the API keys and the decision logic; n8n orchestrates.

**Result (demo mode, 30 fictional emails).** Rules 9, Jev 11, Claude 10; 30/30 expected categories, 6 flagged for human review, 8 alerts. In demo mode Jev and Claude are **deterministic simulators** calibrated on this dataset: this proves the mechanics, not model quality.

**Run it.** `git clone … && cd n8n-email-triage && docker compose up --build` — dashboard on <http://localhost:18090>, n8n on <http://localhost:15678>. No keys needed. For real use, copy `.env.example` to `.env`, set `TRIAGE_MODE=live`, `JEV_API_KEY`, `ANTHROPIC_API_KEY`, and configure the IMAP node in n8n.

MIT licensed.
