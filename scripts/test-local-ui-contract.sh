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

trusted_headers=$'HTTP/1.1 200 OK\r\naccess-control-allow-origin: http://127.0.0.1:8987\r\n\r\n'
refine_cors_response_allows_origin "$trusted_headers" "$REFINE_INSTALLED_UI_ORIGIN" \
  || fail 'doctor rejected the configured CORS origin'

untrusted_headers=$'HTTP/1.1 200 OK\r\naccess-control-allow-origin: http://evil.invalid\r\n\r\n'
if refine_cors_response_allows_origin "$untrusted_headers" "$REFINE_INSTALLED_UI_ORIGIN"; then
  fail 'doctor accepted a different CORS origin'
fi

if refine_cors_response_allows_origin $'HTTP/1.1 200 OK\r\n\r\n' "$REFINE_INSTALLED_UI_ORIGIN"; then
  fail 'doctor accepted a response without a CORS allow-origin header'
fi

printf 'All local UI contract tests passed\n'
