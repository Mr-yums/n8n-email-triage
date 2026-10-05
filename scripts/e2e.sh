#!/usr/bin/env bash
# Test de bout en bout : lance la pile en mode démo, attend que n8n ait traité le jeu
# d'exemples, vérifie les résultats via l'API, puis arrête tout (volumes compris).
# Options : E2E_KEEP=1 pour laisser la pile tournante ; BUILD_NETWORK=host si le build n'a pas de DNS.
set -euo pipefail
cd "$(dirname "$0")/.."
export TRIAGE_MODE=demo
PORT="${DASHBOARD_PORT:-18090}"
PROJECT="n8n-email-triage-e2e"
compose() { docker compose -p "$PROJECT" "$@"; }
cleanup() { [[ "${E2E_KEEP:-0}" == 1 ]] || compose down -v --remove-orphans >/dev/null 2>&1 || true; }
trap cleanup EXIT

if [[ -n "${BUILD_NETWORK:-}" ]]; then
  docker build --network "$BUILD_NETWORK" -f gateway/Dockerfile -t n8n-email-triage-gateway:0.1.0 .
else
  compose build gateway
fi
compose up -d

echo "Attente de la fin du seed (traitement par n8n)…"
for _ in $(seq 1 100); do
  status=$(docker inspect -f '{{.State.Status}} {{.State.ExitCode}}' "$(compose ps -aq demo-seed)" 2>/dev/null || echo "absent")
  [[ "$status" == exited* ]] && break
  sleep 3
done
compose logs demo-seed
[[ "$status" == "exited 0" ]] || { echo "ÉCHEC : seed $status"; compose logs n8n | tail -40; exit 1; }

stats=$(curl -fsS "http://127.0.0.1:${PORT}/api/stats")
echo "$stats"
python3 - "$stats" <<'PY'
import json, sys
s = json.loads(sys.argv[1])
assert s["mode"] == "demo", s
assert s["total"] == 30, f"30 emails attendus, {s['total']} enregistrés"
assert s["matching_expected"] == s["with_expected"] == 30, "catégories non conformes"
assert s["by_route"] == {"regle": 9, "jev": 11, "claude": 10}, s["by_route"]
assert s["alerts_received"] == 8, f"8 alertes attendues, {s['alerts_received']} reçues"
print("OK : 30/30 emails triés par le workflow n8n, routage et alertes conformes.")
PY
curl -fsS "http://127.0.0.1:${PORT}/" | grep -q "Tri automatique des emails"
echo "OK : tableau de bord servi."
