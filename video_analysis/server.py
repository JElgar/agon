"""Opt-in local analysis sidecar. Start one instance with python -m video_analysis.server."""
import argparse
from base64 import b64encode
import hashlib
import json
import logging
import re
import sqlite3
import subprocess
from tempfile import TemporaryDirectory
from concurrent.futures import ThreadPoolExecutor
from contextlib import closing
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import parse_qs, quote, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

import cv2
import numpy as np
import uvicorn
from fastapi import FastAPI, HTTPException, Header
from fastapi.responses import FileResponse, Response
from pydantic import BaseModel, Field

from .worker import DEFAULTS, analyse, probe, render

VIDEO_ID = re.compile(r"[A-Za-z0-9_-]{11}")
COLOUR = re.compile(r"#[a-fA-F0-9]{6}")
MATCH_ID = re.compile(r"[A-Za-z0-9_-]{1,100}")
MAX_JOBS = 4


class Preview(BaseModel):
    youtube: str = Field(max_length=500)
    rotation: int | None = None


class Start(Preview):
    rotation: int
    start: int = Field(ge=0, le=86400)
    seconds: int = Field(ge=1, le=120)
    kickoff: int = Field(ge=0)
    goal: list[tuple[float, float]]
    bibs: dict[str, str] = Field(max_length=2)
    upright_checked: bool


class Review(BaseModel):
    decision: str
    side: str | None = None


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def youtube_id(link):
    try:
        parsed = urlsplit(link.strip())
        host = parsed.hostname
        if parsed.scheme == "https" and host in {"youtube.com", "www.youtube.com", "m.youtube.com"} and parsed.path == "/watch":
            value = parse_qs(parsed.query).get("v", [""])[0]
        elif parsed.scheme == "https" and host == "youtu.be":
            value = parsed.path.strip("/")
        else:
            value = ""
        if VIDEO_ID.fullmatch(value):
            return value
    except ValueError:
        pass
    raise HTTPException(422, "Paste a YouTube watch or share link.")


def create_app(directory, agon_url, demo):
    if not demo and not agon_url:
        raise ValueError("--agon-url is required outside the fixture demo")
    directory = directory.resolve()
    directory.mkdir(parents=True, exist_ok=True)
    db_path = directory / "analysis.sqlite3"
    model = directory / "nanodet.onnx"
    pool = ThreadPoolExecutor(max_workers=1)
    opener = build_opener(NoRedirect)
    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None)
    demo_match = {
        "id": "demo", "name": "Monday Night Football", "match_type": "football",
        "description": "Synthetic footage demo", "status": "scheduled",
        "starts_at": "2025-05-19T19:00:00Z", "created_at": "2025-05-18T18:00:00Z",
        "viewer_role": "owner", "players": [], "header_photos": [], "ranked": False,
        "allow_unassigned": False,
        "sides": [
            {"id": "blue", "name": "Blue", "colour": "#3764e6", "side_admin_user_ids": []},
            {"id": "red", "name": "Red", "colour": "#e04b50", "side_admin_user_ids": []},
        ],
        "social": {"i_liked": False, "like_count": 0, "comment_count": 0},
    }

    def db():
        connection = sqlite3.connect(db_path, timeout=10)
        connection.row_factory = sqlite3.Row
        return closing(connection)

    with db() as conn, conn:
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY, match_id TEXT NOT NULL, options TEXT NOT NULL,
                stage TEXT NOT NULL, percent INTEGER NOT NULL, result TEXT, error TEXT
            );
            CREATE TABLE IF NOT EXISTS reviews (
                job TEXT NOT NULL, candidate INTEGER NOT NULL, decision TEXT NOT NULL,
                side TEXT, PRIMARY KEY (job, candidate)
            );
            CREATE TABLE IF NOT EXISTS demo_scores (id INTEGER PRIMARY KEY, pending_score TEXT NOT NULL);
        """)
        conn.execute("UPDATE jobs SET stage='Failed', error='Worker restarted. Tap Analyse video to retry.' "
                     "WHERE stage NOT IN ('Ready','Failed')")

    @app.middleware("http")
    async def private_responses(request, call_next):
        response = await call_next(request)
        response.headers["Cache-Control"] = "no-store"
        return response

    def get_demo_match():
        with db() as conn:
            score = conn.execute("SELECT pending_score FROM demo_scores WHERE id=1").fetchone()
        return {**demo_match, **({"pending_score": json.loads(score[0])} if score else {})}

    def authorise(match, authorization):
        if not MATCH_ID.fullmatch(match):
            raise HTTPException(404)
        if demo:
            if match != "demo":
                raise HTTPException(404)
            if authorization != "Bearer fixture-organiser":
                raise HTTPException(403, "An organiser must review this footage.")
            info = get_demo_match()
        else:
            if not authorization or not authorization.startswith("Bearer "):
                raise HTTPException(401)
            request = Request(f"{agon_url.rstrip('/')}/matches/{quote(match, safe='')}",
                              headers={"Authorization": authorization, "Accept": "application/json"})
            try:
                with opener.open(request, timeout=8) as response:
                    info = json.load(response)
            except HTTPError as exc:
                raise HTTPException(exc.code if exc.code in (401, 403, 404) else 502,
                                    "Could not verify match access.") from exc
            except (URLError, ValueError, TimeoutError) as exc:
                raise HTTPException(502, "Could not verify match access.") from exc
        if info.get("viewer_role") not in ("owner", "admin"):
            raise HTTPException(403, "An organiser must review this footage.")
        if info.get("match_type") != "football" or info.get("status") == "cancelled":
            raise HTTPException(409, "Video analysis requires an active football match.")
        return {side["id"] for side in info["sides"]}

    def source_for(match, identifier):
        try:
            entry = json.loads((directory / "sources.json").read_text())[identifier]
            media = (directory / entry["path"]).resolve()
            if (not media.is_relative_to(directory) or not media.is_file()
                    or (not demo and match not in entry.get("matches", []))):
                raise KeyError(identifier)
            return entry, media
        except (KeyError, FileNotFoundError):
            raise HTTPException(404, "No retained recording is associated with this match and link.")

    def rotation_ok(rotation):
        if rotation not in (0, 90, 180, 270):
            raise HTTPException(422, "Choose a right-angle rotation.")
        return rotation

    def change(job, stage, percent):
        with db() as conn, conn:
            conn.execute("UPDATE jobs SET stage=?, percent=? WHERE id=?", (stage, percent, job))

    def run(job, path, options):
        clips = directory / job
        try:
            clips.mkdir(exist_ok=True)
            result = analyse(path, model, options, lambda stage, percent: change(job, stage, percent), clips)
            with db() as conn, conn:
                conn.execute("UPDATE jobs SET stage='Ready', percent=100, result=? WHERE id=?",
                             (json.dumps(result), job))
        except Exception:
            logging.exception("Video analysis failed")
            with db() as conn, conn:
                conn.execute("UPDATE jobs SET stage='Failed', error=? WHERE id=?",
                             ("The worker could not decode or analyse this recording. Check its window and worker log.", job))

    def job_data(conn, match, job):
        row = conn.execute("SELECT * FROM jobs WHERE id=? AND match_id=?", (job, match)).fetchone()
        if row is None:
            raise HTTPException(404)
        return row

    prefix = "/video-analysis/{match}"

    @app.post(prefix + "/preview")
    def preview(match: str, body: Preview, authorization: str | None = Header(None)):
        authorise(match, authorization)
        identifier = youtube_id(body.youtube)
        entry, path = source_for(match, identifier)
        rotation = rotation_ok(entry.get("orientation", 0) if body.rotation is None else body.rotation)
        offset = entry.get("video_offset", 0)
        try:
            info = probe(path)
            start = entry.get("start", max(600, offset))
            with TemporaryDirectory(dir=directory) as temporary:
                image = Path(temporary) / "frame.jpg"
                render(path, start - offset, 1, rotation, image)
                encoded = b64encode(image.read_bytes()).decode()
        except (OSError, ValueError, subprocess.SubprocessError):
            raise HTTPException(422, "The retained recording could not produce an orientation preview.")
        return {
            "image": f"data:image/jpeg;base64,{encoded}", "rotation": rotation,
            "goal": entry.get("goal") if rotation == entry.get("orientation", 0) else None,
            "source": info, "fixture": bool(entry.get("fixture")), "start": start,
            "seconds": min(120, int(info["duration"] - (start - offset))),
            "kickoff": entry.get("kickoff", start), "pitch": entry.get("thresholds", {}).get("pitch", DEFAULTS["pitch"]),
        }

    @app.post(prefix + "/jobs")
    def start_job(match: str, body: Start, authorization: str | None = Header(None)):
        sides = authorise(match, authorization)
        identifier = youtube_id(body.youtube)
        entry, path = source_for(match, identifier)
        offset = entry.get("video_offset", 0)
        rotation_ok(body.rotation)
        if not body.upright_checked:
            raise HTTPException(422, "Verify the frame orientation before analysing.")
        if body.start < max(offset, body.kickoff):
            raise HTTPException(422, "The analysis window must start after recording and kick-off.")
        if (not body.bibs.keys() <= sides or
                any(not COLOUR.fullmatch(c) for c in body.bibs.values())):
            raise HTTPException(422, "Choose bib colours for the match sides.")
        if (len(body.goal) != 4 or not all(0 <= n <= 1 for point in body.goal for n in point)):
            raise HTTPException(422, "Mark the four corners of the visible goal.")
        contour = np.array(body.goal, dtype=np.float32)
        if not cv2.isContourConvex(contour) or cv2.contourArea(contour) < 0.0001:
            raise HTTPException(422, "Mark the two mouth corners, then the two back corners in order.")
        if not model.is_file():
            raise HTTPException(503, "The local detector model is missing.")
        options = {**body.model_dump(exclude={"youtube", "upright_checked"}),
                   "offset": offset, "ball": entry.get("ball"),
                   "thresholds": entry.get("thresholds", {}), "video_id": identifier}
        encoded = json.dumps(options, sort_keys=True)
        job = hashlib.sha256(f"{match}/{path.stat().st_mtime_ns}/{encoded}".encode()).hexdigest()
        with db() as conn, conn:
            conn.execute("BEGIN IMMEDIATE")
            existing = conn.execute("SELECT stage FROM jobs WHERE id=?", (job,)).fetchone()
            if existing and existing[0] != "Failed":
                return {"id": job}
            pending = conn.execute("SELECT COUNT(*) FROM jobs WHERE stage NOT IN ('Ready','Failed')").fetchone()[0]
            if pending >= MAX_JOBS:
                raise HTTPException(429, "The worker is busy. Try again shortly.")
            conn.execute("INSERT INTO jobs VALUES (?, ?, ?, 'Queued', 0, NULL, NULL) "
                         "ON CONFLICT(id) DO UPDATE SET stage='Queued', percent=0, error=NULL, result=NULL",
                         (job, match, encoded))
        pool.submit(run, job, path, options)
        return {"id": job}

    @app.get(prefix + "/jobs/{job}")
    def status(match: str, job: str, authorization: str | None = Header(None)):
        authorise(match, authorization)
        with db() as conn:
            row = job_data(conn, match, job)
            result = json.loads(row["result"]) if row["result"] else None
            if result:
                reviewed = {r["candidate"]: r for r in conn.execute(
                    "SELECT * FROM reviews WHERE job=?", (job,))}
                for index, candidate in enumerate(result["candidates"]):
                    review = reviewed.get(index)
                    candidate.update(id=index, decision=review["decision"] if review else "pending",
                                     side=review["side"] if review else candidate["suggested_side"])
        return {"stage": row["stage"], "percent": row["percent"], "error": row["error"],
                "video_id": json.loads(row["options"])["video_id"], "result": result}

    @app.put(prefix + "/jobs/{job}/review/{candidate}")
    def review(match: str, job: str, candidate: int, body: Review, authorization: str | None = Header(None)):
        sides = authorise(match, authorization)
        if (body.decision not in ("confirmed", "dismissed") or
                (body.decision == "confirmed" and body.side not in sides)):
            raise HTTPException(422, "Select a scoring side to confirm this goal.")
        with db() as conn, conn:
            row = job_data(conn, match, job)
            if not row["result"] or not 0 <= candidate < len(json.loads(row["result"])["candidates"]):
                raise HTTPException(404)
            conn.execute("INSERT INTO reviews VALUES (?, ?, ?, ?) "
                         "ON CONFLICT(job,candidate) DO UPDATE SET decision=excluded.decision, side=excluded.side",
                         (job, candidate, body.decision, body.side if body.decision == "confirmed" else None))
        return Response(status_code=204)

    @app.get(prefix + "/jobs/{job}/clips/{candidate}")
    def clip(match: str, job: str, candidate: int, authorization: str | None = Header(None)):
        authorise(match, authorization)
        with db() as conn:
            row = job_data(conn, match, job)
        if not row["result"] or not 0 <= candidate < len(json.loads(row["result"])["candidates"]):
            raise HTTPException(404)
        return FileResponse(directory / row["id"] / f"{candidate}.mp4", media_type="video/mp4")

    if demo:
        @app.get("/matches/demo")
        def demo_get():
            return get_demo_match()

        @app.patch("/matches/demo")
        def demo_patch(body: dict):
            score = body.get("score")
            if not isinstance(score, dict) or score.get("type") != "Football":
                raise HTTPException(422, "Expected a football score.")
            with db() as conn, conn:
                conn.execute("INSERT INTO demo_scores VALUES (1, ?) ON CONFLICT(id) "
                             "DO UPDATE SET pending_score=excluded.pending_score",
                             (json.dumps({"score": score, "submission_id": "fixture-result", "confirmations": [],
                                          "winner_side_id": body.get("winner_side_id")}),))
            return get_demo_match()

        @app.get("/users/me")
        def demo_user():
            return {"profile": {"id": "demo-organiser", "name": "Demo organiser"}}

        @app.get("/matches/demo/comments")
        def demo_comments():
            return {"items": []}

        @app.get("/matches/demo/join-requests")
        @app.get("/matches/demo/waitlist")
        def demo_empty():
            return []

    return app


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", type=Path, default=Path("video_analysis/data"))
    parser.add_argument("--agon-url", help="Trusted Agon backend URL, without a trailing slash")
    parser.add_argument("--demo", action="store_true", help="Use the isolated synthetic match API")
    parser.add_argument("--port", type=int, default=7002)
    args = parser.parse_args()
    uvicorn.run(create_app(args.data, args.agon_url, args.demo), host="127.0.0.1", port=args.port)
