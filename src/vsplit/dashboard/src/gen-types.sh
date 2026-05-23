#!/usr/bin/env bash
# Boot vsplit dashboard in a temp working root, dump openapi.json,
# and convert to TypeScript types.
set -euo pipefail

cd "$(dirname "$0")"
TMP=$(mktemp -d)
trap 'kill $PID 2>/dev/null; rm -rf "$TMP"' EXIT

# Use uv from project root
(cd ../../../.. && uv run vsplit -w "$TMP" dashboard --no-open --port 18765 &)
PID=$!
sleep 2
curl -s http://127.0.0.1:18765/openapi.json > openapi.json
bunx openapi-typescript openapi.json -o src/lib/api.types.ts
echo "regenerated src/lib/api.types.ts"
