import { useRef, useState } from 'react'
import { getApiClient } from '../lib/api/client'
import type { CommitDiscussion as Discussion, CommitProjection, CommitResult } from '../lib/api/types'

export function decisionQuotes(session: Discussion, projection: CommitProjection, itemId: string) {
  if (!session.content_hash || !projection.source_version?.startsWith(session.content_hash)) return []
  const item = projection.items.find(item => item.id === itemId)
  if (!item || item.tags.some(tag => tag === 'curated' || tag === 'curation_needs_review')) return []
  const observation = projection.evidence?.observations?.find(entry => entry.item_id === itemId && entry.field === 'decisions')
  const evidence = projection.evidence?.facets?.find(entry => entry.field === 'decisions' && entry.index === observation?.index && entry.status === 'references_validated')
  if (!evidence?.message_ids.length) return []
  const messages = evidence.message_ids.map(id => session.messages.find(message => message.id === id))
  return messages.every(Boolean) ? messages.filter(message => message != null) : []
}

export function CommitDiscussion() {
  const [project, setProject] = useState('')
  const [reference, setReference] = useState('')
  const [result, setResult] = useState<CommitResult | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const generation = useRef(0)
  const clearPending = () => { generation.current += 1; setResult(null); setLoading(false); setError('') }
  const lookup = async () => {
    const run = ++generation.current
    setLoading(true); setError(''); setResult(null)
    try {
      const data = await getApiClient().getCommitContext(project.trim(), reference.trim())
      if (run === generation.current) setResult(data)
    } catch (error) {
      if (run === generation.current) setError(error instanceof Error ? error.message : '查询失败')
    } finally { if (run === generation.current) setLoading(false) }
  }
  return <section className="h-full overflow-auto p-6" aria-label="Commit 决定来源">
    <h2 className="font-display text-2xl">从 commit 找回讨论</h2>
    <p className="my-3 text-sm text-slate-600">输入 Remem 中的完整项目路径和 commit SHA 或公开 GitHub PR 链接，查看讨论、尝试与决定的来源。</p>
    <form onSubmit={event => { event.preventDefault(); void lookup() }} className="space-y-3">
      <input required aria-label="项目路径" placeholder="Remem 项目路径" value={project} onChange={event => { clearPending(); setProject(event.target.value) }} className="w-full rounded-lg border p-2" />
      <input required aria-label="Commit 或 PR" placeholder="commit SHA / https://github.com/owner/repo/pull/123" value={reference} onChange={event => { clearPending(); setReference(event.target.value) }} className="w-full rounded-lg border p-2" />
      <button className="rounded-lg bg-brand-700 px-4 py-2 text-white" disabled={loading}>{loading ? '查询中…' : '找回来源'}</button>
    </form>
    {error && <p role="alert" className="my-4 text-red-700">{error}</p>}
    {result?.commits.length === 0 && <p role="status" className="my-4">不知道：没有可查询的 commit 关联记录。</p>}
    {result?.commits.map(commit => <article key={`${commit.project}:${commit.sha}`} className="my-5 rounded-xl border p-4">
      <h3 className="break-all font-mono text-sm">{commit.sha}</h3><p className="my-2">{commit.message || '提交说明未知'}</p>
      {!commit.sessions.length && <p>不知道：没有关联会话。</p>}
      {commit.sessions.map((session, index) => {
        const projection = result.projections.find(projection => projection.session_ref === session.session_ref)
        return <div key={`${session.session_id}:${index}`} className="my-4 space-y-3">
          <p className="break-all text-xs">会话 {session.session_id} · 关联来源 {session.source}</p>
          {session.status !== 'verified' ? <p>不知道：没有明确成功 commit 与唯一原始会话的证据。</p> : <>
            <details><summary className="cursor-pointer">对应讨论与尝试方案（原始消息 {session.messages.length} 条）</summary>
              {session.messages.map(message => <blockquote key={message.id} className="my-3 whitespace-pre-wrap border-l-2 pl-3 text-sm">
                <p className="text-xs text-slate-500">#{message.id} · {message.role} · {new Date(message.created_at_epoch * 1000).toISOString()}</p>{message.content}
              </blockquote>)}
            </details>
            <h4 className="font-semibold">当前提炼的决定与来源原话</h4>
            {!projection && <p>不知道：该会话尚无 Refine 投影。可在上方原始讨论中核对。</p>}
            {projection && <>
              {!projection.items.some(item => item.tags.includes('decision')) && <p>不知道：当前投影没有决定。</p>}
              {projection.items.filter(item => item.tags.includes('decision')).map(item => {
                const quotes = decisionQuotes(session, projection, item.id)
                return <div key={item.id} className="rounded-lg bg-sand-50 p-3"><p>{item.title}</p><p className="text-xs text-slate-500">提炼或人工修订内容；引用存在不代表语义已证明。</p>
                  {!quotes.length && <p className="text-sm">决定原话不知道：缺少当前快照的机器证据或该条已被人工修订。</p>}
                  {quotes.map(message => <blockquote key={message.id} className="mt-2 whitespace-pre-wrap border-l-2 pl-3 text-sm">#{message.id} · {message.role}：{message.content}</blockquote>)}
                </div>
              })}
              <details><summary className="cursor-pointer">已有投影中的其他决定（最近 {projection.history.length} 个历史版本）</summary>
                {projection.history.map(revision => <div key={revision.revision_id} className="my-3 text-sm"><p>归档于 {revision.archived_at}</p>
                  {revision.items.filter(item => item.tags.includes('decision')).map(item => <p key={item.id}>{item.title}</p>)}
                </div>)}
              </details>
            </>}
            <p className="text-xs text-slate-600">是否被新决定替代：不知道。请依据原话和历史决定核对；重算历史未建立语义替代关系。</p>
          </>}
        </div>
      })}
    </article>)}
  </section>
}
