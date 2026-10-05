#!/bin/sh
# Importe (ou met à jour) le workflow versionné, le publie, puis démarre n8n normalement.
set -eu
WORKFLOW=/opt/triage/workflows/email-triage.json
WORKFLOW_ID=EmailTriageDemo1

n8n import:workflow --input="$WORKFLOW"
n8n publish:workflow --id="$WORKFLOW_ID"
exec /docker-entrypoint.sh start
