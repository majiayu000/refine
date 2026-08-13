#!/usr/bin/env bash

# Shared browser boundary for the locally installed Vite UI. Keep the
# installer and doctor on the same exact origin so a healthy UI cannot be
# silently blocked by the server's CORS policy.
REFINE_INSTALLED_UI_ORIGIN="http://127.0.0.1:8987"

refine_server_trusted_origins_xml() {
  local ui_dev_enabled="$1"
  if [[ "$ui_dev_enabled" == "1" ]]; then
    printf '    <key>REFINE_TRUSTED_ORIGINS</key>\n    <string>%s</string>\n' \
      "$REFINE_INSTALLED_UI_ORIGIN"
  fi
}

refine_cors_response_allows_origin() {
  local headers="$1"
  local expected_origin="$2"
  local actual_origin
  actual_origin="$(printf '%s\n' "$headers" | awk '
    tolower($1) == "access-control-allow-origin:" {
      $1 = ""
      sub(/^ /, "")
      sub(/\r$/, "")
      print
      exit
    }
  ')"
  [[ "$actual_origin" == "$expected_origin" ]]
}
