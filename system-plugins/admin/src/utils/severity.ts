export type Severity = 'success' | 'warn' | 'danger' | 'info' | 'secondary' | 'contrast'

const statusSeverityMap: Record<string, Severity> = {
  enabled: 'success',
  active: 'success',
  healthy: 'success',
  applied: 'success',
  disabled: 'warn',
  unknown: 'warn',
  pending: 'warn',
  error: 'danger',
  unhealthy: 'danger',
  failed: 'danger',
  basic: 'info',
  bearer: 'info',
  none: 'secondary',
}

export function mapSeverity(status: string): Severity {
  return statusSeverityMap[status.toLowerCase()] ?? 'info'
}
