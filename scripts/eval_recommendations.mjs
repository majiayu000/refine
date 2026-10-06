#!/usr/bin/env node

import fs from 'node:fs/promises'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'

const DEFAULT_BASE_URL = process.env.REFINE_API_BASE || 'http://127.0.0.1:21567'
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
      if ('relevant_ids' in parsed && (!Array.isArray(parsed.relevant_ids) || parsed.relevant_ids.some((id) => typeof id !== 'string' || !id.trim()))) {
        throw new Error('relevant_ids must be an array of non-empty item IDs')
      }
      return {
        id: String(parsed.id || `line-${index + 1}`),
        query: String(parsed.query || ''),
        kind: Array.isArray(parsed.relevant_ids) ? 'gold' : 'metadata_proxy',
        relevantIds: Array.isArray(parsed.relevant_ids) ? [...new Set(parsed.relevant_ids.map(String))] : [],
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

function metadataMatches(item, expectedType, expectedTags) {
  if (!item || typeof item !== 'object') return false
  const typeMatches = typeof item.item_type === 'string' && item.item_type === expectedType
  if (!typeMatches) return false

  if (!expectedTags.length) return true
  const itemTags = Array.isArray(item.tags)
    ? item.tags.map((tag) => String(tag).toLowerCase())
    : []
  return expectedTags.some((tag) => itemTags.includes(tag))
}

/** Exact relevance judgments: shared tags or types cannot count as a gold hit. */
export function evaluateHits(sample, returnedItems, limit) {
  const items = returnedItems.slice(0, limit)
  if (sample.kind === 'metadata_proxy') {
    return {
      kind: 'metadata_proxy',
      top1: metadataMatches(items[0], sample.expectedType, sample.expectedTags),
      top3: items.slice(0, 3).some((item) => metadataMatches(item, sample.expectedType, sample.expectedTags)),
    }
  }
  const relevant = new Set(sample.relevantIds)
  if (!relevant.size) {
    return { kind: 'negative', correct: items.length === 0, falsePositive: items.length > 0 }
  }
  const matched = new Set()
  let firstRank = 0
  items.forEach((item, index) => {
    if (item && relevant.has(item.id)) {
      matched.add(item.id)
      if (!firstRank) firstRank = index + 1
    }
  })
  return {
    kind: 'positive',
    top1: firstRank === 1,
    top3: firstRank > 0 && firstRank <= 3,
    recall: matched.size / relevant.size,
    reciprocalRank: firstRank ? 1 / firstRank : 0,
  }
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
    if (payload?.success !== true || !Array.isArray(payload.items)) {
      return { ok: false, latencyMs: performance.now() - startedAt, error: 'Invalid recommendation response' }
    }
    return {
      ok: true,
      latencyMs: performance.now() - startedAt,
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
  const show = (value) => value === null ? 'N/A (no labeled samples)' : formatPct(value)
  const report = `# Recommendation Evaluation Report

- Generated at: ${new Date().toISOString()}
- API: ${summary.baseUrl}
- Dataset: ${summary.dataset}
- Dataset SHA-256: ${summary.datasetSha256}
- Queries: ${summary.total}
- Successful requests: ${formatPct(summary.successRate)}
- Limit: ${summary.limit}
- Latency p95: ${summary.latencyP95.toFixed(2)} ms

## Relevance labels (exact item IDs)

- Positive queries: ${summary.goldPositiveQueries}
- Negative queries (explicit relevant_ids=[]): ${summary.goldNegativeQueries}
- Top-1 hit rate: ${show(summary.top1HitRate)}
- Top-3 hit rate: ${show(summary.top3HitRate)}
- Recall@${summary.limit}: ${show(summary.recallAtK)}
- MRR@${summary.limit}: ${summary.mrrAtK === null ? 'N/A' : summary.mrrAtK.toFixed(4)}
- Correct empty result rate on negatives: ${show(summary.negativeEmptyRate)}
- False-positive result rate on negatives: ${show(summary.negativeFalsePositiveRate)}

Failed requests count as unsuccessful judgments. Negative-query request failures are not counted as correct empty responses.

## Metadata proxy (not a relevance metric)

- Unlabeled queries with only type/tag expectations: ${summary.metadataProxyQueries}
- Proxy top-1: ${show(summary.metadataTop1Proxy)}
- Proxy top-3: ${show(summary.metadataTop3Proxy)}

Type/tag overlap alone does not establish relevance. This section cannot validate retrieval quality; add relevant_ids and explicit negative queries for that purpose.

## Failed requests

${summary.failures.length === 0 ? '- none' : summary.failures.map((f) => `- ${f.id}: ${f.error}`).join('\n')}

## Miss samples

${summary.misses.length === 0 ? '- none' : summary.misses.slice(0, 10).map((m) => `- ${m.id}: ${m.query}`).join('\n')}
`
  await fs.mkdir(path.dirname(outPath), { recursive: true })
  await fs.writeFile(outPath, report, 'utf8')
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  if (args.help) { printHelp(); return }
  const dataset = await loadDataset(args.dataset)
  if (!dataset.length) throw new Error('Dataset is empty')
  const ids = new Set()
  for (const sample of dataset) {
    if (!sample.query.trim()) throw new Error(`Empty query: ${sample.id}`)
    if (ids.has(sample.id)) throw new Error(`Duplicate query ID: ${sample.id}`)
    if (sample.kind === 'gold' && sample.relevantIds.some((id) => !id.trim())) throw new Error(`Blank relevant ID: ${sample.id}`)
    ids.add(sample.id)
  }
  const positiveCount = dataset.filter((s) => s.kind === 'gold' && s.relevantIds.length > 0).length
  const negativeCount = dataset.filter((s) => s.kind === 'gold' && s.relevantIds.length === 0).length
  const proxyCount = dataset.length - positiveCount - negativeCount
  let successes = 0, top1 = 0, top3 = 0, recall = 0, reciprocal = 0
  let negativesCorrect = 0, negativesFalsePositive = 0, proxyTop1 = 0, proxyTop3 = 0
  const latencies = [], failures = [], misses = []
  for (const sample of dataset) {
    const result = await fetchRecommendation(args.baseUrl, args.token, args.limit, args.timeoutMs, sample.query)
    latencies.push(result.latencyMs)
    if (!result.ok) {
      failures.push({ id: sample.id, error: result.error || 'unknown error' })
      continue
    }
    successes += 1
    const verdict = evaluateHits(sample, result.payload.items, args.limit)
    if (verdict.kind === 'positive') {
      top1 += Number(verdict.top1); top3 += Number(verdict.top3)
      recall += verdict.recall; reciprocal += verdict.reciprocalRank
      if (!verdict.top3) misses.push(sample)
    } else if (verdict.kind === 'negative') {
      negativesCorrect += Number(verdict.correct)
      negativesFalsePositive += Number(verdict.falsePositive)
      if (!verdict.correct) misses.push(sample)
    } else {
      proxyTop1 += Number(verdict.top1); proxyTop3 += Number(verdict.top3)
    }
  }
  const ratio = (value, count) => count ? value / count : null
  const summary = {
    baseUrl: args.baseUrl, dataset: args.dataset, limit: args.limit,
    datasetSha256: createHash('sha256').update(await fs.readFile(args.dataset)).digest('hex'),
    total: dataset.length, successRate: successes / dataset.length,
    goldPositiveQueries: positiveCount, goldNegativeQueries: negativeCount, metadataProxyQueries: proxyCount,
    top1HitRate: ratio(top1, positiveCount), top3HitRate: ratio(top3, positiveCount),
    recallAtK: ratio(recall, positiveCount), mrrAtK: ratio(reciprocal, positiveCount),
    negativeEmptyRate: ratio(negativesCorrect, negativeCount), negativeFalsePositiveRate: ratio(negativesFalsePositive, negativeCount),
    metadataTop1Proxy: ratio(proxyTop1, proxyCount), metadataTop3Proxy: ratio(proxyTop3, proxyCount),
    latencyP95: percentile(latencies, 95), latencyAvg: latencies.reduce((sum, v) => sum + v, 0) / latencies.length,
    failures, misses,
  }
  await writeReport(args.out, summary)
  console.log(JSON.stringify(summary, null, 2))
  console.log(`Saved report: ${args.out}`)
  if (failures.length) process.exitCode = 1
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main().catch((error) => {
    console.error(`eval failed: ${error instanceof Error ? error.message : String(error)}`)
    process.exitCode = 1
  })
}
