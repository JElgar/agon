"""Generate the project's rights-cleared synthetic test footage and media manifest."""
import json
import subprocess
import sys
from pathlib import Path

import cv2
import numpy as np

VIDEO_ID = "7g9XA8HWn8M"
WIDTH, HEIGHT, FPS = 1920, 1080, 30
MOUTH = [[0.78, 0.34], [0.78, 0.65], [0.855, 0.65], [0.855, 0.34]]
# Each tuple describes a shot: start time within the window, path, ground truth.
SHOTS = [
    (16, [(0.61, 0.49), (0.89, 0.49)], "goal"),
    (48, [(0.64, 0.74), (0.90, 0.74)], "side-net hit"),
    (78, [(0.63, 0.56), (0.765, 0.56), (0.66, 0.58)], "save"),
]


def pixels(point):
    return round(point[0] * WIDTH), round(point[1] * HEIGHT)


def generate(directory):
    directory.mkdir(parents=True, exist_ok=True)
    background = np.full((HEIGHT, WIDTH, 3), (59, 101, 48), np.uint8)
    for x in range(0, WIDTH, 240):
        shade = 102 if x % 480 == 0 else 113
        background[:, x:x + 240] = (59, shade, 48)
    cv2.rectangle(background, pixels((0.04, 0.1)), pixels((0.78, 0.9)), (195, 218, 202), 4)
    cv2.rectangle(background, pixels((0.65, 0.25)), pixels((0.78, 0.75)), (195, 218, 202), 4)
    cv2.line(background, pixels((0.39, 0.1)), pixels((0.39, 0.9)), (195, 218, 202), 4)
    cv2.ellipse(background, pixels((0.39, 0.5)), (110, 110), 0, 0, 360, (195, 218, 202), 4)
    cv2.fillPoly(background, [np.array([pixels(p) for p in MOUTH])], (86, 117, 81))
    for x in np.linspace(0.78, 0.855, 10):
        cv2.line(background, pixels((x, 0.34)), pixels((x, 0.65)), (135, 155, 132), 2)
    for y in np.linspace(0.34, 0.65, 16):
        cv2.line(background, pixels((0.78, y)), pixels((0.855, y)), (135, 155, 132), 2)
    cv2.polylines(background, [np.array([pixels(p) for p in MOUTH])], True, (213, 227, 215), 4)
    cv2.putText(background, "AGON / SYNTHETIC TEST FOOTAGE", (85, 75),
                cv2.FONT_HERSHEY_SIMPLEX, 1.1, (236, 242, 236), 2, cv2.LINE_AA)
    command = [
        "ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgr24",
        "-s", f"{WIDTH}x{HEIGHT}", "-r", str(FPS), "-i", "-", "-c:v", "libx264",
        "-threads", "2", "-preset", "ultrafast", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart", str(directory / "fixture.mp4"),
    ]
    with subprocess.Popen(command, stdin=subprocess.PIPE) as process:
        for index in range(120 * FPS):
            t = index / FPS
            frame = background.copy()
            for x, y, colour in [(0.58, 0.46, (230, 100, 55)), (0.47, 0.66, (230, 100, 55)),
                                 (0.70, 0.53, (80, 75, 224)), (0.62, 0.30, (80, 75, 224))]:
                cx, cy = pixels((x + 0.007 * np.sin(t + y * 10), y))
                cv2.ellipse(frame, (cx, cy + 22), (21, 9), 0, 0, 360, (43, 70, 40), -1)
                cv2.circle(frame, (cx, cy - 19), 9, (119, 158, 187), -1, cv2.LINE_AA)
                cv2.ellipse(frame, (cx, cy), (15, 20), 0, 0, 360, colour, -1, cv2.LINE_AA)
                cv2.line(frame, (cx - 5, cy + 12), (cx - 9, cy + 29), (35, 30, 25), 5)
                cv2.line(frame, (cx + 5, cy + 12), (cx + 9, cy + 29), (35, 30, 25), 5)
            for start, path, _ in SHOTS:
                elapsed = (t - start) / 1.2
                if 0 <= elapsed < 1:
                    positions = np.array(path)
                    point = [np.interp(elapsed, np.linspace(0, 1, len(path)), axis) for axis in positions.T]
                    cv2.circle(frame, pixels(point), 9, (12, 152, 254), -1, cv2.LINE_AA)
            process.stdin.write(frame.tobytes())
        process.stdin.close()
        if process.wait():
            raise RuntimeError("Fixture encoding failed")
    manifest = {
        VIDEO_ID: {
            "path": "fixture.mp4", "video_offset": 600, "fixture": True,
            "goal": MOUTH, "ball": "#fe980c", "orientation": 0,
        }
    }
    (directory / "sources.json").write_text(json.dumps(manifest, indent=2) + "\n")
    labels = [{"from": 600 + start, "to": 601.2 + start, "label": label,
               **({"side": "blue"} if label == "goal" else {})} for start, _, label in SHOTS]
    (directory / "fixture-labels.json").write_text(json.dumps(labels, indent=2) + "\n")


if __name__ == "__main__":
    generate(Path(sys.argv[1] if len(sys.argv) > 1 else "video_analysis/data"))
