#!/usr/bin/env node

import fs from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'

const DEFAULT_BASE_URL = process.env.REFINE_API_BASE || 'http://localhost:8787'
const DEFAULT_DATASET = 'docs/eval/recommendation_queries.jsonl'
const DEFAULT_LIMIT = 5
const DEFAULT_TIMEOUT_MS = 1_500
const DEFAULT_OUT = 'docs/eval/recommendation_eval_latest.md'

function parseArgs(argv) {
  const args = {
    baseUrl: DEFAULT_BASE_URL,
    dataset: DEFAULT_DATASET,
    token: process.env.REFINE_API_TOKEN || '',
    limit: DEFAULT_LIMIT,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    out: DEFAULT_OUT,
    help: false,
  }

  for (let i = 0; i < argv.length; i += 1) {
    const current = argv[i]
    const next = argv[i + 1]
    if (current === '--help' || current === '-h') {
      args.help = true
      continue
    }
    if (current === '--base-url' && next) {
      args.baseUrl = next
      i += 1
      continue
    }
    if (current === '--dataset' && next) {
      args.dataset = next
      i += 1
      continue
    }
    if (current === '--token' && next) {
      args.token = next
      i += 1
      continue
    }
    if (current === '--limit' && next) {
      args.limit = Number.parseInt(next, 10) || DEFAULT_LIMIT
      i += 1
      continue
    }
    if (current === '--timeout-ms' && next) {
      args.timeoutMs = Number.parseInt(next, 10) || DEFAULT_TIMEOUT_MS
      i += 1
      continue
    }
    if (current === '--out' && next) {
      args.out = next
      i += 1
    }
  }

  return args
}

function printHelp() {
  console.log(`
Usage:
  node scripts/eval_recommendations.mjs [options]

Options:
  --base-url <url>      API base URL (default: ${DEFAULT_BASE_URL})
  --dataset <path>      JSONL dataset path (default: ${DEFAULT_DATASET})
  --token <token>       Optional bearer token (default: REFINE_API_TOKEN)
  --limit <n>           Recommendation list limit (default: ${DEFAULT_LIMIT})
  --timeout-ms <n>      Per request timeout ms (default: ${DEFAULT_TIMEOUT_MS})
  --out <path>          Markdown report path (default: ${DEFAULT_OUT})
  -h, --help            Show help
`)
}

async function loadDataset(datasetPath) {
  const raw = await fs.readFile(datasetPath, 'utf8')
  const lines = raw
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)

  return lines.map((line, index) => {
    try {
      const parsed = JSON.parse(line)
      return {
        id: String(parsed.id || `line-${index + 1}`),
        query: String(parsed.query || ''),
        expectedType: String(parsed.expected_type || ''),
        expectedTags: Array.isArray(parsed.expected_tags)
          ? parsed.expected_tags.map((tag) => String(tag).toLowerCase())
          : [],
      }
    } catch (error) {
      throw new Error(`Invalid JSONL at line ${index + 1}: ${String(error)}`)
    }
  })
}

function percentile(values, p) {
  if (!values.length) return 0
  const sorted = [...values].sort((a, b) => a - b)
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)
  return sorted[Math.max(0, idx)]
}

function hitMatches(item, expectedType, expectedTags) {
  if (!item || typeof item !== 'object') return false
  const typeMatches = typeof item.item_type === 'string' && item.item_type === expectedType
  if (!typeMatches) return false

  if (!expectedTags.length) return true
  const itemTags = Array.isArray(item.tags)
    ? item.tags.map((tag) => String(tag).toLowerCase())
    : []
  return expectedTags.some((tag) => itemTags.includes(tag))
}

async function fetchRecommendation(baseUrl, token, limit, timeoutMs, query) {
  const endpoint = new URL('/v1/recommendations', baseUrl)
  endpoint.searchParams.set('q', query)
  endpoint.searchParams.set('limit', String(limit))

  const headers = {
    'X-Refine-Client': 'eval-script',
  }
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }

  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs)
  const startedAt = performance.now()

  try {
    const response = await fetch(endpoint.toString(), {
      method: 'GET',
      headers,
      signal: controller.signal,
    })
    const latencyMs = performance.now() - startedAt
    if (!response.ok) {
      return {
        ok: false,
        latencyMs,
        error: `HTTP ${response.status}`,
      }
    }

    const payload = await response.json()
    return {
      ok: true,
      latencyMs,
      payload,
    }
  } catch (error) {
    return {
      ok: false,
      latencyMs: performance.now() - startedAt,
      error: error instanceof Error ? error.message : String(error),
    }
  } finally {
    clearTimeout(timeoutId)
  }
}

function formatPct(value) {
  return `${(value * 100).toFixed(2)}%`
}

async function writeReport(outPath, summary) {
  const report = `# Recommendation Eval Report

- generated_at: ${new Date().toISOString()}
- base_url: ${summary.baseUrl}
- dataset: ${summary.dataset}
- total_queries: ${summary.total}
- request_success_rate: ${formatPct(summary.successRate)}
- top1_hit_rate: ${formatPct(summary.top1HitRate)}
- top3_hit_rate: ${formatPct(summary.top3HitRate)}
- latency_p95_ms: ${summary.latencyP95.toFixed(2)}
- latency_avg_ms: ${summary.latencyAvg.toFixed(2)}

## Failed Requests

${summary.failures.length === 0 ? '- none' : summary.failures.map((f) => `- ${f.id}: ${f.error}`).join('\n')}

## Miss Samples (Top 10)

${summary.misses.length === 0 ? '- none' : summary.misses.slice(0, 10).map((m) => `- ${m.id}: ${m.query}`).join('\n')}
`

  await fs.mkdir(path.dirname(outPath), { recursive: true })
  await fs.writeFile(outPath, report, 'utf8')
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  if (args.help) {
    printHelp()
    return
  }

  const dataset = await loadDataset(args.dataset)
  if (!dataset.length) {
    throw new Error('Dataset is empty')
  }

  let successCount = 0
  let top1Hits = 0
  let top3Hits = 0
  const latencies = []
  const failures = []
  const misses = []

  for (const sample of dataset) {
    const result = await fetchRecommendation(
      args.baseUrl,
      args.token,
      args.limit,
      args.timeoutMs,
      sample.query
    )

    if (!result.ok) {
      failures.push({
        id: sample.id,
        error: result.error || 'unknown error',
      })
      continue
    }

    successCount += 1
    latencies.push(result.latencyMs)

    const payload = result.payload
    const items = Array.isArray(payload?.items) ? payload.items : []
    const top1 = items[0]
    const top3 = items.slice(0, 3)

    const top1Matched = hitMatches(top1, sample.expectedType, sample.expectedTags)
    const top3Matched = top3.some((item) =>
      hitMatches(item, sample.expectedType, sample.expectedTags)
    )

    if (top1Matched) top1Hits += 1
    if (top3Matched) top3Hits += 1

    if (!top3Matched) {
      misses.push({
        id: sample.id,
        query: sample.query,
      })
    }
  }

  const total = dataset.length
  const summary = {
    baseUrl: args.baseUrl,
    dataset: args.dataset,
    total,
    successRate: successCount / total,
    top1HitRate: top1Hits / total,
    top3HitRate: top3Hits / total,
    latencyP95: percentile(latencies, 95),
    latencyAvg: latencies.length
      ? latencies.reduce((sum, value) => sum + value, 0) / latencies.length
      : 0,
    failures,
    misses,
  }

  await writeReport(args.out, summary)
  console.log(JSON.stringify(summary, null, 2))
  console.log(`Saved report: ${args.out}`)
}

main().catch((error) => {
  console.error(`eval failed: ${error instanceof Error ? error.message : String(error)}`)
  process.exitCode = 1
})
