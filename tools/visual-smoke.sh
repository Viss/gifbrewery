#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OUT_DIR:-/code/gifbrewery-visual-smoke}"
SCREEN_SIZE="${SCREEN_SIZE:-1280x900x24}"
APP="${APP:-${ROOT_DIR}/target/debug/gifbrewery-gtk}"
SMOKE_APP="${OUT_DIR}/gifbrewery-gtk"
DEFAULT_SMOKE_SOURCE="${ROOT_DIR}/GIF Brewery 3.app/Contents/Resources/loading.mp4"
if [[ ! -f "${DEFAULT_SMOKE_SOURCE}" ]]; then
  DEFAULT_SMOKE_SOURCE="/code/GIF Brewery 3.app/Contents/Resources/loading.mp4"
fi
SMOKE_SOURCE="${SMOKE_SOURCE:-${DEFAULT_SMOKE_SOURCE}}"

mkdir -p "${OUT_DIR}"

if [[ ! -x "${APP}" ]]; then
  cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" --bin gifbrewery-gtk
fi
cp "${APP}" "${SMOKE_APP}.next"
mv -f "${SMOKE_APP}.next" "${SMOKE_APP}"

display_file="$(mktemp)"
exec 3>"${display_file}"
Xvfb -displayfd 3 -screen 0 "${SCREEN_SIZE}" -nolisten tcp &
xvfb_pid=$!
exec 3>&-

app_pid=""
cleanup() {
  if [[ -n "${app_pid}" ]]; then
    kill "${app_pid}" 2>/dev/null || true
  fi
  kill "${xvfb_pid}" 2>/dev/null || true
  rm -f "${display_file}"
}
trap cleanup EXIT

for _ in {1..50}; do
  if [[ -s "${display_file}" ]]; then
    break
  fi
  if ! kill -0 "${xvfb_pid}" 2>/dev/null; then
    printf 'Xvfb failed to start\n' >&2
    exit 1
  fi
  sleep 0.1
done

if [[ ! -s "${display_file}" ]]; then
  printf 'Xvfb did not report a display number\n' >&2
  exit 1
fi

DISPLAY_ID=":$(cat "${display_file}")"

app_args=()
if [[ -f "${SMOKE_SOURCE}" ]]; then
  app_args+=("${SMOKE_SOURCE}")
fi

env \
  DISPLAY="${DISPLAY_ID}" \
  GDK_BACKEND=x11 \
  GSK_RENDERER=cairo \
  LIBGL_ALWAYS_SOFTWARE=1 \
  GTK_A11Y=none \
  NO_AT_BRIDGE=1 \
  "${SMOKE_APP}" "${app_args[@]}" >"${OUT_DIR}/app.log" 2>&1 &
app_pid=$!

window_id=""
for _ in {1..80}; do
  window_id="$(env DISPLAY="${DISPLAY_ID}" xdotool search --onlyvisible --name "GIF Brewery" 2>/dev/null | head -n 1 || true)"
  if [[ -n "${window_id}" ]]; then
    break
  fi
  if ! kill -0 "${app_pid}" 2>/dev/null; then
    printf 'GIF Brewery exited before showing a window. See %s/app.log\n' "${OUT_DIR}" >&2
    exit 1
  fi
  sleep 0.1
done

if [[ -z "${window_id}" ]]; then
  printf 'Could not find GIF Brewery window on %s. See %s/app.log\n' "${DISPLAY_ID}" "${OUT_DIR}" >&2
  exit 1
fi

sleep 2

capture() {
  local name="$1"
  local mean="0"
  for _ in {1..20}; do
    env DISPLAY="${DISPLAY_ID}" xwd -id "${window_id}" -silent -out "${OUT_DIR}/${name}.xwd"
    magick "${OUT_DIR}/${name}.xwd" "${OUT_DIR}/${name}.png"
    mean="$(magick "${OUT_DIR}/${name}.png" -format "%[fx:mean]" info:)"
    if [[ "${mean}" != "0" && "${mean}" != "0.0" ]]; then
      return
    fi
    sleep 0.25
  done
  printf 'Capture %s remained black after retries\n' "${name}" >&2
  return 1
}

capture clip
env DISPLAY="${DISPLAY_ID}" xdotool mousemove 1104 68 click 1
sleep 0.3
capture gif
env DISPLAY="${DISPLAY_ID}" xdotool mousemove 1210 68 click 1
sleep 0.3
capture overlays

if [[ -f "${SMOKE_SOURCE}" ]]; then
  "${SMOKE_APP}" --smoke-export-multi-overlay "${SMOKE_SOURCE}" "${OUT_DIR}/export-multi-overlay.gif" >"${OUT_DIR}/export-multi-overlay.log" 2>&1
  if command -v ffmpeg >/dev/null 2>&1; then
    ffmpeg -hide_banner -y -i "${OUT_DIR}/export-multi-overlay.gif" -frames:v 1 -update 1 "${OUT_DIR}/export-multi-overlay-first-frame.png" >>"${OUT_DIR}/export-multi-overlay.log" 2>&1
    ffmpeg -hide_banner -y -i "${OUT_DIR}/export-multi-overlay.gif" -vf "select='gte(t,0.55)'" -frames:v 1 -update 1 "${OUT_DIR}/export-multi-overlay-late-frame.png" >>"${OUT_DIR}/export-multi-overlay.log" 2>&1
  fi
fi

printf 'Screenshots written to %s\n' "${OUT_DIR}"
