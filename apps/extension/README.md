# Browser extension capture and sync

The extension is a developer preview for the local Refine service. A successful
capture means the full payload and its retry identity have been saved to the
extension's local outbox. Network access, authentication and server quota checks
happen afterward. Failed or timed-out deliveries retain the same idempotency key
and remain available for retry.

The popup distinguishes three facts:

- **Saved locally:** the browser outbox has accepted the capture.
- **Accepted by the service:** a response contains a conversation ID. The outbox
  retains that ID, the optional job ID, and the status observed in the receipt.
- **Extraction completed:** this is determined by the service's extraction job.
  The popup does not poll jobs and never treats a queued receipt as completion.

Requests have a deadline covering discovery, credential reads and response body
reads. A timed-out delivery releases the queue; any late result from that attempt
cannot replace the receipt of its retry. Browser-worker restart recovery still
uses the persisted outbox lease. The existing `totalItems` local stats field counts
accepted conversations; the popup labels it separately from the server's actual
knowledge-item total.

## Capture identity

Sidebar captures are bound to the conversation URL/key and a navigation
generation. The identity is checked during content polling and after provider
history loading. Changing conversations cancels the capture, including leaving
and returning to the same URL when the browser reports navigation events.

When quick-save navigates to another conversation, it records a fingerprint of
the previously rendered text and waits for replacement text to become stable.
It does not copy that previous text into the site's `sessionStorage`. If the new
conversation has exactly the same text, or readiness cannot be established, the
capture fails closed and asks the user to save again on the loaded target page.
Provider DOM changes and browsers without Navigation API support still require
manual validation; URL checks, popstate/hashchange and DOM observation are the
fallbacks. This does not claim a complete provider transcript when the website
has not rendered its history.

## Recommendation boundary

Recommendation titles, summaries and tags render only in the extension-owned
popup. Open the toolbar popup after typing in a supported conversation input,
or click **刷新** to update the preview. The per-site switch now lives in the popup
and retains existing settings. Opening/refreshing the popup performs the query;
there is no private recommendation panel in the website DOM.

Only clicking **插入** sends the selected item's content (or its summary/title
fallback) to the content script and website input. Other recommendations and
metadata remain in the popup. **复制** writes the selected fragment to the
clipboard without adding it to the website. Insertion refuses if the original
input text or conversation URL changed; refresh the preview to try again.

Recommendation exposure/click/reuse events are sent through the background
worker, which authenticates the request. Popup events bind their source to the
browser's tab URL; content events bind it to their sender URL. Both use the same
existing event validation and strip raw queries/unrecognized properties. The
service's CORS allowlist does not need to include visited websites. Failed
clipboard writes or insertions are not reported as successful reuse.

### Implementation choice

**Adopt** the existing Chrome action popup, following the official
[popup documentation](https://developer.chrome.com/docs/extensions/develop/ui/add-popup).
[Content-script isolation](https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts)
protects JavaScript variables, but page DOM remains shared. **Adapt** the existing
input bridge and recommendation API. A side panel was rejected for this change:
the existing popup meets the private-preview requirement without another UI
surface, permission or recommendation cache. No capture pipeline was added.

The tradeoff is explicit refresh while the popup is open, rather than an
in-page preview while typing. Provider DOM selectors still require real account
validation when providers change their editors.

## Validation

From this directory:

```sh
bun install --frozen-lockfile
bun test
bun run typecheck
bun run build
```

The regression tests use synthetic providers, deferred requests and local storage
fixtures. They do not connect to real conversations or browser accounts.

### Browser privacy regression

After the MV3 build, run the real extension with a synthetic supported-origin
site whose script records all DOM changes and input values:

```sh
bunx playwright install chromium
bun run test:privacy
```

The test loads the actual popup document and MV3 worker in headless Chromium,
serves a fixture API on unused port 21570, and routes the ChatGPT document to a
local DOM-reader fixture. It asserts private previews are absent from every
page-side read, only the selected fragment appears after insertion, changed
input/URL insertion fails, per-site disabling prevents recommendation requests,
and only successful reuse is reported. It writes JSON, DOM-read records and a
popup screenshot to a temporary evidence directory. `PRIVACY_EVIDENCE_DIR` can
set the output directory; `CHROMIUM_EXECUTABLE` can select an installed compatible
Chromium. This is programmatic browser validation, not a live provider account
or manual toolbar-installation acceptance test.
