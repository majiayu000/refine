import { expect, test } from 'bun:test'
import { readFileSync } from 'node:fs'

// Load only the pure selector/role helpers, without starting the content script.
const source = readFileSync(new URL('../../contents/grok.tsx', import.meta.url), 'utf8')
const start = source.indexOf('const GROK_TURN_SELECTORS')
const end = source.indexOf('function extractConversationByBubbleFallback', start)
if (start < 0 || end < 0) throw new Error('Grok role helpers were not found')
const javascript = new Bun.Transpiler({ loader: 'tsx' }).transformSync(source.slice(start, end))
const inferRole = new Function(`${javascript}\nreturn inferBubbleRole`)() as (
  bubble: Element,
  index: number,
) => 'Human' | 'Assistant'

function bubble(attributes: Record<string, string>, className = 'message-bubble'): Element {
  return {
    className,
    getAttribute: (name: string) => attributes[name] ?? null,
    // Every fixture is a div.message-bubble inside main; match the explicit marker.
    matches: (selector: string) => Object.entries(attributes).some(
      ([name, value]) => selector.includes(`[${name}="${value}"]`),
    ),
  } as unknown as Element
}

test('known author roles override parity after empty or consecutive-user bubbles', () => {
  expect(inferRole(bubble({ 'data-message-author-role': 'user' }), 1)).toBe('Human')
  expect(inferRole(bubble({ 'data-message-author-role': 'assistant' }), 2)).toBe('Assistant')
  expect(inferRole(bubble({ 'data-message-author-role': 'user' }), 3)).toBe('Human')
})

test('the same known-selector precedence is retained before class heuristics', () => {
  expect(inferRole(bubble({ 'data-role': 'assistant' }, 'message-bubble user-message'), 0)).toBe('Assistant')
  expect(inferRole(bubble({ 'data-role': 'user' }), 1)).toBe('Human')
})

test('unmatched bubbles retain the existing fallback', () => {
  expect(inferRole(bubble({}), 0)).toBe('Human')
  expect(inferRole(bubble({}), 1)).toBe('Assistant')
})
