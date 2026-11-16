#!/usr/bin/env sh
set -eu

data=${1:-video_analysis/data}
mkdir -p "$data"
model="$data/nanodet.onnx"
tmp=$(mktemp "$data/.nanodet.XXXXXX")
trap 'rm -f "$tmp"' EXIT
curl --fail --location --silent --show-error \
    'https://media.githubusercontent.com/media/opencv/opencv_zoo/main/models/object_detection_nanodet/object_detection_nanodet_2022nov.onnx' \
    --output "$tmp"
printf '%s  %s\n' '4b82da9944b88577175ee23a459dce2e26e6e4be573def65b1055dc2d9720186' "$tmp" | sha256sum -c -
mv "$tmp" "$model"
