#!/usr/bin/env sh
# Experimental only. The application reads its local, match-bound source manifest.
set -eu

link='https://www.youtube.com/watch?v=7g9XA8HWn8M'
out=${1:-video_analysis/data/pilot}
mkdir -p "$out"
rm -f "$out/ten-seconds.mp4" "$out/two-minutes-4fps.mp4"
yt-dlp --no-playlist -f 'bv[height=1080][fps=30][ext=mp4]' \
    --download-sections '*00:10:00-00:10:10' --force-keyframes-at-cuts \
    -o "$out/ten-seconds.mp4" "$link"
ffprobe -v error -count_frames -select_streams v:0 \
    -show_entries stream=width,height,avg_frame_rate,nb_read_frames:format=duration \
    -of json "$out/ten-seconds.mp4"
count=$(ffprobe -v error -count_frames -select_streams v:0 \
    -show_entries stream=nb_read_frames -of csv=p=0 "$out/ten-seconds.mp4")
test "$count" = 300
ffmpeg -v error -y -ss 600 -i "$(yt-dlp --no-playlist -f 'bv[height=1080][fps=30][ext=mp4]' -g "$link")" \
    -t 120 -an -vf fps=4 -c:v libx264 "$out/two-minutes-4fps.mp4"
ffprobe -v error -count_frames -select_streams v:0 \
    -show_entries stream=width,height,avg_frame_rate,nb_read_frames -of json "$out/two-minutes-4fps.mp4"
count=$(ffprobe -v error -count_frames -select_streams v:0 \
    -show_entries stream=nb_read_frames -of csv=p=0 "$out/two-minutes-4fps.mp4")
test "$count" = 480
