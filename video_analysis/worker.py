"""Local CPU inference. Coordinates are normalized against the oriented frame."""
import json
import subprocess
import time
from fractions import Fraction

import cv2
import numpy as np

MODEL = "OpenCV Zoo NanoDet Plus 416 (COCO, 2022nov)"
SAMPLE_FPS = 4
DEFAULTS = {
    "pitch": [21, 15], "player_confidence": 0.35, "track_distance": 0.08,
    "colour_distance": 85, "side_radius": 0.19, "side_margin": 0.035,
    "window_seconds": 0.75, "goal_cooldown": 2, "ball_hue_delta": 12,
    "ball_area": [10, 700], "ball_circularity": 0.58, "goal_margin": 0.18,
}


def probe(path):
    result = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_streams",
         "-show_format", "-of", "json", str(path)],
        check=True, capture_output=True, timeout=15,
    )
    data = json.loads(result.stdout)
    if not data.get("streams"):
        raise ValueError("The source has no decodable video stream.")
    stream = data["streams"][0]
    return {
        "width": stream["width"], "height": stream["height"],
        "fps": str(Fraction(stream["avg_frame_rate"])),
        "duration": float(data["format"]["duration"]),
        "rotation_metadata": next((s["rotation"] for s in stream.get("side_data_list", [])
                                   if "rotation" in s), 0),
    }


def filters(rotation):
    # FFmpeg applies display metadata first. This is the organiser's extra rotation.
    return {0: [], 90: ["transpose=clock"], 180: ["hflip", "vflip"], 270: ["transpose=cclock"]}[rotation]


def decode(path, start, seconds, rotation, width, height, fps=None):
    transforms = filters(rotation) + [f"scale={width}:{height}"]
    if fps is not None:
        transforms.append(f"fps={fps}")
    command = [
        "ffmpeg", "-v", "error", "-nostdin", "-threads", "2", "-ss", f"{start:.6f}",
        "-i", str(path), "-t", f"{seconds:.6f}", "-an", "-sn", "-vf", ",".join(transforms),
        "-filter_threads", "1", "-threads", "1", "-fps_mode", "passthrough",
        "-f", "rawvideo", "-pix_fmt", "bgr24", "-",
    ]
    with subprocess.Popen(command, stdout=subprocess.PIPE) as process:
        try:
            size = width * height * 3
            while chunk := process.stdout.read(size):
                if len(chunk) != size:
                    raise ValueError("FFmpeg returned an incomplete frame.")
                yield np.frombuffer(chunk, np.uint8).reshape(height, width, 3)
            if process.wait():
                raise ValueError("FFmpeg could not decode the video window.")
        finally:
            if process.poll() is None:
                process.kill()


def render(path, start, seconds, rotation, destination):
    command = ["ffmpeg", "-v", "error", "-nostdin", "-y", "-ss", f"{start:.6f}",
               "-threads", "2", "-i", str(path), "-t", str(seconds), "-an",
               "-filter_threads", "1", "-vf", ",".join(filters(rotation) + ["scale='min(960,iw)':-2"])]
    if destination.suffix == ".jpg":
        command += ["-frames:v", "1", "-threads", "1", "-update", "1"]
    else:
        command += ["-threads", "2", "-c:v", "libx264", "-preset", "fast",
                    "-pix_fmt", "yuv420p", "-movflags", "+faststart"]
    subprocess.run(command + [str(destination)], check=True, timeout=50)
    if not destination.is_file() or destination.stat().st_size == 0:
        raise ValueError("FFmpeg did not produce review media.")


def bgr(colour):
    return np.array([int(colour[i:i + 2], 16) for i in (5, 3, 1)], dtype=np.float32)


class Players:
    def __init__(self, path, bibs, config):
        self.net = cv2.dnn.readNet(str(path))
        self.outputs = self.net.getUnconnectedOutLayersNames()
        self.bibs = {side: bgr(colour) for side, colour in bibs.items()}
        self.config = config
        self.tracks = {}
        self.next_id = 0
        self.centres = {}
        for stride in (8, 16, 32):
            grid = (np.indices((416 // stride, 416 // stride))[::-1]
                    .reshape(2, -1).T * stride + (stride - 1) / 2)
            self.centres[len(grid)] = (stride, grid)

    def sample(self, frame, sample):
        height, width = frame.shape[:2]
        scale = 416 / max(height, width)
        scaled = cv2.resize(frame, (round(width * scale), round(height * scale)))
        padded = np.zeros((416, 416, 3), dtype=np.uint8)
        padded[:scaled.shape[0], :scaled.shape[1]] = scaled
        values = (padded.astype(np.float32) - [103.53, 116.28, 123.675]) / [57.375, 57.12, 58.395]
        self.net.setInput(cv2.dnn.blobFromImage(values.astype(np.float32)))
        outputs = [out[0] for out in self.net.forward(self.outputs)]
        regressions = {len(out): out for out in outputs if out.shape[-1] == 32}
        boxes, scores = [], []
        for out in (o for o in outputs if o.shape[-1] == 80):
            indexes = np.flatnonzero((out[:, 0] >= self.config["player_confidence"]) &
                                     (out.argmax(axis=1) == 0))
            if not len(indexes):
                continue
            stride, grid = self.centres[len(out)]
            logits = regressions[len(out)][indexes].reshape(-1, 4, 8)
            weights = np.exp(logits - logits.max(axis=-1, keepdims=True))
            distances = (weights @ np.arange(8)) / weights.sum(axis=-1) * stride
            coords = (np.tile(grid[indexes], 2) + distances * [-1, -1, 1, 1]) / scale
            for (x1, y1, x2, y2), confidence in zip(coords.round().astype(int), out[indexes, 0]):
                x1, x2 = np.clip([x1, x2], 0, width)
                y1, y2 = np.clip([y1, y2], 0, height)
                if x2 > x1 and y2 > y1:
                    boxes.append([int(x1), int(y1), int(x2 - x1), int(y2 - y1)])
                    scores.append(float(confidence))
        kept = cv2.dnn.NMSBoxes(boxes, scores, self.config["player_confidence"], 0.55) if boxes else []
        self.tracks = {key: item for key, item in self.tracks.items() if sample - item[0] <= 3}
        used, observations = set(), []
        for index in kept:
            x, y, w, h = boxes[int(index)]
            point = np.array([(x + w / 2) / width, (y + h * 0.7) / height])
            shirt = frame[y + h // 5:y + max(h // 5 + 1, h // 2),
                          x + w // 4:x + max(w // 4 + 1, 3 * w // 4)]
            distances = sorted((np.linalg.norm(np.median(shirt, axis=(0, 1)) - colour), side)
                               for side, colour in self.bibs.items())
            side = distances[0][1] if distances and distances[0][0] < self.config["colour_distance"] else None
            if len(distances) > 1 and distances[1][0] - distances[0][0] < 22:
                side = None
            nearby = [(np.linalg.norm(point - p), key) for key, (_, p, _) in self.tracks.items() if key not in used]
            distance, key = min(nearby, default=(1, self.next_id))
            if distance > self.config["track_distance"]:
                key = self.next_id
                self.next_id += 1
            votes = (self.tracks[key][2] if key in self.tracks else []) + [side]
            votes = votes[-5:]
            self.tracks[key] = (sample, point, votes)
            used.add(key)
            stable = side if side is not None and votes.count(side) >= 2 else None
            observations.append((point, stable))
        return observations


class GoalArea:
    def __init__(self, points, colour, config):
        self.points = np.array(points, np.float32)
        self.colour = None if colour is None else int(cv2.cvtColor(
            bgr(colour).astype(np.uint8)[None, None, :], cv2.COLOR_BGR2HSV)[0, 0, 0])
        self.config = config
        margin = config["goal_margin"]
        self.low = np.maximum(0, self.points.min(axis=0) - margin)
        self.high = np.minimum(1, self.points.max(axis=0) + margin)
        self.a, self.b = self.points[:2]
        self.edge = self.b - self.a
        self.sign = np.sign(self.distance(self.points[2:].mean(axis=0)))

    def distance(self, point):
        x, y = point - self.a
        return (self.edge[0] * y - self.edge[1] * x) / np.linalg.norm(self.edge)

    def ball(self, frame):
        size = np.array(frame.shape[:2][::-1])
        low = (self.low * size).astype(int)
        high = (self.high * size).astype(int)
        left, top = low
        right, bottom = high
        hsv = cv2.cvtColor(frame[top:bottom, left:right], cv2.COLOR_BGR2HSV)
        hue, saturation, value = cv2.split(hsv)
        if self.colour is None:
            mask = (saturation < 72) & (value > 192)
        else:
            diff = np.abs(hue.astype(np.int16) - self.colour)
            mask = (np.minimum(diff, 180 - diff) <= self.config["ball_hue_delta"]) & (saturation > 100) & (value > 130)
        contours, _ = cv2.findContours(mask.astype(np.uint8), cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
        low_area, high_area = np.array(self.config["ball_area"]) * np.prod(size) / (640 * 360)
        options = []
        for contour in contours:
            area = cv2.contourArea(contour)
            if not max(2, low_area) <= area <= high_area:
                continue
            perimeter = cv2.arcLength(contour, True)
            circularity = 4 * np.pi * area / perimeter ** 2 if perimeter else 0
            if circularity >= self.config["ball_circularity"]:
                (x, y), _ = cv2.minEnclosingCircle(contour)
                options.append((circularity, np.array([x + left, y + top]) / size))
        return max(options, key=lambda item: item[0])[1] if options else None

    def crosses(self, before, after):
        outside, inside = self.sign * self.distance(before), self.sign * self.distance(after)
        if outside >= 0 or inside < 0 or np.linalg.norm(after - before) > 0.05:
            return False
        hit = before + (after - before) * (-outside / (inside - outside))
        along = np.dot(hit - self.a, self.edge) / np.dot(self.edge, self.edge)
        return 0.04 <= along <= 0.96


def suggested_side(observations, point, config):
    distances = {}
    for position, side in observations:
        if side is not None:
            distances[side] = min(distances.get(side, 1), np.linalg.norm(position - point))
    ranked = sorted((distance, side) for side, distance in distances.items())
    if not ranked or ranked[0][0] > config["side_radius"]:
        return None
    if len(ranked) > 1 and ranked[1][0] - ranked[0][0] < config["side_margin"]:
        return None
    return ranked[0][1]


def analyse(path, model, options, progress, clip_dir):
    started = time.monotonic()
    cv2.setNumThreads(1)
    source = probe(path)
    start, seconds, offset = options["start"], options["seconds"], options["offset"]
    rotation = options["rotation"]
    config = DEFAULTS | options.get("thresholds", {})
    area = GoalArea(options["goal"], options["ball"], config)
    players = Players(model, options["bibs"], config)
    native_fps = float(Fraction(source["fps"]))
    source_width, source_height = source["width"], source["height"]
    if (source["rotation_metadata"] + rotation) % 180:
        source_width, source_height = source_height, source_width
    width = min(640, source_width)
    height = round(width * source_height / source_width)
    wanted = SAMPLE_FPS * seconds
    windows, observations = [], []
    previous = None
    sightings = 0
    progress("Sampling players", 3)
    for index, frame in enumerate(decode(path, start - offset, seconds, rotation, width, height, SAMPLE_FPS)):
        observations.append(players.sample(frame, index))
        ball = area.ball(frame)
        if ball is not None and (previous is None or np.linalg.norm(ball - previous) > 0.006):
            t = index / SAMPLE_FPS
            low = max(0, t - config["window_seconds"])
            high = min(seconds, t + config["window_seconds"])
            if windows and low <= windows[-1][1]:
                windows[-1][1] = high
            else:
                windows.append([low, high])
            sightings += 1
        previous = ball
        if index % 16 == 0:
            progress("Sampling players", min(73, 5 + round(68 * (index + 1) / wanted)))
    if len(observations) != wanted:
        raise ValueError(f"Expected {wanted} decoded frames at 4 fps, got {len(observations)}.")
    progress("Inspecting goalmouth at source frame rate", 75)
    candidates, inspected = [], 0
    for low, high in windows:
        previous = None
        for index, frame in enumerate(decode(path, start - offset + low, high - low, rotation,
                                            source_width, source_height)):
            t = low + index / native_fps
            inspected += 1
            ball = area.ball(frame)
            if (ball is not None and previous is not None and area.crosses(previous, ball)
                    and (not candidates or start + t - candidates[-1]["video_time"] >= config["goal_cooldown"])):
                sample = min(wanted - 1, max(0, round((t - 0.5) * SAMPLE_FPS)))
                side = suggested_side(observations[sample], previous, config)
                candidates.append({"video_time": round(start + t, 3),
                                   "match_time": round(start + t - options["kickoff"], 3),
                                   "suggested_side": side})
            previous = ball
    progress("Preparing footage for review", 93)
    for index, candidate in enumerate(candidates):
        seek = max(0, candidate["video_time"] - start - 2)
        render(path, start - offset + seek, min(5, seconds - seek), rotation, clip_dir / f"{index}.mp4")
    return {
        "source": source, "model": MODEL, "sample_fps": SAMPLE_FPS, "sample_frames": len(observations),
        "goalmouth_frames": inspected, "goalmouth_windows": len(windows), "ball_triggers": sightings,
        "player_observations": sum(map(len, observations)), "tracks": players.next_id,
        "wall_seconds": round(time.monotonic() - started, 2), "candidates": candidates,
        "visible_end": "observed", "other_end": "unobserved", "pitch": config["pitch"],
    }
