#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
src=assets/demos
out=assets/filetree-overview.gif
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
segments=(
  "readme-01-files 4.0 8.5"
  "readme-01-files 9.6 13.0"
  "readme-01-files 14.0 17.5"
  "readme-01-files 20.0 24.0"
  "readme-02-properties 4.6 8.0"
  "readme-02-properties 16.6 20.5"
  "readme-04-sections 3.2 7.5"
  "readme-04-sections 12.6 15.0"
  "readme-04-sections 18.6 21.0"
  "readme-05-plugins 11.6 15.0"
  "readme-05-plugins 16.2 19.0"
  "readme-05-plugins 21.6 25.0"
)
i=0
for segment in "${segments[@]}"; do
  read -r name start end <<<"$segment"
  ffmpeg -v error -y -ss "$start" -to "$end" -i "$src/$name.mp4" -an -c:v libx264 -preset veryfast -crf 16 "$work/$i.mp4"
  echo "file '$work/$i.mp4'" >>"$work/list.txt"
  i=$((i + 1))
done
ffmpeg -v error -y -f concat -safe 0 -i "$work/list.txt" -c copy "$work/all.mp4"
ffmpeg -v error -y -i "$work/all.mp4" \
  -filter_complex "fps=8,scale=1024:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=64:stats_mode=diff[p];[b][p]paletteuse=dither=none:diff_mode=rectangle" \
  "$out"
ls -l "$out"
