#!/usr/bin/env bash
# Validate the Helm chart. Used by `just dashboard-deploy-check` and by CI.
# kubeconform downloads the Kubernetes JSON schemas from GitHub, so this needs
# network. CRD kinds (HTTPRoute) have no schema and are skipped. Also checks the
# chart version/appVersion match the crate version and that the chart stays HA.
#
# Usage: scripts/chart-check.sh [<version>]
# With <version>, the crate, chart version and appVersion must all equal it
# (the release workflow passes the tag without its leading `v`).
set -euo pipefail
cd "$(dirname "$0")/.."

want="${1:-}"
chart=deploy/helm/internal-dashboard
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "== chart version matches the crate"
crate="$(grep -m1 '^version = ' Cargo.toml | sed 's/^version = "\(.*\)"$/\1/')"
chartv="$(grep -m1 '^version: ' "$chart/Chart.yaml" | sed 's/^version: //')"
appv="$(grep -m1 '^appVersion: ' "$chart/Chart.yaml" | sed 's/^appVersion: "\(.*\)"$/\1/')"
if [ "$crate" != "$chartv" ] || [ "$crate" != "$appv" ]; then
    echo "Chart.yaml version ($chartv) / appVersion ($appv) != crate version ($crate)." >&2
    echo "The versions are kept in sync by \`cog bump\` (cog.toml pre_bump_hooks) in the standalone repo and \`just release-bump internal-dashboard <version>\` in yggdrasil; otherwise edit version and appVersion in $chart/Chart.yaml by hand to match." >&2
    exit 1
fi
if [ -n "$want" ] && [ "$crate" != "$want" ]; then
    echo "Crate, chart version and appVersion are $crate, but the release version is $want." >&2
    exit 1
fi

echo "== helm lint"
helm lint "$chart" -f "$chart/ci/minimal-values.yaml"
helm lint "$chart" -f "$chart/ci/all-on-values.yaml"

echo "== helm template + kubeconform"
helm template dashboard "$chart" -f "$chart/ci/minimal-values.yaml" > "$tmp/minimal.yaml"
helm template dashboard "$chart" -f "$chart/ci/all-on-values.yaml" > "$tmp/all-on.yaml"
kubeconform -strict -summary -ignore-missing-schemas "$tmp/minimal.yaml"
kubeconform -strict -summary -ignore-missing-schemas "$tmp/all-on.yaml"

echo "== HA assertions"
grep -q '^  replicas: 2$' "$tmp/minimal.yaml"
grep -q '^kind: PodDisruptionBudget$' "$tmp/minimal.yaml"
if helm template dashboard "$chart" -f "$chart/ci/minimal-values.yaml" --set replicaCount=1 > /dev/null 2>&1; then
    echo "replicaCount=1 rendered; the chart must refuse fewer than 2 replicas." >&2
    exit 1
fi
