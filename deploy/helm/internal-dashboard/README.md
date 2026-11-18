# internal-dashboard Helm chart

Deploys `internal-dashboard` (UI, SSE at `/events`, REST `/api/v1`, MCP `/mcp`)
to Kubernetes, highly available.

## What HA means here

- At least 2 replicas (`replicaCount: 2`); the templates fail below that, also
  for an HPA `minReplicas`.
- One shared Postgres is the only state. Every replica runs the migrations at
  start (safe concurrently) and holds one `LISTEN` connection.
- Changes fan out to all replicas over Postgres `LISTEN/NOTIFY`, so a link
  edited through one pod reaches the SSE streams of every pod.
- The MCP endpoint is stateless: any replica serves any request, no session
  affinity.
- A PodDisruptionBudget (`minAvailable: 1`), a soft spread and a preferred
  anti-affinity over nodes, and a rolling update with `maxUnavailable: 0`.
- Graceful drain: `preStop` sleep so the endpoint leaves the Services, then
  SIGTERM turns `/health/ready` to 503 and closes the SSE streams, all within
  `terminationGracePeriodSeconds`.

The app has no authentication. Put it behind the Gateway with an auth proxy.

## Install

Create the Secret with the Postgres URL (size Postgres `max_connections` for
replicas x (`database.maxConnections` + 1)):

```sh
kubectl create secret generic internal-dashboard-db \
  --from-literal=database-url='postgres://user:PASSWORD@postgres:5432/dashboard'
helm install internal-dashboard ./deploy/helm/internal-dashboard \
  --set database.existingSecret=internal-dashboard-db \
  --set httpRoute.enabled=true \
  --set 'httpRoute.hostnames={dashboard.homelab.internal}' \
  --set 'httpRoute.parentRefs[0].name=public-gateway' \
  --set 'httpRoute.parentRefs[0].namespace=gateway-system'
```

The httpRoute hostnames and the in-cluster Service names are added to
`MCP_ALLOWED_HOSTS` automatically; use `mcp.extraAllowedHosts` for others.

Validate the chart with `just dashboard-deploy-check`.
