# Football video analysis proof of concept

## Try it on your phone

Requires Python 3.12, Node.js 22 or newer, FFmpeg with the libx264 encoder, curl, and sha256sum. Run from the repository root.

```sh
python3 -m venv video_analysis/.venv
video_analysis/.venv/bin/python -m pip install -r video_analysis/requirements.txt
video_analysis/setup-model.sh
video_analysis/.venv/bin/python -m video_analysis.fixture
npm --prefix agon_ui ci
video_analysis/.venv/bin/python -m video_analysis.server --demo
```

In a second terminal, from the repository root:

```sh
VITE_SUPABASE_URL=http://127.0.0.1:54321 VITE_SUPABASE_ANON_KEY=fixture \
  npm --prefix agon_ui run dev -- --mode video-demo --host 0.0.0.0
```

Open `http://<computer-LAN-IP>:5173/matches/demo` on a phone connected to the same Wi-Fi. Use `http://127.0.0.1:5173/matches/demo` from the computer. This is a mobile browser demo of the Agon match page with an isolated fixture match API. It uses the actual match detail page, video panel and football result editor. The local HTTP URL is for browser testing. PWA installation from the phone requires HTTPS.

1. Leave the prefilled Monday Night Football URL or paste it. Tap **Check orientation**. The preview is generated fixture footage keyed to the pilot link for this demo.
2. Use **Rotate 90°** to correct orientation if needed. To redraw the goal, mark the two front posts and the back corners in order around the shape. The fixture starts with its goal marked and its bib colours set.
3. Check **Frame is upright**. Tap **Analyse video** and wait for the review queue.
4. Tap **Inspect footage** and play the clip. Choose a scoring side, then confirm or dismiss the candidate. You can repeat either action.
5. Open the reviewed goal in the existing result editor. Review the full score and tick the checkbox before **Save result**. The demo records a local pending result. To repeat from a fresh match, stop the demo worker, remove `video_analysis/data/analysis.sqlite3`, and restart it.

### Repeatable mobile browser check

With a fresh demo database, start both servers as above and run:

```sh
cd agon_ui
npx playwright install chromium
node video-demo/check.mjs
```

If using an existing Chromium installation, set `CHROME_BIN` to its executable. The check uses an iPhone-sized touch viewport. It checks auth, absent media, rotation, drawing, progress, candidate footage playback, dismissal, two confirmations of one candidate, persistence on reload, and the result editor’s outgoing score with exactly one goal.

## Use a retained recording with a local Agon stack

Start the Agon service and UI dependencies as described in [the local stack guide](../local/README.md). Keep all storage and services local. Set `VITE_VIDEO_ANALYSIS=1`, `AGON_API_PROXY_TARGET=http://127.0.0.1:7000` and `AGON_VIDEO_WORKER_URL=http://127.0.0.1:7002` in the Agon UI development environment, alongside its normal Supabase settings. Start the worker with its real authorization adapter:

```sh
video_analysis/.venv/bin/python -m video_analysis.server \
  --agon-url http://127.0.0.1:7000
```

Place the original or a verified clip under the worker data directory and add a match-bound entry to `sources.json`. The source resolver only accepts paths inside that directory and only serves entries for the specified match. Agon authenticates the caller via its match endpoint. The worker requires the owner or administrator role returned for the viewer, the same gate used by the match page.

```json
{
  "7g9XA8HWn8M": {
    "path": "recordings/monday.mp4",
    "matches": ["actual-agon-match-id"],
    "video_offset": 0,
    "start": 600,
    "kickoff": 600,
    "orientation": 0,
    "thresholds": { "pitch": [21, 15] }
  }
}
```

Use `video_offset: 600` if the retained file itself begins at YouTube 10:00. A source may supply a default `goal` polygon with normalized coordinates, a `ball` hex colour and threshold overrides from [worker.py](worker.py). The preview honours FFmpeg’s display transform before applying the organiser’s extra rotation. Use constant-frame-rate material for the initial benchmark. The optional [pilot probe](probe-youtube.sh) is separate from the app and only for an authorised media-access experiment.

SQLite holds analysis jobs and reviews. A repeated review updates the same candidate row. The existing football score API accepts a whole score and its goal list, with no source-specific goal ID. This POC therefore seeds the result editor only for a match without a pending or confirmed result and outside live scoring. Each candidate seeds at most one event. A fresh match read checks eligibility again before save. Review actions never save official scores. Video time remains in the analysis database, and the result editor gets the match minute derived from the configured kickoff. The import represents the visible window only. The organiser adds any missing goals before saving. Existing results can still use the ordinary result editor.

The worker runs one bounded queue inside one local process. Jobs interrupted by restart can be retried. A cloud deployment needs durable job dispatch, shared review storage and a retention policy. The proof of concept does not change Agon’s database schema, OAuth or publishing code.

## Pilot and fixture evidence

The Monday Night Football watch page resolved from the CPU runner. A ten-second cut at 10:00 failed in FFmpeg. A separate first-range request for the selected 1080p/30 MP4 returned HTTP 206 with `text/html` and 195 bytes (`Site Unavailable`). FFprobe rejected the response. The 10:00–12:00 decode failed at input open as well. No real-match frame count, orientation confirmation, visible-goal labels or detection metrics are available. Colab remains unverified.

The generated fixture decoded as 1920 × 1080 at 30 fps. FFprobe counted 300 frames in a 10-second cut and 3,600 in the source. The worker decoded all 480 required samples and inspected 222 native-rate frames across three short windows. In the mobile browser run it took 18.12 seconds (19.64 seconds in a separate direct worker run) on an Intel Xeon Platinum 8573C using FFmpeg 7.1.5, OpenCV 5.0 and OpenCV Zoo NanoDet Plus 416 on CPU. The runner suggested Blue at video 10:16.717 (match 0:16.717). The one scripted visible goal was found with the correct side, for zero missed visible goals, zero false candidates and 1/1 side agreement. The scripted side-net hit at 10:48 and save at 11:18 produced no candidates. These counts apply to synthetic footage only.

The event rule checks a moving ball-like contour crossing the marked line toward the net in the image. The pretrained detector identifies people at 4 fps, and short track histories stabilise bib classification. The nearest confident bib vote around the candidate can suggest a side. Results are uncertain under occlusion, small ball size, lookalike contours or camera movement. There is no heatmap, possession, named player output, or calibrated motion estimate. The opposite end is unobserved.

The second clip should be rights-cleared and include at least two visible goals across distinguishable scoring sides, saves, side-net hits, and play without goals under the same camera setup. Label goal-line crossing timestamps, scoring side, occlusions and coverage before running inference. If the real pilot contains no visible goals, it can contribute negatives only. Once a real benchmark exists, run the same frozen clips and thresholds on a mid-range Android and iPhone using an on-device ONNX runtime. Compare candidate agreement, 480-frame completeness, end-to-end latency including decode, peak memory, battery and thermal impact across back-to-back runs.

OpenCV Zoo’s NanoDet directory distributes the model under [Apache 2.0](https://github.com/opencv/opencv_zoo/blob/main/models/object_detection_nanodet/LICENSE). [COCO](https://cocodataset.org/dataset/termsofuse.htm) annotations are CC BY 4.0 and its training images have separate terms. This repo downloads the pretrained model and generates its own footage. FFmpeg’s license depends on its build. The libx264-enabled build uses GPL components, so verify distribution terms before shipping an image containing it. Use recordings and derived clips only with the organiser’s permission and within the applicable platform terms.

## Weekly publishing and Zoho

Agon currently stores the OAuth connection and can obtain channel information. This checkout has neither a recording publisher nor a retained-original hook. When publishing is implemented, use the original file once. Finalise it into private object storage, publish that same object through the [resumable YouTube upload API](https://developers.google.com/youtube/data/guides/using_resumable_upload_protocol), and save the returned video ID against the match and original object. Handle upload retries via the persisted publishing job and reconcile the final video ID before retrying an uncertain response. Analysis resolves the match and video ID to the original, and review clips can expire with the recording. A live broadcast would need a simultaneous local recording or an ingest copy.

Zoho Catalyst is viable for a hosted experiment. AppSail can serve the PWA/API and run a custom OCI image containing FFmpeg and OpenCV. Stratus can hold private source clips, and Catalyst Data Store or NoSQL can hold job and review records. AppSail’s HTTP request limit is 30 seconds and its maximum memory is 2 GB. Move durable work to Catalyst Job Scheduling with a Python Job Function (up to 15 minutes) if the native FFmpeg/OpenCV dependencies fit. Verify the package, CPU, memory and temporary disk limits in the desired region. If the runtime does not support those binaries or larger recordings, keep the worker on local hardware initially.

The Catalyst Development sandbox is documented as free. The production pay-as-you-go free tier advertises 900 AppSail GB-minutes, 25,000 Function GB-seconds and 5 GB Stratus storage per month, with usage and any project charge to verify in the account’s calculator. A budget calculation with four 45-minute recordings at 5 Mb/s gives roughly 6.75 GB of source video before clips. Short retention or smaller retained inputs are the main levers. A phone test can use Catalyst’s Development URL after a deployment is approved. The Zoho path in this document was assessed from public documentation and has not been deployed or benchmarked. Moving all of Agon into Zoho would also require adapting its current Supabase Auth, DynamoDB, S3, Temporal, SQS and Meilisearch integrations. Hosting the analysis slice on Zoho leaves those application dependencies in place.

- [Free tier](https://catalyst.zoho.com/free-tier/)
- [Development sandbox](https://docs.catalyst.zoho.com/en/deployment-and-billing/environments/development-environment/)
- [AppSail container support](https://catalyst.zoho.com/blog/custom-runtime-in-catalyst-appsail.html)
- [AppSail resource limits](https://docs.catalyst.zoho.com/en/faq/serverless/appsail-resource-limits/)
- [Job Functions](https://docs.catalyst.zoho.com/en/serverless/help/functions/job-functions/)
- [Function execution limits](https://docs.catalyst.zoho.com/en/faq/serverless/functions-exec-limit/)
