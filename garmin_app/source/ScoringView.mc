using Toybox.WatchUi;
using Toybox.Graphics;
using Toybox.Timer;
using Toybox.Lang;

//! The app's one screen. Draws whichever of three states applies —
//! waiting to be paired, paired but not yet configured with a match, or
//! ready to score — and, only in that last state, drives the periodic
//! queue flush against `agon_service` while it's the active view. See
//! `ScoringDelegate` for the input side (opening the scoring menu).
//!
//! Deliberately not using a layout resource (`resources/layouts/`) —
//! plain `dc.drawText` calls instead, so there's one less XML/id mapping
//! to get right without a compiler in this sandbox to check it against.
class ScoringView extends WatchUi.View {

    //! How often to attempt a flush while this view is visible. Not tied
    //! to any particular urgency — a queued goal shows up server-side
    //! within one tick either way; this is just "often enough to feel
    //! live without hammering the radio".
    const FLUSH_INTERVAL_MS = 15000;

    var _timer as Timer.Timer?;
    var _flushing as Boolean;
    var _pendingBatchSize as Number;
    var _lastSyncError as Boolean;

    // Session-local display counters — see the onUpdate comment below on
    // why these are just for the on-watch display, not the source of
    // truth.
    var _homeGoals as Number;
    var _awayGoals as Number;

    function initialize() {
        View.initialize();
        _flushing = false;
        _pendingBatchSize = 0;
        _lastSyncError = false;
        _homeGoals = 0;
        _awayGoals = 0;
    }

    function onShow() as Void {
        _timer = new Timer.Timer();
        _timer.start(method(:onFlushTick), FLUSH_INTERVAL_MS, true);
        flushQueue();
    }

    function onHide() as Void {
        if (_timer != null) {
            _timer.stop();
            _timer = null;
        }
    }

    function onFlushTick() as Void {
        flushQueue();
    }

    function onUpdate(dc as Graphics.Dc) as Void {
        dc.setColor(Graphics.COLOR_WHITE, Graphics.COLOR_BLACK);
        dc.clear();

        var app = AgonLiveScoringApp.get();
        var centerX = dc.getWidth() / 2;

        if (app.accessToken() == null) {
            drawWrapped(dc, centerX, dc.getHeight() / 2, WatchUi.loadResource(Rez.Strings.PairingPrompt));
            return;
        }

        if (!app.isConfigured()) {
            drawWrapped(dc, centerX, dc.getHeight() / 2, WatchUi.loadResource(Rez.Strings.MissingMatchConfig));
            return;
        }

        // Ready to score: a big score line plus a small sync-status line
        // underneath. The score shown is just this session's own tally of
        // goals tapped on this device (see recordGoal) — not fetched from
        // GET /matches/:id/score, so it can disagree with the real
        // server-derived score (another scoring device, an app restart).
        // Fetching and rendering that instead is a natural next step, not
        // done here — see the README's "Known gaps" section.
        dc.drawText(centerX, dc.getHeight() / 2 - 20, Graphics.FONT_NUMBER_MEDIUM,
            _homeGoals.toString() + " - " + _awayGoals.toString(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER);

        dc.drawText(centerX, dc.getHeight() / 2 + 30, Graphics.FONT_SMALL,
            syncStatusText(),
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER);
    }

    function recordGoal(sideId as String?, ownGoal as Boolean) as Void {
        if (sideId == null) {
            return;
        }
        var app = AgonLiveScoringApp.get();
        if (sideId.equals(app.homeSideId())) {
            _homeGoals += 1;
        } else if (sideId.equals(app.awaySideId())) {
            _awayGoals += 1;
        }
        app.eventQueue.enqueue(FootballEvents.wrapNewEvent(FootballEvents.goalEvent(sideId, ownGoal)));
        WatchUi.requestUpdate();
        flushQueue();
    }

    function recordPeriod(period as String) as Void {
        var app = AgonLiveScoringApp.get();
        app.eventQueue.enqueue(FootballEvents.wrapNewEvent(FootballEvents.periodEvent(period)));
        WatchUi.requestUpdate();
        flushQueue();
    }

    function syncStatusText() as String {
        if (_lastSyncError) {
            return WatchUi.loadResource(Rez.Strings.SyncStatusError);
        }
        var pending = AgonLiveScoringApp.get().eventQueue.pendingCount();
        if (pending == 0) {
            return WatchUi.loadResource(Rez.Strings.SyncStatusSynced);
        }
        return Lang.format(WatchUi.loadResource(Rez.Strings.SyncStatusPending), [pending]);
    }

    //! Try to send everything currently queued. A no-op if there's
    //! nothing queued, the device isn't configured yet, or a
    //! flush/seq-fetch is already in flight. See `AgonApiClient`'s doc
    //! comment for the request/response shapes this drives.
    function flushQueue() as Void {
        if (_flushing) {
            return;
        }
        var app = AgonLiveScoringApp.get();
        if (!app.isConfigured()) {
            return;
        }
        if (app.eventQueue.pendingCount() == 0) {
            return;
        }

        var lastSeq = app.eventQueue.lastAckedSeq();
        if (lastSeq == null) {
            _flushing = true;
            app.apiClient.fetchLiveSeq(app.matchId(), app.accessToken(), method(:onSeqFetchedForFlush));
            return;
        }
        sendBatch(lastSeq);
    }

    function sendBatch(lastSeq as Number) as Void {
        var app = AgonLiveScoringApp.get();
        var batch = app.eventQueue.nextBatch();
        if (batch.size() == 0) {
            return;
        }
        _flushing = true;
        _pendingBatchSize = batch.size();
        app.apiClient.appendLiveEvents(
            app.matchId(), app.accessToken(), lastSeq, batch, method(:onAppendForFlush)
        );
    }

    function onSeqFetchedForFlush(success as Boolean, responseCode as Number, data as Dictionary?) as Void {
        _flushing = false;
        if (!success || data == null || !data.hasKey("last_seq")) {
            _lastSyncError = true;
            WatchUi.requestUpdate();
            return;
        }
        AgonLiveScoringApp.get().eventQueue.setLastAckedSeq(data.get("last_seq"));
        flushQueue();
    }

    function onAppendForFlush(success as Boolean, responseCode as Number, data as Dictionary?) as Void {
        _flushing = false;
        var app = AgonLiveScoringApp.get();

        if (success && data != null && data.hasKey("last_seq")) {
            app.eventQueue.confirmSent(_pendingBatchSize, data.get("last_seq"));
            _lastSyncError = false;
            if (app.eventQueue.pendingCount() > 0) {
                flushQueue();
            }
        } else if (responseCode == 409) {
            // The log's tip moved under us (another device scored in the
            // meantime) — reconcile against the real tip and retry once.
            // A second 409 right after a fresh fetch would be a genuine,
            // unexpected conflict; left as a surfaced sync error rather
            // than looping, since retrying blindly could spin forever.
            _flushing = true;
            app.apiClient.fetchLiveSeq(app.matchId(), app.accessToken(), method(:onSeqFetchedForFlush));
        } else {
            _lastSyncError = true;
        }
        WatchUi.requestUpdate();
    }

    function drawWrapped(dc as Graphics.Dc, x as Number, y as Number, text as String) as Void {
        dc.drawText(x, y, Graphics.FONT_XTINY, text,
            Graphics.TEXT_JUSTIFY_CENTER | Graphics.TEXT_JUSTIFY_VCENTER);
    }
}
