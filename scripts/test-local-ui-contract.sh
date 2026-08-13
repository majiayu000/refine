#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=scripts/local-ui-contract.sh
source "${SCRIPT_DIR}/local-ui-contract.sh"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

enabled_xml="$(refine_server_trusted_origins_xml 1)"
[[ "$enabled_xml" == *'<key>REFINE_TRUSTED_ORIGINS</key>'* ]] \
  || fail 'UI-enabled server plist omitted REFINE_TRUSTED_ORIGINS'
[[ "$enabled_xml" == *"<string>${REFINE_INSTALLED_UI_ORIGIN}</string>"* ]] \
  || fail 'UI-enabled server plist used the wrong trusted origin'

disabled_xml="$(refine_server_trusted_origins_xml 0)"
[[ -z "$disabled_xml" ]] \
  || fail '--no-ui-dev server plist still emitted a trusted UI origin'

[[ "$(refine_url_origin 'http://127.0.0.1:8987/dashboard/?tab=all#top')" == "$REFINE_INSTALLED_UI_ORIGIN" ]] \
  || fail 'URL path/query/fragment leaked into the browser origin'
[[ "$(refine_url_origin 'HTTP://LOCALHOST:80/')" == 'http://localhost' ]] \
  || fail 'URL origin did not normalize scheme, host, and default port'
[[ "$(refine_url_origin 'http://[::1]:8987/items')" == 'http://[::1]:8987' ]] \
  || fail 'URL origin did not preserve an IPv6 authority'
if refine_url_origin 'http://user@example.test/' >/dev/null; then
  fail 'URL origin accepted credentials'
fi

trusted_headers=$'HTTP/1.1 200 OK\r\naccess-control-allow-origin:http://127.0.0.1:8987\r\naccess-control-allow-methods: POST, GET, OPTIONS\r\n\r\n'
refine_cors_preflight_succeeds "$trusted_headers" "$REFINE_INSTALLED_UI_ORIGIN" GET \
  || fail 'doctor rejected the configured CORS origin'

untrusted_headers=$'HTTP/1.1 200 OK\r\naccess-control-allow-origin: http://evil.invalid\r\naccess-control-allow-methods: GET\r\n\r\n'
if refine_cors_preflight_succeeds "$untrusted_headers" "$REFINE_INSTALLED_UI_ORIGIN" GET; then
  fail 'doctor accepted a different CORS origin'
fi

for invalid_headers in \
  $'HTTP/1.1 403 Forbidden\r\naccess-control-allow-origin: http://127.0.0.1:8987\r\naccess-control-allow-methods: GET\r\n\r\n' \
  $'HTTP/1.1 200 OK\r\naccess-control-allow-origin: http://127.0.0.1:8987\r\n\r\n' \
  $'HTTP/1.1 200 OK\r\naccess-control-allow-methods: GET\r\n\r\n'; do
  if refine_cors_preflight_succeeds "$invalid_headers" "$REFINE_INSTALLED_UI_ORIGIN" GET; then
    fail 'doctor accepted an incomplete or unsuccessful preflight response'
  fi
done

multiple_blocks=$'HTTP/1.1 200 OK\r\naccess-control-allow-origin: http://127.0.0.1:8987\r\naccess-control-allow-methods: GET\r\n\r\nHTTP/1.1 403 Forbidden\r\n\r\n'
if refine_cors_preflight_succeeds "$multiple_blocks" "$REFINE_INSTALLED_UI_ORIGIN" GET; then
  fail 'doctor accepted headers from a non-final response block'
fi

printf 'All local UI contract tests passed\n'
