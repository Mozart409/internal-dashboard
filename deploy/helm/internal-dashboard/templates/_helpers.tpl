{{- define "internal-dashboard.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "internal-dashboard.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{- define "internal-dashboard.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "internal-dashboard.labels" -}}
helm.sh/chart: {{ include "internal-dashboard.chart" . }}
{{ include "internal-dashboard.selectorLabels" . }}
app.kubernetes.io/version: {{ .Values.image.tag | default .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "internal-dashboard.selectorLabels" -}}
app.kubernetes.io/name: {{ include "internal-dashboard.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "internal-dashboard.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "internal-dashboard.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
Fail unless the chart runs highly available: at least 2 replicas, and an HPA
that never scales below 2.
*/}}
{{- define "internal-dashboard.assertHA" -}}
{{- if lt (int .Values.replicaCount) 2 }}
{{- fail (printf "replicaCount is %d: this chart is HA only, set replicaCount to 2 or more" (int .Values.replicaCount)) }}
{{- end }}
{{- if and .Values.autoscaling.enabled (lt (int .Values.autoscaling.minReplicas) 2) }}
{{- fail (printf "autoscaling.minReplicas is %d: set it to 2 or more" (int .Values.autoscaling.minReplicas)) }}
{{- end }}
{{- end }}

{{/* DATABASE_URL from the database Secret, as a list item */}}
{{- define "internal-dashboard.databaseUrlEnv" -}}
- name: DATABASE_URL
  valueFrom:
    secretKeyRef:
      name: {{ required "database.existingSecret is required" .Values.database.existingSecret }}
      key: {{ .Values.database.secretKey }}
{{- with .Values.database.listenSecretKey }}
- name: DATABASE_LISTEN_URL
  valueFrom:
    secretKeyRef:
      name: {{ $.Values.database.existingSecret }}
      key: {{ . }}
{{- end }}
{{- end }}

{{/*
MCP_ALLOWED_HOSTS: the httpRoute hostnames (when enabled), the in-cluster
Service names and mcp.extraAllowedHosts, comma-joined.
*/}}
{{- define "internal-dashboard.mcpAllowedHosts" -}}
{{- $fullname := include "internal-dashboard.fullname" . }}
{{- $hosts := list }}
{{- if .Values.httpRoute.enabled }}
{{- $hosts = concat $hosts .Values.httpRoute.hostnames }}
{{- end }}
{{- $hosts = concat $hosts (list $fullname (printf "%s.%s" $fullname .Release.Namespace) (printf "%s.%s.svc" $fullname .Release.Namespace) (printf "%s.%s.svc.cluster.local" $fullname .Release.Namespace)) }}
{{- $hosts = concat $hosts .Values.mcp.extraAllowedHosts }}
{{- join "," $hosts }}
{{- end }}
