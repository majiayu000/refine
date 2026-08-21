const DEFAULT_CLOUD_API_BASE = 'http://localhost:21567'
declare const process: { env: Record<string, string | undefined> }

export const OUTBOX_FLUSH_ALARM = 'flushOutbox'
export const RESET_DAILY_STATS_ALARM = 'resetDailyStats'

export const OUTBOX_FLUSH_INTERVAL_MINUTES = 2
export const RESET_DAILY_STATS_INTERVAL_MINUTES = 60 * 24

export const RETRY_BASE_DELAY_MS = 5_000
export const RETRY_MAX_DELAY_MS = 30 * 60 * 1_000

/** Must match FALLBACK_PORTS in server/src/main.rs */
const CANDIDATE_PORTS = [21567, 21568, 21569, 21570]
const DISCOVERY_CACHE_KEY = 'refine_server_port'
const DISCOVERY_CACHE_TTL_MS = 5 * 60 * 1_000 // 5 minutes
export const API_TOKEN_STORAGE_KEY = 'refine_api_token'

function normalizeBaseUrl(url: string): string {
  return url.replace(/\/+$/, '')
}

export async function readApiToken(): Promise<string> {
  const stored = await chrome.storage.local.get(API_TOKEN_STORAGE_KEY)
  const value = stored[API_TOKEN_STORAGE_KEY]
  return typeof value === 'string' ? value.trim() : ''
}

export async function setApiToken(token: string): Promise<boolean> {
  const normalized = token.trim()
  if (normalized && !/^[!-~]+$/.test(normalized)) {
    throw new Error('API token must contain visible ASCII characters only')
  }
  if (normalized) {
    await chrome.storage.local.set({ [API_TOKEN_STORAGE_KEY]: normalized })
    return true
  }
  await chrome.storage.local.remove(API_TOKEN_STORAGE_KEY)
  return false
}

/**
 * Probe candidate ports to find the running refine-server.
 * Caches the result in chrome.storage.local for DISCOVERY_CACHE_TTL_MS.
 */
export async function discoverCloudApiBase(): Promise<string> {
  // Env override skips discovery entirely
  const envBase = process.env.PLASMO_PUBLIC_REFINE_API_BASE?.trim()
  if (envBase) return normalizeBaseUrl(envBase)

  // Check cache first
  try {
    const cached = await chrome.storage.local.get(DISCOVERY_CACHE_KEY)
    const entry = cached[DISCOVERY_CACHE_KEY]
    if (entry && Date.now() - entry.ts < DISCOVERY_CACHE_TTL_MS) {
      return `http://localhost:${entry.port}`
    }
  } catch {
    // storage unavailable — fall through to probe
  }

  // Probe candidates
  for (const port of CANDIDATE_PORTS) {
    try {
      const resp = await fetch(`http://localhost:${port}/health`, {
        signal: AbortSignal.timeout(500),
      })
      if (resp.ok) {
        const body = await resp.text()
        if (body.includes('Refine cloud API')) {
          // Cache the discovered port
          try {
            await chrome.storage.local.set({
              [DISCOVERY_CACHE_KEY]: { port, ts: Date.now() },
            })
          } catch {
            // non-critical
          }
          return `http://localhost:${port}`
        }
      }
    } catch {
      // Port unreachable, try next
    }
  }

  // All probes failed — return default, caller will handle the error
  return DEFAULT_CLOUD_API_BASE
}

/** Sync getter for backward compat — returns default or env override only. */
export function getCloudApiBase(): string {
  const envBase = process.env.PLASMO_PUBLIC_REFINE_API_BASE?.trim()
  if (!envBase) return DEFAULT_CLOUD_API_BASE
  return normalizeBaseUrl(envBase)
}
