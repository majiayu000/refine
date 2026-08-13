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

refine_url_origin() {
  local url="$1"
  local scheme authority host port=""

  if [[ "$url" =~ ^([Hh][Tt][Tt][Pp][Ss]?)://([^/?#]+)(/[^?#]*)?(\?[^#]*)?(#.*)?$ ]]; then
    scheme="$(printf '%s' "${BASH_REMATCH[1]}" | tr '[:upper:]' '[:lower:]')"
    authority="${BASH_REMATCH[2]}"
  else
    return 1
  fi
  [[ "$authority" != *'@'* && "$authority" != *[$'\t\r\n ']* ]] || return 1

  if [[ "$authority" =~ ^(\[[0-9A-Fa-f:.]+\])(:([0-9]+))?$ ]]; then
    host="${BASH_REMATCH[1]}"
    port="${BASH_REMATCH[3]:-}"
  elif [[ "$authority" =~ ^([A-Za-z0-9._-]+)(:([0-9]+))?$ ]]; then
    host="${BASH_REMATCH[1]}"
    port="${BASH_REMATCH[3]:-}"
  else
    return 1
  fi
  host="$(printf '%s' "$host" | tr '[:upper:]' '[:lower:]')"
  if [[ "$scheme" == "http" && "$port" == "80" ]] \
    || [[ "$scheme" == "https" && "$port" == "443" ]]; then
    port=""
  fi

  printf '%s://%s' "$scheme" "$host"
  [[ -z "$port" ]] || printf ':%s' "$port"
  printf '\n'
}

refine_cors_preflight_succeeds() {
  local headers="$1"
  local expected_origin="$2"
  local expected_method="$3"
  local line status="" actual_origin="" allowed_methods="" name value token

  while IFS= read -r line; do
    line="${line%$'\r'}"
    if [[ "$line" =~ ^HTTP/[0-9.]+[[:space:]]+([0-9]{3})([[:space:]]|$) ]]; then
      # Only the final response block is authoritative (for example after an
      # intermediary's 100 Continue response).
      status="${BASH_REMATCH[1]}"
      actual_origin=""
      allowed_methods=""
      continue
    fi
    [[ "$line" == *:* ]] || continue
    name="$(printf '%s' "${line%%:*}" | tr '[:upper:]' '[:lower:]')"
    value="${line#*:}"
    value="${value#"${value%%[![:space:]]*}"}"
    value="${value%"${value##*[![:space:]]}"}"
    case "$name" in
      access-control-allow-origin) actual_origin="$value" ;;
      access-control-allow-methods) allowed_methods="${allowed_methods:+${allowed_methods},}${value}" ;;
    esac
  done <<< "$headers"

  [[ "$status" == 2[0-9][0-9] && "$actual_origin" == "$expected_origin" ]] || return 1
  while IFS= read -r token; do
    token="${token#"${token%%[![:space:]]*}"}"
    token="${token%"${token##*[![:space:]]}"}"
    [[ "$(printf '%s' "$token" | tr '[:lower:]' '[:upper:]')" == "$expected_method" ]] \
      && return 0
  done < <(printf '%s' "$allowed_methods" | tr ',' '\n')
  return 1
}
