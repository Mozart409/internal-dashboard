#!/usr/bin/env bash
# Start a built image against a real Postgres and check that it serves. Used by
# the release workflow before an image is uploaded or published. Needs docker
# and curl; the app is published on 127.0.0.1:3000 while the script runs.
#
# Usage: scripts/image-smoke.sh <image-ref>
# e.g.   scripts/image-smoke.sh internal-dashboard:latest
set -euo pipefail

image="${1:?usage: scripts/image-smoke.sh <image-ref>}"
net=dashboard-smoke
db=dashboard-smoke-db
app=dashboard-smoke-app

cleanup() {
    docker rm -f "$db" "$app" >/dev/null 2>&1 || true
    docker network rm "$net" >/dev/null 2>&1 || true
}
fail() {
    echo "smoke test failed: $*" >&2
    if docker container inspect "$app" >/dev/null 2>&1; then
        docker logs "$app" >&2 2>&1 || true
    fi
    exit 1
}
trap cleanup EXIT
cleanup

docker network create "$net" >/dev/null
docker run -d --name "$db" --network "$net" \
    -e POSTGRES_USER=dashboard \
    -e POSTGRES_PASSWORD=dashboard \
    -e POSTGRES_DB=dashboard \
    docker.io/library/postgres:18.4 >/dev/null

# TCP, so the init-only server (socket, no TCP listener) does not count.
db_ready=0
for _ in $(seq 60); do
    if docker exec "$db" pg_isready -h 127.0.0.1 -U dashboard >/dev/null 2>&1; then
        db_ready=1
        break
    fi
    sleep 1
done
[ "$db_ready" = 1 ] || fail "postgres did not accept connections within 60s"

docker run -d --name "$app" --network "$net" -p 127.0.0.1:3000:3000 \
    -e DATABASE_URL=postgres://dashboard:dashboard@"$db":5432/dashboard \
    "$image" >/dev/null

ready=0
for _ in $(seq 60); do
    if [ "$(docker inspect -f '{{.State.Running}}' "$app" 2>/dev/null)" != true ]; then
        fail "container exited before /health/ready answered"
    fi
    if curl -fsS -o /dev/null http://127.0.0.1:3000/health/ready; then
        ready=1
        break
    fi
    sleep 1
done
[ "$ready" = 1 ] || fail "/health/ready did not answer within 60s"

curl -fsS -o /dev/null http://127.0.0.1:3000/health/live \
    || fail "/health/live did not answer"
curl -fsS -o /dev/null http://127.0.0.1:3000/ \
    || fail "/ did not answer"

echo "smoke test passed: $image"
