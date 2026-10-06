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

Recommendation previews currently render titles, summaries and tags in the
website's ordinary DOM. The website's own scripts can read that preview before
the user clicks insert or copy. Content-script JavaScript isolation does not
make this DOM private. Use the existing per-site recommendation toggle to disable
previews on sites where that disclosure is undesirable. This change preserves
the existing default; moving previews to extension-owned UI is separate work.

Recommendation exposure/click/reuse events are sent through the background
worker, which authenticates the request. The message handler accepts only these
event names, verifies the sender site's source, and strips raw queries and other
unrecognized properties. The service's CORS allowlist does not need to include
ChatGPT, Claude or other visited websites. Failed clipboard writes are not
reported as successful reuse.

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
