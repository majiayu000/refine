# Extension-owned recommendation preview validation

Implementation is complete. Private titles, summaries, tags and all recommendation
content stay in the Chrome extension popup. Opening the popup or refreshing it
queries the current input; the popup retains the existing per-site enable setting.
Only an explicit insertion sends one selected fragment to the page. Copying uses
the clipboard. Inputs/conversation URLs changed since preview are refused.

Completed checks on 2026-10-07:

- `bun install --frozen-lockfile`: passed against the final lockfile, including
  the pinned Playwright development dependency.
- `bun test`: 33 passed, zero failed (existing 9 test files).
- `bun run typecheck`: passed.
- `bun run build`: passed, real `chrome-mv3-prod` output.
- `bun run test:privacy`: passed with Playwright 1.62.1 and Chromium 153.0.8010.12;
  rerun against the final version 0.1.4 MV3 build passed and refreshed all records.
- `git diff --check -- apps/extension`: passed.

The browser command used an installed compatible Chromium selected through
`CHROMIUM_EXECUTABLE` and wrote into `PRIVACY_EVIDENCE_DIR`. The committed test can
run with Playwright's installed Chromium without either override. It loads the
actual built MV3 worker and popup document and routes a supported-origin page to
a synthetic DOM-reader fixture. Its local API returns two synthetic private items.
The popup is opened as an extension page following Playwright's documented
extension testing path, while the target website tab remains active.

The records demonstrate:

1. No private title, summary, tag or content appears in any page-side DOM/input
   snapshot during preview.
2. Clicking insertion exposes only the selected item's fragment, without its
   metadata or the unselected item.
3. Input changes and conversation URL changes refuse insertion.
4. The existing per-site setting disables queries and empties the preview.
5. A deliberately denied clipboard write is not counted as successful reuse.
6. Both textarea and contenteditable insertion work; the API receives reuse
   events only for the two deliberate successful insertions and no raw query.

`result.json` records browser version, API receipts and assertions.
`page-before.txt` and `page-after.txt` are the website script's reads around the
first insertion. `popup-preview.png` shows the real popup document's private
preview. The fixture HTML is embedded in `tests/recommendation-privacy.e2e.cjs`.
All items and conversations in these records are synthetic.

Still unverified: manual toolbar installation/use and current signed-in provider
editor behavior on real accounts. No Computer Use, real account access or
production API was used, and these results do not claim those acceptances.

Decision and sources: reuse the existing popup instead of adding a side panel,
permission or recommendation cache. Chrome's official popup/content-script docs
and Playwright's extension test docs support this boundary and test path:

- https://developer.chrome.com/docs/extensions/develop/ui/add-popup
- https://developer.chrome.com/docs/extensions/develop/concepts/content-scripts
- https://playwright.dev/docs/chrome-extensions
