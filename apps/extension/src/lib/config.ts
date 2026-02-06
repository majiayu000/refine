const DEFAULT_CLOUD_API_BASE = 'http://localhost:8787'
declare const process: { env: Record<string, string | undefined> }

export const OUTBOX_FLUSH_ALARM = 'flushOutbox'
export const RESET_DAILY_STATS_ALARM = 'resetDailyStats'

export const OUTBOX_FLUSH_INTERVAL_MINUTES = 2
export const RESET_DAILY_STATS_INTERVAL_MINUTES = 60 * 24

export const RETRY_BASE_DELAY_MS = 5_000
export const RETRY_MAX_DELAY_MS = 30 * 60 * 1_000

function normalizeBaseUrl(url: string): string {
  return url.replace(/\/+$/, '')
}

export function getCloudApiBase(): string {
  const envBase = process.env.PLASMO_PUBLIC_REFINE_API_BASE?.trim()
  if (!envBase) return DEFAULT_CLOUD_API_BASE
  return normalizeBaseUrl(envBase)
}
