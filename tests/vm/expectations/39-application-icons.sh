#!/usr/bin/env bash
set -euo pipefail
: "${OVM:?set OVM to the harness executable}"
[[ -n ${OVM_HOME:-} && -n ${OVM_SSH_PORT:-} ]]
source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/lib.sh"

guest true || { fail harness "guest is available" "SSH command failed"; summary; }
FIXTURE=$GUEST_PLUGIN/tests/vm/fixtures/media_icons.py
[[ $(guest "test -f $(printf '%q' "$FIXTURE") && echo yes || echo no") == yes ]] || {
  fail harness "stage the application icon fixture" "guest fixture is absent: $FIXTURE"
  summary
}

RUN_DIR=$(guest 'mktemp -d /tmp/filetree-icons-39.XXXXXX' | tr -d '\r')
[[ $RUN_DIR =~ ^/tmp/filetree-icons-39\.[[:alnum:]]+$ ]] || {
  fail harness "create the disposable icon evidence directory" "invalid guest path: $RUN_DIR"
  summary
}
LOG=$RUN_DIR/fixture.log
PIDFILE=$RUN_DIR/fixture.pid
PROBE_PATH=
PROBE_PID=
VIEWER_PID=
APPFILE=$RUN_DIR/E-39-01-${RUN_DIR##*.}.txt

cleanup() {
  if [[ ${VIEWER_PID:-} =~ ^[0-9]+$ ]]; then
    guest "python3 - $(printf '%q' "$VIEWER_PID") $(printf '%q' "$APPFILE") <<'PYGUEST'
import os,signal,sys
from pathlib import Path
try:
    if os.fsencode(sys.argv[2]) in Path('/proc',sys.argv[1],'cmdline').read_bytes().split(b'\0'):
        os.kill(int(sys.argv[1]),signal.SIGTERM)
except ProcessLookupError:
    pass
except FileNotFoundError:
    pass
PYGUEST" >/dev/null 2>&1 || true
  fi
  if [[ -n ${PROBE_PATH:-} ]]; then
    guest "qs ipc -n -p $(printf '%q' "$PROBE_PATH") call -- media-icons-probe abort" >/dev/null 2>&1 || true
  fi
  if [[ ${PROBE_PID:-} =~ ^[0-9]+$ ]]; then
    guest "kill -- -$PROBE_PID" >/dev/null 2>&1 || true
  fi
  guest "rm -rf -- $(printf '%q' "$RUN_DIR")" >/dev/null 2>&1 || true
}
trap cleanup EXIT
trap 'exit 130' INT TERM

guest "setsid env MEDIA_ICONS_WORK_ROOT=$(printf '%q' "$RUN_DIR") MEDIA_ICONS_TIMEOUT=180 python3 $(printf '%q' "$FIXTURE") evidence >$(printf '%q' "$LOG") 2>&1 < /dev/null & echo \$! >$(printf '%q' "$PIDFILE")"
PROBE_PID=$(guest "cat $(printf '%q' "$PIDFILE")" | tr -d '\r')
[[ $PROBE_PID =~ ^[0-9]+$ ]] || {
  fail harness "launch the application icon fixture" "guest did not return a process id"
  summary
}

wait_marker() {
  local marker=$1
  local deadline=$((SECONDS + ${2:-40}))
  while ((SECONDS <= deadline)); do
    if guest "grep -Fq -- $(printf '%q' "$marker") $(printf '%q' "$LOG")"; then return 0; fi
    if ! guest "kill -0 $PROBE_PID" >/dev/null 2>&1; then return 1; fi
    sleep 0.2
  done
  return 1
}

wait_probe_exit() {
  local deadline=$((SECONDS + ${1:-15}))
  while ((SECONDS <= deadline)); do
    guest "kill -0 $PROBE_PID" >/dev/null 2>&1 || return 0
    sleep 0.2
  done
  return 1
}

if ! wait_marker 'MEDIA_ICONS_SOURCE ' 30; then
  diagnostic=$(guest "tail -n 30 $(printf '%q' "$LOG")" | tr '\n' ' ' || true)
  fail harness "launch the application icon fixture" "source marker absent; $diagnostic"
  summary
fi
source_line=$(guest "grep -F 'MEDIA_ICONS_SOURCE ' $(printf '%q' "$LOG") | tail -n 1" | tr -d '\r')
PROBE_PATH=${source_line#*MEDIA_ICONS_SOURCE }
[[ $PROBE_PATH == "$RUN_DIR"/*/probe.qml ]] || {
  fail harness "locate the application icon probe" "unexpected source path: $PROBE_PATH"
  summary
}

capture_case() {
  local number=$1
  local id=$2
  local marker="MEDIA_ICONS_EVIDENCE_READY $id"
  local ready_line observed shot_path
  if ! wait_marker "$marker" 40; then
    guest "tail -n 25 $(printf '%q' "$LOG")" || true
    fail "$id" "ready state for evidence" "marker absent from the fixture log"
    return 1
  fi
  ready_line=$(guest "grep -F $(printf '%q' "$marker") $(printf '%q' "$LOG") | tail -n 1" | tr -d '\r')
  observed=${ready_line#*${marker} }
  if ! jq -e . >/dev/null 2>&1 <<<"$observed"; then
    fail "$id" "fixture observed state is JSON" "$observed"
    return 1
  fi
  printf 'evidence %02d action %s observed %s\n' "$number" "$id" "$observed"
  shot_path=$("$OVM" shot "filetree-media-icons-$id" 2>/dev/null | tail -n 1 | tr -d '\r' || true)
  if [[ ! -f $shot_path ]]; then
    fail "$id" "capture OVM screenshot" "shot returned [$shot_path]"
    return 1
  fi
  printf 'evidence %02d screenshot %s %s\n' "$number" "$id" "$shot_path"
  if ! guest "qs ipc -n -p $(printf '%q' "$PROBE_PATH") call -- media-icons-probe capture $(printf '%q' "$id")" >/dev/null; then
    fail "$id" "advance the fixture after screenshot" "IPC capture failed"
    return 1
  fi
  return 0
}

ids=(E-39-01 E-39-02 E-39-03 E-39-04 E-39-05 E-39-06)
for index in "${!ids[@]}"; do
  capture_case "$((index + 1))" "${ids[$index]}" || summary
done

if ! wait_marker 'MEDIA_ICONS_EVIDENCE_DONE ' 30; then
  diagnostic=$(guest "tail -n 30 $(printf '%q' "$LOG")" | tr '\n' ' ' || true)
  fail harness "finish application icon evidence" "completion marker absent; $diagnostic"
  summary
fi
wait_marker 'MEDIA_ICONS_FIXTURE_OK' 20 || {
  fail harness "verify the complete fixture" "Python verification did not succeed"
  summary
}
wait_probe_exit 20 || {
  fail harness "finish application icon fixture" "fixture process remained alive"
  summary
}

done_line=$(guest "grep -F 'MEDIA_ICONS_EVIDENCE_DONE ' $(printf '%q' "$LOG") | tail -n 1" | tr -d '\r')
evidence_json=${done_line#*MEDIA_ICONS_EVIDENCE_DONE }
if ! jq -e 'length == 6 and (map(.id) | sort == ["E-39-01","E-39-02","E-39-03","E-39-04","E-39-05","E-39-06"])' >/dev/null 2>&1 <<<"$evidence_json"; then
  fail harness "parse six application icon evidence results" "$evidence_json"
  summary
fi

evidence_pass() {
  local number=$1
  local id=$2
  local label=$3
  local expression=$4
  skip "$id" && return 0
  if jq -e --arg id "$id" "$expression" >/dev/null 2>&1 <<<"$evidence_json"; then
    printf 'evidence %02d pass %s\n' "$number" "$id"
    pass "$id" "$label"
  else
    local observed
    observed=$(jq -c --arg id "$id" '.[] | select(.id == $id)' <<<"$evidence_json" 2>/dev/null || printf '%s' invalid)
    fail "$id" "$label" "observed $observed"
  fi
}

BEFORE_WINDOWS=$("$OVM" hypr clients | jq '[.[].address]')
guest "printf 'Application icon integration fixture\n' >$(printf '%q' "$APPFILE")"
ctl openWithPath "$APPFILE" nvim.desktop
wait_for "[[ \$(field lastLaunchedPath) == '$APPFILE' && \$(field launchBusy) == false ]]" 20 || {
  fail E-39-01 "open the fixture with its desktop application" "launch did not finish"
  summary
}
VIEWER_PID=$(guest "python3 - $(printf '%q' "$APPFILE") <<'PYGUEST'
import os,sys
from pathlib import Path
for process in Path('/proc').iterdir():
    if not process.name.isdigit(): continue
    try:
        args=(process/'cmdline').read_bytes().split(b'\0')
        if args and Path(os.fsdecode(args[0])).name=='nvim' and os.fsencode(sys.argv[1]) in args:
            print(process.name)
            break
    except (OSError,ValueError):
        pass
PYGUEST" | tr -d '\r')
WINDOWS=$("$OVM" hypr clients | jq -c --argjson before "$BEFORE_WINDOWS" '[.[] | select(.address as $a | $before | index($a) | not) | {address,class,title,pid}]')
DISPLAY_TEXT=
for attempt in {1..10}; do
  DISPLAY_TEXT=$("$OVM" ocr)
  [[ $DISPLAY_TEXT == *"icon integration fixture"* ]] && break
  sleep 0.5
done
printf 'evidence 01 application screenshot %s\n' "$("$OVM" shot filetree-media-icons-E-39-01-opened)"
printf 'evidence 01 launch notice %s\n' "$(field launchError)"
if [[ $VIEWER_PID =~ ^[0-9]+$ && $DISPLAY_TEXT == *"icon integration fixture"* ]] && jq -e 'length>0' >/dev/null <<<"$WINDOWS"; then
  printf 'evidence 01 application pid %s path %s windows %s\n' "$VIEWER_PID" "$APPFILE" "$WINDOWS"
  evidence_pass 1 E-39-01 "catalogue icon identity and real application opening" '.[] | select(.id == $id) | .observed.desktop_id == "nvim.desktop" and .observed.application_icon != "" and .observed.source != "" and .observed.image_ready'
else
  fail E-39-01 "open the file with the desktop application" "pid=$VIEWER_PID windows=$WINDOWS error=$(field launchError) text=$DISPLAY_TEXT"
fi
evidence_pass 2 E-39-02 "bundled application mark renders without the generic glyph" '.[] | select(.id == $id) | (.observed.source | endswith("/assets/marks/herdr.svg")) and .observed.image_ready and (.observed.fallback_visible == false)'
evidence_pass 3 E-39-03 "explicit override glyph renders after a missing icon file" '.[] | select(.id == $id) | .observed.source == "" and .observed.rejected_sources == 1 and .observed.fallback_visible and .observed.fallback_text == "X"'
evidence_pass 4 E-39-04 "four real failed assets recover after descriptor reset" '.[] | select(.id == $id) | (.observed.source | endswith("/assets/marks/herdr.svg")) and .observed.image_ready and .observed.rejected_sources == 0 and (.observed.failed_sources | unique | length) == 4'

evidence_pass 5 E-39-05 "large application icon keeps aspect ratio under the 128 pixel decode cap" '.[] | select(.id == $id) | .observed.image_ready and .observed.source_width > 0 and .observed.source_width <= 128 and .observed.source_height > 0 and .observed.source_height <= 128 and .observed.aspect_ratio >= 1.99 and .observed.aspect_ratio <= 2.01'
evidence_pass 6 E-39-06 "legacy icon name, trusted source and fallback glyph retain their behavior" '.[] | select(.id == $id) | .observed.image_ready and .observed.icon_name == "nvim" and .observed.desktop_id == "" and (.observed.descriptor_path_active == false) and (.observed.trusted_source | endswith("/assets/marks/herdr.svg")) and .observed.source == .observed.trusted_source and .observed.legacy_glyph_visible and .observed.legacy_glyph == "L"'

printf 'qualification E-39-01 combines the real renderer/catalogue with public Open with dispatch, nvim process argv and a mapped application window; no mocked dispatch runs in evidence mode\n'
printf 'qualification E-39-04 uses four genuinely failed assets in an isolated desktop/theme fixture and observes recovery to herdr\n'
printf 'qualification E-39-06 observes the legacy descriptor guard; lookup counts are qualified separately by the blade-open audit\n'
guest "cat $(printf '%q' "$LOG")"
summary
