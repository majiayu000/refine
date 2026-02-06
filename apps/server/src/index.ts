interface ConversationRecord {
  id: string
  userId: string
  source: string
  url: string
  title?: string
  rawContent: string
  capturedAt: string
  createdAt: string
  status: 'queued' | 'processed'
  idempotencyKey: string
}

interface ExtractionJobRecord {
  id: string
  conversationId: string
  mode: 'auto' | 'knowledge' | 'skill' | 'snippet'
  status: 'pending' | 'running' | 'succeeded' | 'failed'
  createdAt: string
  updatedAt: string
  error?: string
}

interface ItemRecord {
  id: string
  conversationId: string
  title: string
  summary: string
  content: string
  source: string
  createdAt: string
}

interface ConversationPayload {
  content?: string
  url?: string
  source?: string
  title?: string
  captured_at?: string
  idempotency_key?: string
}

interface CreateExtractionJobPayload {
  conversation_id?: string
  mode?: 'auto' | 'knowledge' | 'skill' | 'snippet'
}

const conversations = new Map<string, ConversationRecord>()
const conversationsByIdempotency = new Map<string, string>()
const extractionJobs = new Map<string, ExtractionJobRecord>()
const items: ItemRecord[] = []

function json(status: number, payload: unknown, origin?: string): Response {
  const headers = new Headers({
    'Content-Type': 'application/json',
  })

  if (origin && isAllowedOrigin(origin)) {
    headers.set('Access-Control-Allow-Origin', origin)
    headers.set('Vary', 'Origin')
    headers.set('Access-Control-Allow-Methods', 'GET,POST,OPTIONS')
    headers.set('Access-Control-Allow-Headers', 'Content-Type, Authorization, X-Refine-Client')
  }

  return new Response(JSON.stringify(payload), {
    status,
    headers,
  })
}

function isAllowedOrigin(origin: string): boolean {
  return (
    origin.startsWith('chrome-extension://') ||
    origin.startsWith('moz-extension://') ||
    origin.startsWith('http://localhost:') ||
    origin.startsWith('http://127.0.0.1:')
  )
}

function getUserId(req: Request): string | null {
  const expectedToken = process.env.REFINE_API_TOKEN?.trim()
  if (!expectedToken) {
    return 'dev-user'
  }

  const authorization = req.headers.get('authorization') || ''
  const token = authorization.replace(/^Bearer\s+/i, '').trim()
  if (!token || token !== expectedToken) {
    return null
  }

  return 'token-user'
}

function createItemFromConversation(record: ConversationRecord): ItemRecord {
  const plainContent = record.rawContent.trim()
  const summary =
    plainContent.length > 160 ? `${plainContent.slice(0, 160).trim()}...` : plainContent

  return {
    id: crypto.randomUUID(),
    conversationId: record.id,
    title: record.title || `[${record.source}] ${new Date(record.createdAt).toLocaleString()}`,
    summary,
    content: plainContent,
    source: record.source,
    createdAt: new Date().toISOString(),
  }
}

function ensureExtractionJob(conversationId: string): ExtractionJobRecord {
  const now = new Date().toISOString()
  const job: ExtractionJobRecord = {
    id: crypto.randomUUID(),
    conversationId,
    mode: 'auto',
    status: 'running',
    createdAt: now,
    updatedAt: now,
  }

  extractionJobs.set(job.id, job)

  const record = conversations.get(conversationId)
  if (record) {
    const item = createItemFromConversation(record)
    items.unshift(item)
    record.status = 'processed'
    conversations.set(record.id, record)
  }

  job.status = 'succeeded'
  job.updatedAt = new Date().toISOString()
  extractionJobs.set(job.id, job)

  return job
}

async function handleCreateConversation(req: Request, origin?: string): Promise<Response> {
  const userId = getUserId(req)
  if (!userId) {
    return json(401, { success: false, message: 'Unauthorized' }, origin)
  }

  let payload: ConversationPayload
  try {
    payload = (await req.json()) as ConversationPayload
  } catch {
    return json(400, { success: false, message: 'Invalid JSON payload' }, origin)
  }

  if (!payload.content || !payload.url || !payload.source || !payload.idempotency_key) {
    return json(400, {
      success: false,
      message: 'Missing required fields: content, url, source, idempotency_key',
    }, origin)
  }

  const existingConversationId = conversationsByIdempotency.get(payload.idempotency_key)
  if (existingConversationId) {
    const existing = conversations.get(existingConversationId)
    return json(200, {
      success: true,
      conversation_id: existingConversationId,
      status: existing?.status || 'queued',
      deduplicated: true,
    }, origin)
  }

  const now = new Date().toISOString()
  const conversationId = crypto.randomUUID()
  const record: ConversationRecord = {
    id: conversationId,
    userId,
    source: payload.source,
    url: payload.url,
    title: payload.title,
    rawContent: payload.content,
    capturedAt: payload.captured_at || now,
    createdAt: now,
    status: 'queued',
    idempotencyKey: payload.idempotency_key,
  }

  conversations.set(conversationId, record)
  conversationsByIdempotency.set(payload.idempotency_key, conversationId)

  ensureExtractionJob(conversationId)

  return json(200, {
    success: true,
    conversation_id: conversationId,
    status: 'queued',
  }, origin)
}

async function handleCreateExtractionJob(req: Request, origin?: string): Promise<Response> {
  const userId = getUserId(req)
  if (!userId) {
    return json(401, { success: false, message: 'Unauthorized' }, origin)
  }

  let payload: CreateExtractionJobPayload
  try {
    payload = (await req.json()) as CreateExtractionJobPayload
  } catch {
    return json(400, { success: false, message: 'Invalid JSON payload' }, origin)
  }

  if (!payload.conversation_id) {
    return json(400, { success: false, message: 'conversation_id is required' }, origin)
  }

  if (!conversations.has(payload.conversation_id)) {
    return json(404, { success: false, message: 'Conversation not found' }, origin)
  }

  const job = ensureExtractionJob(payload.conversation_id)

  return json(200, {
    success: true,
    job_id: job.id,
    status: job.status,
  }, origin)
}

function handleGetExtractionJob(id: string, req: Request, origin?: string): Response {
  const userId = getUserId(req)
  if (!userId) {
    return json(401, { success: false, message: 'Unauthorized' }, origin)
  }

  const job = extractionJobs.get(id)
  if (!job) {
    return json(404, { success: false, message: 'Job not found' }, origin)
  }

  return json(200, {
    success: true,
    job,
  }, origin)
}

function handleGetItems(req: Request, origin?: string): Response {
  const userId = getUserId(req)
  if (!userId) {
    return json(401, { success: false, message: 'Unauthorized' }, origin)
  }

  const { searchParams } = new URL(req.url)
  const cursor = Math.max(0, Number(searchParams.get('cursor') || '0'))
  const limit = Math.min(100, Math.max(1, Number(searchParams.get('limit') || '20')))

  const slice = items.slice(cursor, cursor + limit)
  const nextCursor = cursor + slice.length < items.length ? cursor + slice.length : null

  return json(200, {
    success: true,
    items: slice,
    next_cursor: nextCursor,
  }, origin)
}

function handleSearch(req: Request, origin?: string): Response {
  const userId = getUserId(req)
  if (!userId) {
    return json(401, { success: false, message: 'Unauthorized' }, origin)
  }

  const { searchParams } = new URL(req.url)
  const query = (searchParams.get('q') || '').trim().toLowerCase()
  const limit = Math.min(100, Math.max(1, Number(searchParams.get('limit') || '20')))

  if (!query) {
    return json(200, { success: true, items: [] }, origin)
  }

  const matched = items
    .filter((item) => {
      return (
        item.title.toLowerCase().includes(query) ||
        item.summary.toLowerCase().includes(query) ||
        item.content.toLowerCase().includes(query)
      )
    })
    .slice(0, limit)

  return json(200, {
    success: true,
    items: matched,
  }, origin)
}

const port = Number(process.env.PORT || '8787')

Bun.serve({
  port,
  fetch: async (req) => {
    const url = new URL(req.url)
    const origin = req.headers.get('origin') || undefined

    if (req.method === 'OPTIONS') {
      return json(204, {}, origin)
    }

    if (url.pathname === '/health' && req.method === 'GET') {
      return json(200, { success: true, message: 'Refine cloud API is running' }, origin)
    }

    if (url.pathname === '/v1/conversations' && req.method === 'POST') {
      return handleCreateConversation(req, origin)
    }

    if (url.pathname === '/v1/extraction-jobs' && req.method === 'POST') {
      return handleCreateExtractionJob(req, origin)
    }

    if (url.pathname.startsWith('/v1/extraction-jobs/') && req.method === 'GET') {
      const id = url.pathname.replace('/v1/extraction-jobs/', '').trim()
      if (!id) {
        return json(400, { success: false, message: 'Job id is required' }, origin)
      }
      return handleGetExtractionJob(id, req, origin)
    }

    if (url.pathname === '/v1/items' && req.method === 'GET') {
      return handleGetItems(req, origin)
    }

    if (url.pathname === '/v1/search' && req.method === 'GET') {
      return handleSearch(req, origin)
    }

    return json(404, { success: false, message: 'Not found' }, origin)
  },
})

console.log(`Refine cloud API listening on http://localhost:${port}`)
