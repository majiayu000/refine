const { chromium } = require('playwright');
const http = require('node:http');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const assert = require('node:assert/strict');
const { setTimeout: delay } = require('node:timers/promises');
const root = path.resolve(__dirname, '..');
const extension = path.join(root, 'build/chrome-mv3-prod');
const evidence = process.env.PRIVACY_EVIDENCE_DIR || fs.mkdtempSync(path.join(os.tmpdir(), 'refine-privacy-evidence-'));
fs.mkdirSync(evidence, {recursive:true});
const item = (suffix) => ({ id: suffix, title: `PRIVATE_TITLE_${suffix}`, summary: `PRIVATE_SUMMARY_${suffix}`, tags: [`PRIVATE_TAG_${suffix}`], content: `SELECTED_FRAGMENT_${suffix}`, item_type: 'knowledge', match_strategy: 'keyword_match', score: 1 });
const items = [item('ONE'), item('TWO')];
const events = [];
let requests = 0;
const server = http.createServer((req, res) => {
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('x-refine-contract-version', '1');
  if (req.url.startsWith('/v1/recommendations')) { requests++; res.end(JSON.stringify({ triggered: true, query: 'public query', items })); }
  else if (req.url === '/v1/events') { let body = ''; req.on('data', data => body += data); req.on('end', () => { events.push(JSON.parse(body)); res.end(JSON.stringify({ success: true })); }); }
  else if (req.url.startsWith('/v1/items')) res.end(JSON.stringify({ success: true, total: 2, items: [] }));
  else if (req.url === '/v1/quota') res.end(JSON.stringify({ success: true, limit: null, used: 0, remaining: null, exceeded: false }));
  else res.end(JSON.stringify({ success: true, service: 'Refine cloud API' }));
});
const html = `<!doctype html><html><head><title>DOM-reader fixture</title></head><body><textarea id="prompt-textarea" placeholder="Message"></textarea><script>
window.pageReads=[];
function read(){window.pageReads.push(JSON.stringify({html:document.documentElement.outerHTML,values:[...document.querySelectorAll('input,textarea')].map(e=>e.value)}));}
new MutationObserver(read).observe(document.documentElement,{subtree:true,childList:true,characterData:true,attributes:true});
document.addEventListener('input',read,true);read();
</script></body></html>`;
(async () => {
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(21570, '127.0.0.1', resolve); });
  let context;
  const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'refine-mv3-'));
  try {
    context = await chromium.launchPersistentContext(profile, { headless: true, channel: 'chromium', ...(process.env.CHROMIUM_EXECUTABLE ? {executablePath: process.env.CHROMIUM_EXECUTABLE} : {}), args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`] });
    await context.route('https://chatgpt.com/**', route => route.fulfill({ contentType: 'text/html', body: html }));
    const worker = context.serviceWorkers()[0] || await context.waitForEvent('serviceworker');
    const id = new URL(worker.url()).host;
    await worker.evaluate(() => chrome.storage.local.set({ refine_server_port: { port: 21570, ts: Date.now() } }));
    const page = await context.newPage();
    await page.goto('https://chatgpt.com/c/dom-reader-fixture');
    await page.locator('#prompt-textarea').fill('Explain our architecture choice');
    const tabId = await worker.evaluate(async () => (await chrome.tabs.query({url:'https://chatgpt.com/*'}))[0].id);
    // A popup opened as an extension tab uses the same real popup document and
    // runtime. Keep the host tab active for the popup's active-tab query.
    const popup = await context.newPage();
    await worker.evaluate((id) => chrome.tabs.update(id, { active: true }), tabId);
    await popup.goto(`chrome-extension://${id}/popup.html`);
    await popup.getByText('PRIVATE_TITLE_ONE', { exact: true }).waitFor();
    assert.equal(await popup.locator('.recommendations-item').count(), 2);
    const before = await page.evaluate(() => window.pageReads.join('\n'));
    for (const item of items) for (const secret of [item.title, item.summary, ...item.tags, item.content]) assert(!before.includes(secret), `Before insertion disclosed ${secret}`);
    await popup.screenshot({path: path.join(evidence, 'popup-preview.png'), fullPage: true});
    await popup.evaluate(() => { navigator.clipboard.writeText = async () => { throw new DOMException('fixture denial', 'NotAllowedError'); }; });
    await popup.locator('.recommendations-item').first().getByRole('button', {name:'复制',exact:true}).click();
    await popup.getByText('复制失败，请重试', {exact:true}).waitFor();
    assert.equal(events.filter(e => e.event_name === 'knowledge_reused').length, 0);
    await popup.locator('.recommendations-item').first().getByRole('button', { name: '插入', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('#prompt-textarea').value.includes('SELECTED_FRAGMENT_ONE'));
    const after = await page.evaluate(() => window.pageReads.join('\n'));
    assert(after.includes(items[0].content));
    for (const secret of [items[0].title, items[0].summary, ...items[0].tags, items[1].title, items[1].summary, ...items[1].tags, items[1].content]) assert(!after.includes(secret), `After insertion disclosed ${secret}`);
    // Changed-input and changed-conversation insertions must fail without reuse events.
    await popup.getByRole('button', {name:'刷新', exact:true}).click();
    await popup.getByText('PRIVATE_TITLE_ONE', { exact:true }).waitFor();
    await page.locator('#prompt-textarea').fill('Changed input after preview');
    await popup.locator('.recommendations-item').first().getByRole('button', {name:'插入',exact:true}).click();
    await popup.getByText('输入或页面已变化，请刷新推荐后再插入').waitFor();
    assert.equal(await page.inputValue('#prompt-textarea'), 'Changed input after preview');
    await popup.getByRole('button', {name:'刷新',exact:true}).click();
    await popup.getByText('PRIVATE_TITLE_ONE', { exact:true }).waitFor();
    await page.evaluate(() => history.pushState({}, '', '/c/other-conversation'));
    await popup.locator('.recommendations-item').first().getByRole('button', {name:'插入',exact:true}).click();
    await popup.getByText('输入或页面已变化，请刷新推荐后再插入').waitFor();
    assert.equal(await page.inputValue('#prompt-textarea'), 'Changed input after preview');
    await popup.getByRole('button', {name:'此网站：开',exact:true}).click();
    await popup.getByText('此网站推荐已关闭', {exact:true}).waitFor();
    const disabledRequests = requests;
    await popup.getByRole('button', {name:'刷新',exact:true}).click();
    await popup.getByText('此网站推荐已关闭', {exact:true}).waitFor();
    assert.equal(requests, disabledRequests);
    assert.equal(await popup.locator('.recommendations-item').count(), 0);
    // Exercise the other supported editor type with a deliberate second selection.
    await page.evaluate(() => {
      const editor = document.createElement('div'); editor.id = 'prompt-textarea'; editor.contentEditable = 'true';
      document.querySelector('#prompt-textarea').replaceWith(editor);
    });
    await page.locator('#prompt-textarea').fill('Contenteditable public query');
    await popup.getByRole('button', {name:'此网站：关',exact:true}).click();
    await popup.getByText('PRIVATE_TITLE_TWO', {exact:true}).waitFor();
    await popup.locator('.recommendations-item').nth(1).getByRole('button', {name:'插入',exact:true}).click();
    await page.waitForFunction(() => document.querySelector('#prompt-textarea').textContent.includes('SELECTED_FRAGMENT_TWO'));
    const editorReads = await page.evaluate(() => window.pageReads.join('\n'));
    for (const item of items) for (const secret of [item.title,item.summary,...item.tags]) assert(!editorReads.includes(secret));
    // Tracking is asynchronous; wait on the observed API receipt with a deadline.
    const eventDeadline = Date.now() + 5000;
    while (events.filter(e => e.event_name === 'knowledge_reused').length < 2 && Date.now() < eventDeadline) await delay(20);
    const reused = events.filter(e => e.event_name === 'knowledge_reused');
    assert.equal(reused.length, 2);
    assert(!JSON.stringify(events).includes('Explain our architecture choice'));
    fs.writeFileSync(path.join(evidence, 'result.json'), JSON.stringify({browser:await context.browser().version(), extensionId:id, requests, events, checks:{previewPrivate:true, selectedFragmentOnly:true, changedInputRefused:true, changedUrlRefused:true, perSiteDisabled:true, clipboardFailureNotReused:true, contenteditableInsertion:true}, scope:'Real built MV3 extension, synthetic ChatGPT-shaped DOM reader and local API; no live account validation'},null,2));
    fs.writeFileSync(path.join(evidence, 'page-before.txt'), before);
    fs.writeFileSync(path.join(evidence, 'page-after.txt'), after);
    fs.writeFileSync(path.join(evidence, 'fixture.html'), html);
    console.log('PASS: private preview; selected fragment only; stale input/URL refused; per-site disabled; textarea/contenteditable insertion; successful reuse only. Evidence:', evidence);
  } finally { if(context) await context.close(); server.close(); fs.rmSync(profile, {recursive:true,force:true}); }
})().catch(error => { console.error(error); process.exitCode = 1; });
