#!/usr/bin/env bash
set -euo pipefail
(( $# >= 2 && $# <= 4 )) || { printf '%s\n' 'Usage: hold-space-near.sh X Y [REACH_PX] [HOLD_MS]'; exit 1; }
x=${1:?x}; y=${2:?y}; reach=${3:-10}; hold=${4:-700}
limit=$((reach * reach))
for _ in $(seq 1 12000); do
  read -r cx cy < <(hyprctl cursorpos | tr -d ',')
  dx=$((cx - x)); dy=$((cy - y))
  if (( dx * dx + dy * dy <= limit )); then
    wtype -P space -s "$hold" -p space
    exit 0
  fi
  sleep 0.005
done
echo "hold-space-near: the pointer never reached $x,$y" >&2
exit 1
