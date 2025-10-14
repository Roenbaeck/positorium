#!/usr/bin/env bash
# ---------------------------------------------------------------------------
# Positorium convenience script (macOS / Linux)
# Similar intent as PowerShell positorium.ps1: start / stop / restart the server
# with controlled logging. Can be sourced to expose functions or executed
# directly with subcommands.
# ---------------------------------------------------------------------------
# Usage examples:
#   ./scripts/positorium.sh start                      # start with default (normal) profile
#   ./scripts/positorium.sh start --profile verbose    # verbose logging
#   ./scripts/positorium.sh start --log 'warn,positorium=info' --tail
#   ./scripts/positorium.sh restart --profile trace
#   ./scripts/positorium.sh stop
#   source ./scripts/positorium.sh && positorium_start --profile verbose
#
# Profiles:
#   quiet -> RUST_LOG=error
#   normal -> RUST_LOG=info
#   verbose -> RUST_LOG=debug,positorium=info
#   trace -> RUST_LOG=trace
# If --log is supplied it overrides the profile mapping.
#
# Flags:
#   --profile <quiet|normal|verbose|trace>
#   --log <env_filter>
#   --release            (use cargo --release)
#   --force-rebuild      (cargo clean before starting)
#   --tail               (run in foreground, do not daemonize)
#   --help
# ---------------------------------------------------------------------------
set -euo pipefail

SCRIPT_DIR="$( cd "${BASH_SOURCE[0]%/*}" && pwd )"
REPO_ROOT="$( cd "${SCRIPT_DIR}/.." && pwd )"
PID_FILE="${REPO_ROOT}/.positorium.pid"
LOG_FILE="${REPO_ROOT}/positorium.out"
DEFAULT_PROFILE="normal"

color() { local c="$1"; shift || true; if [[ -t 1 ]]; then case "$c" in
  red) printf '\033[31m%s\033[0m' "$*";; green) printf '\033[32m%s\033[0m' "$*";;
  yellow) printf '\033[33m%s\033[0m' "$*";; cyan) printf '\033[36m%s\033[0m' "$*";;
  *) printf '%s' "$*";; esac; else printf '%s' "$*"; fi }

set_log_env() {
  local profile="$1"; local log_override="${2:-}"; local val
  if [[ -n "${log_override}" ]]; then
    val="${log_override}"
  else
    case "$profile" in
      quiet)   val="error";;
      normal)  val="info";;
      verbose) val="debug,positorium=info";;
      trace)   val="trace";;
      *)       val="info";;
    esac
  fi
  export RUST_LOG="$val"
  echo "[positorium] RUST_LOG=${RUST_LOG}" >&2
}

is_running() {
  [[ -f "$PID_FILE" ]] || return 1
  local pid; pid="$(cat "$PID_FILE" 2>/dev/null || true)"; [[ -n "$pid" ]] || return 1
  if kill -0 "$pid" 2>/dev/null; then return 0; fi
  return 1
}

positorium_start() {
  local profile="$DEFAULT_PROFILE" log_override="" release=0 force=0 tail=0
  while [[ $# -gt 0 ]]; do case "$1" in
    --profile) profile="$2"; shift 2;;
    --log) log_override="$2"; shift 2;;
    --release) release=1; shift;;
    --force-rebuild) force=1; shift;;
    --tail) tail=1; shift;;
    --help) positorium_help; return 0;;
    *) echo "Unknown option: $1" >&2; return 1;;
  esac; done
  if is_running; then echo "$(color yellow '[positorium] Already running') (PID $(cat "$PID_FILE"))"; return 0; fi
  set_log_env "$profile" "$log_override"
  local cargo_cmd=(cargo run --quiet)
  (( release )) && cargo_cmd=(cargo run --release --quiet)
  (( force )) && { echo "$(color yellow '[positorium] Forcing clean build…')"; (cd "$REPO_ROOT" && cargo clean); }
  echo "$(color green '[positorium] Starting') args: ${cargo_cmd[*]}" >&2
  if (( tail )); then
    (cd "$REPO_ROOT" && exec "${cargo_cmd[@]}")
  else
    (cd "$REPO_ROOT" && "${cargo_cmd[@]}" >"$LOG_FILE" 2>&1 & echo $! >"$PID_FILE")
    sleep 0.6
    if is_running; then
      echo "$(color green '[positorium] Running') PID $(cat "$PID_FILE") (logs: $LOG_FILE)" >&2
    else
      echo "$(color red '[positorium] Failed to start (see logs)')" >&2
      return 1
    fi
  fi
}

positorium_stop() {
  if ! is_running; then echo "[positorium] Not running"; return 0; fi
  local pid; pid="$(cat "$PID_FILE")"
  echo "$(color yellow '[positorium] Stopping') PID $pid" >&2
  if kill "$pid" 2>/dev/null; then
    wait "$pid" 2>/dev/null || true
    rm -f "$PID_FILE"
    echo "$(color green '[positorium] Stopped')" >&2
  else
    echo "$(color red '[positorium] Failed to signal process')" >&2
    return 1
  fi
}

positorium_restart() {
  local args=("$@")
  positorium_stop || true
  positorium_start "${args[@]}"
}

positorium_status() {
  if is_running; then echo "[positorium] Running (PID $(cat "$PID_FILE"))"; else echo "[positorium] Not running"; fi
}

positorium_tail() {
  if ! is_running; then echo "[positorium] Not running"; return 1; fi
  tail -f "$LOG_FILE"
}

positorium_help() {
  cat <<EOF
Positorium helper (bash)
Commands:
  start [--profile P] [--log FILTER] [--release] [--force-rebuild] [--tail]
  stop
  restart [same flags as start]
  status
  tail            Follow log file (background mode only)
  help
Profiles: quiet | normal | verbose | trace (default: normal)
Examples:
  ./scripts/positorium.sh start --profile verbose
  ./scripts/positorium.sh start --log 'warn,positorium=info'
  ./scripts/positorium.sh restart --profile trace --force-rebuild
EOF
}

# When sourced, do not execute CLI dispatch
if [[ "${BASH_SOURCE[0]}" != "$0" ]]; then
  return 0
fi

cmd="${1:-help}"; shift || true
case "$cmd" in
  start)   positorium_start "$@";;
  stop)    positorium_stop;;
  restart) positorium_restart "$@";;
  status)  positorium_status;;
  tail)    positorium_tail;;
  help|*)  positorium_help;;
 esac
