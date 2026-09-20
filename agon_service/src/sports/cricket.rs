//! Cricket's API↔DAO mapping — colocated here instead of scattered across
//! `mapping.rs` (see `crate::sports::netball`'s doc comment for the pattern
//! and its rationale).

use std::collections::HashMap;

use agon_core::dao::records::MatchFormatRecord;
use agon_core::sports::cricket::{
    CricketBattingEntryRecord, CricketBowlingEntryRecord, CricketDeliveryExtraRecord,
    CricketDeliveryRecord, CricketDeliveryWicketRecord, CricketDismissalKindRecord,
    CricketDismissalRecord, CricketExtraKindRecord, CricketExtrasRecord, CricketFallOfWicketRecord,
    CricketFormatRecord, CricketInningsEndEventRecord, CricketInningsStartEventRecord,
    CricketLiveEventRecord, CricketRetireEventRecord, CricketScoreInningsRecord,
    CricketScoreRecord, InningsEndReasonRecord, NextBallContextRecord, OversRecord,
};

use crate::detailed_score::cricket::{
    CricketBattingEntry, CricketBowlingEntry, CricketDelivery, CricketDeliveryExtra,
    CricketDeliveryWicket, CricketDismissal, CricketDismissalKind, CricketExtraKind, CricketExtras,
    CricketFallOfWicket, NextBallContext, Overs,
};
use crate::live_score::cricket::{
    CricketInningsEndEvent, CricketInningsStartEvent, CricketLiveEvent, CricketRetireEvent,
    InningsEndReason,
};
use crate::mapping::parse_ts_opt;
use crate::match_format::CricketFormat;
use crate::{CricketScore, CricketScoreInnings};

pub fn score_to_record(s: &CricketScore) -> CricketScoreRecord {
    CricketScoreRecord {
        innings: s.innings.iter().map(score_innings_to_record).collect(),
        recent_deliveries: s
            .recent_deliveries
            .as_ref()
            .map(|ds| ds.iter().map(delivery_to_record).collect()),
        next_ball_context: s
            .next_ball_context
            .as_ref()
            .map(next_ball_context_to_record),
        awaiting_next_innings: s.awaiting_next_innings,
    }
}

pub fn score_from_record(rec: &CricketScoreRecord) -> CricketScore {
    CricketScore {
        innings: rec.innings.iter().map(score_innings_from_record).collect(),
        recent_deliveries: rec
            .recent_deliveries
            .as_ref()
            .map(|ds| ds.iter().map(delivery_from_record).collect()),
        next_ball_context: rec
            .next_ball_context
            .as_ref()
            .map(next_ball_context_from_record),
        awaiting_next_innings: rec.awaiting_next_innings,
        // Not stored — `Api::hydrate_score_players` fills this afterward (see
        // `CricketScore::players`' doc comment).
        players: HashMap::new(),
    }
}

pub fn format_to_record(f: &CricketFormat) -> CricketFormatRecord {
    CricketFormatRecord {
        overs_per_innings: f.overs_per_innings,
        innings_per_side: f.innings_per_side,
        balls_per_over: f.balls_per_over,
        no_ball_penalty_runs: f.no_ball_penalty_runs,
        wide_penalty_runs: f.wide_penalty_runs,
        wide_is_extra_ball: f.wide_is_extra_ball,
        no_ball_is_extra_ball: f.no_ball_is_extra_ball,
        free_hit_after_no_ball: f.free_hit_after_no_ball,
    }
}

pub fn format_from_record(f: &CricketFormatRecord) -> CricketFormat {
    CricketFormat {
        overs_per_innings: f.overs_per_innings,
        innings_per_side: f.innings_per_side,
        balls_per_over: f.balls_per_over,
        no_ball_penalty_runs: f.no_ball_penalty_runs,
        wide_penalty_runs: f.wide_penalty_runs,
        wide_is_extra_ball: f.wide_is_extra_ball,
        no_ball_is_extra_ball: f.no_ball_is_extra_ball,
        free_hit_after_no_ball: f.free_hit_after_no_ball,
    }
}

/// A cricket match's configured over length and extra-ball rules — standard
/// rules (a 6-ball over, wides/no-balls re-bowled as extras) unless the match
/// configured something else (e.g. The Hundred's 5-ball over). The only
/// pieces of match format the DAO-agnostic scoring math actually needs, so
/// callers thread these three through as plain arguments rather than the
/// whole format.
pub fn format_args(format: Option<&MatchFormatRecord>) -> (u32, bool, bool) {
    match format {
        Some(MatchFormatRecord::Cricket(f)) => (
            f.balls_per_over,
            f.wide_is_extra_ball,
            f.no_ball_is_extra_ball,
        ),
        _ => (6, true, true),
    }
}

/// Folds an ordered event log into a full `CricketScore` — see
/// `crate::sports::football::from_events`'s doc comment for the uniform
/// signature every sport's `from_events` shares. Cricket is the one sport
/// that actually needs `format` here, for `format_args`' over-length/
/// extra-ball rules.
pub fn from_events(
    events: &[(chrono::DateTime<chrono::Utc>, CricketLiveEvent)],
    format: Option<&MatchFormatRecord>,
) -> CricketScore {
    let (balls_per_over, wide_is_extra_ball, no_ball_is_extra_ball) = format_args(format);
    CricketScore::from_events(
        events,
        balls_per_over,
        wide_is_extra_ball,
        no_ball_is_extra_ball,
    )
}

fn overs_to_record(overs: &Overs) -> OversRecord {
    OversRecord {
        overs: overs.overs,
        balls: overs.balls,
    }
}

fn overs_from_record(rec: &OversRecord) -> Overs {
    Overs {
        overs: rec.overs,
        balls: rec.balls,
    }
}

fn dismissal_to_record(d: &CricketDismissal) -> CricketDismissalRecord {
    CricketDismissalRecord {
        kind: dismissal_kind_to_record(&d.kind),
        bowler_player_id: d.bowler_player_id.clone(),
        fielder_player_id: d.fielder_player_id.clone(),
    }
}

fn dismissal_from_record(rec: &CricketDismissalRecord) -> CricketDismissal {
    CricketDismissal {
        kind: dismissal_kind_from_record(&rec.kind),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn batting_entry_to_record(b: &CricketBattingEntry) -> CricketBattingEntryRecord {
    CricketBattingEntryRecord {
        player_id: b.player_id.clone(),
        runs: b.runs,
        balls_faced: b.balls_faced,
        fours: b.fours,
        sixes: b.sixes,
        dismissal: b.dismissal.as_ref().map(dismissal_to_record),
        batting_position: b.batting_position,
    }
}

fn batting_entry_from_record(rec: &CricketBattingEntryRecord) -> CricketBattingEntry {
    CricketBattingEntry {
        player_id: rec.player_id.clone(),
        runs: rec.runs,
        balls_faced: rec.balls_faced,
        fours: rec.fours,
        sixes: rec.sixes,
        dismissal: rec.dismissal.as_ref().map(dismissal_from_record),
        batting_position: rec.batting_position,
    }
}

fn bowling_entry_to_record(b: &CricketBowlingEntry) -> CricketBowlingEntryRecord {
    CricketBowlingEntryRecord {
        player_id: b.player_id.clone(),
        overs: overs_to_record(&b.overs),
        maidens: b.maidens,
        runs_conceded: b.runs_conceded,
        wickets: b.wickets,
        wides: b.wides,
        no_balls: b.no_balls,
    }
}

fn bowling_entry_from_record(rec: &CricketBowlingEntryRecord) -> CricketBowlingEntry {
    CricketBowlingEntry {
        player_id: rec.player_id.clone(),
        overs: overs_from_record(&rec.overs),
        maidens: rec.maidens,
        runs_conceded: rec.runs_conceded,
        wickets: rec.wickets,
        wides: rec.wides,
        no_balls: rec.no_balls,
    }
}

fn extras_to_record(e: &CricketExtras) -> CricketExtrasRecord {
    CricketExtrasRecord {
        byes: e.byes,
        leg_byes: e.leg_byes,
        wides: e.wides,
        no_balls: e.no_balls,
        penalty: e.penalty,
    }
}

fn extras_from_record(rec: &CricketExtrasRecord) -> CricketExtras {
    CricketExtras {
        byes: rec.byes,
        leg_byes: rec.leg_byes,
        wides: rec.wides,
        no_balls: rec.no_balls,
        penalty: rec.penalty,
    }
}

fn fall_of_wicket_to_record(f: &CricketFallOfWicket) -> CricketFallOfWicketRecord {
    CricketFallOfWicketRecord {
        wicket: f.wicket,
        runs: f.runs,
        player_id: f.player_id.clone(),
        overs: f.overs.map(|o| overs_to_record(&o)),
    }
}

fn fall_of_wicket_from_record(rec: &CricketFallOfWicketRecord) -> CricketFallOfWicket {
    CricketFallOfWicket {
        wicket: rec.wicket,
        runs: rec.runs,
        player_id: rec.player_id.clone(),
        overs: rec.overs.map(|o| overs_from_record(&o)),
    }
}

fn score_innings_to_record(i: &CricketScoreInnings) -> CricketScoreInningsRecord {
    CricketScoreInningsRecord {
        batting_side_id: i.batting_side_id.clone(),
        bowling_side_id: i.bowling_side_id.clone(),
        runs: i.runs,
        wickets: i.wickets,
        overs: overs_to_record(&i.overs),
        declared: i.declared,
        batting: i
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(batting_entry_to_record).collect()),
        bowling: i
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(bowling_entry_to_record).collect()),
        fall_of_wickets: i
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(fall_of_wicket_to_record).collect()),
        extras: i.extras.as_ref().map(extras_to_record),
    }
}

fn score_innings_from_record(rec: &CricketScoreInningsRecord) -> CricketScoreInnings {
    CricketScoreInnings {
        batting_side_id: rec.batting_side_id.clone(),
        bowling_side_id: rec.bowling_side_id.clone(),
        runs: rec.runs,
        wickets: rec.wickets,
        overs: overs_from_record(&rec.overs),
        declared: rec.declared,
        batting: rec
            .batting
            .as_ref()
            .map(|bs| bs.iter().map(batting_entry_from_record).collect()),
        bowling: rec
            .bowling
            .as_ref()
            .map(|bs| bs.iter().map(bowling_entry_from_record).collect()),
        fall_of_wickets: rec
            .fall_of_wickets
            .as_ref()
            .map(|fs| fs.iter().map(fall_of_wicket_from_record).collect()),
        extras: rec.extras.as_ref().map(extras_from_record),
    }
}

fn delivery_to_record(d: &CricketDelivery) -> CricketDeliveryRecord {
    CricketDeliveryRecord {
        over: d.over,
        ball: d.ball,
        bowler_player_id: d.bowler_player_id.clone(),
        striker_player_id: d.striker_player_id.clone(),
        non_striker_player_id: d.non_striker_player_id.clone(),
        runs_off_bat: d.runs_off_bat,
        extra: d.extra.as_ref().map(delivery_extra_to_record),
        wicket: d.wicket.as_ref().map(delivery_wicket_to_record),
        occurred_at: d
            .occurred_at
            .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
    }
}

fn delivery_from_record(rec: &CricketDeliveryRecord) -> CricketDelivery {
    CricketDelivery {
        over: rec.over,
        ball: rec.ball,
        bowler_player_id: rec.bowler_player_id.clone(),
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        runs_off_bat: rec.runs_off_bat,
        extra: rec.extra.as_ref().map(delivery_extra_from_record),
        wicket: rec.wicket.as_ref().map(delivery_wicket_from_record),
        occurred_at: parse_ts_opt(&rec.occurred_at),
    }
}

fn delivery_extra_to_record(e: &CricketDeliveryExtra) -> CricketDeliveryExtraRecord {
    CricketDeliveryExtraRecord {
        kind: extra_kind_to_record(&e.kind),
        runs: e.runs,
    }
}

fn delivery_extra_from_record(rec: &CricketDeliveryExtraRecord) -> CricketDeliveryExtra {
    CricketDeliveryExtra {
        kind: extra_kind_from_record(&rec.kind),
        runs: rec.runs,
    }
}

fn extra_kind_to_record(kind: &CricketExtraKind) -> CricketExtraKindRecord {
    match kind {
        CricketExtraKind::Wide => CricketExtraKindRecord::Wide,
        CricketExtraKind::NoBall => CricketExtraKindRecord::NoBall,
        CricketExtraKind::Bye => CricketExtraKindRecord::Bye,
        CricketExtraKind::LegBye => CricketExtraKindRecord::LegBye,
        CricketExtraKind::Penalty => CricketExtraKindRecord::Penalty,
    }
}

fn extra_kind_from_record(rec: &CricketExtraKindRecord) -> CricketExtraKind {
    match rec {
        CricketExtraKindRecord::Wide => CricketExtraKind::Wide,
        CricketExtraKindRecord::NoBall => CricketExtraKind::NoBall,
        CricketExtraKindRecord::Bye => CricketExtraKind::Bye,
        CricketExtraKindRecord::LegBye => CricketExtraKind::LegBye,
        CricketExtraKindRecord::Penalty => CricketExtraKind::Penalty,
    }
}

fn delivery_wicket_to_record(w: &CricketDeliveryWicket) -> CricketDeliveryWicketRecord {
    CricketDeliveryWicketRecord {
        kind: dismissal_kind_to_record(&w.kind),
        dismissed_player_id: w.dismissed_player_id.clone(),
        bowler_player_id: w.bowler_player_id.clone(),
        fielder_player_id: w.fielder_player_id.clone(),
    }
}

fn delivery_wicket_from_record(rec: &CricketDeliveryWicketRecord) -> CricketDeliveryWicket {
    CricketDeliveryWicket {
        kind: dismissal_kind_from_record(&rec.kind),
        dismissed_player_id: rec.dismissed_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        fielder_player_id: rec.fielder_player_id.clone(),
    }
}

fn dismissal_kind_to_record(kind: &CricketDismissalKind) -> CricketDismissalKindRecord {
    match kind {
        CricketDismissalKind::Bowled => CricketDismissalKindRecord::Bowled,
        CricketDismissalKind::Caught => CricketDismissalKindRecord::Caught,
        CricketDismissalKind::LegBeforeWicket => CricketDismissalKindRecord::LegBeforeWicket,
        CricketDismissalKind::RunOut => CricketDismissalKindRecord::RunOut,
        CricketDismissalKind::Stumped => CricketDismissalKindRecord::Stumped,
        CricketDismissalKind::HitWicket => CricketDismissalKindRecord::HitWicket,
        CricketDismissalKind::RetiredOut => CricketDismissalKindRecord::RetiredOut,
        CricketDismissalKind::RetiredHurt => CricketDismissalKindRecord::RetiredHurt,
    }
}

fn dismissal_kind_from_record(rec: &CricketDismissalKindRecord) -> CricketDismissalKind {
    match rec {
        CricketDismissalKindRecord::Bowled => CricketDismissalKind::Bowled,
        CricketDismissalKindRecord::Caught => CricketDismissalKind::Caught,
        CricketDismissalKindRecord::LegBeforeWicket => CricketDismissalKind::LegBeforeWicket,
        CricketDismissalKindRecord::RunOut => CricketDismissalKind::RunOut,
        CricketDismissalKindRecord::Stumped => CricketDismissalKind::Stumped,
        CricketDismissalKindRecord::HitWicket => CricketDismissalKind::HitWicket,
        CricketDismissalKindRecord::RetiredOut => CricketDismissalKind::RetiredOut,
        CricketDismissalKindRecord::RetiredHurt => CricketDismissalKind::RetiredHurt,
    }
}

fn next_ball_context_to_record(ctx: &NextBallContext) -> NextBallContextRecord {
    NextBallContextRecord {
        striker_player_id: ctx.striker_player_id.clone(),
        non_striker_player_id: ctx.non_striker_player_id.clone(),
        bowler_player_id: ctx.bowler_player_id.clone(),
        over: ctx.over,
        ball: ctx.ball,
        previous_over_bowler_player_id: ctx.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: ctx.runs_conceded_this_over,
    }
}

fn next_ball_context_from_record(rec: &NextBallContextRecord) -> NextBallContext {
    NextBallContext {
        striker_player_id: rec.striker_player_id.clone(),
        non_striker_player_id: rec.non_striker_player_id.clone(),
        bowler_player_id: rec.bowler_player_id.clone(),
        over: rec.over,
        ball: rec.ball,
        previous_over_bowler_player_id: rec.previous_over_bowler_player_id.clone(),
        runs_conceded_this_over: rec.runs_conceded_this_over,
    }
}

fn innings_end_reason_to_record(reason: &InningsEndReason) -> InningsEndReasonRecord {
    match reason {
        InningsEndReason::AllOut => InningsEndReasonRecord::AllOut,
        InningsEndReason::OversComplete => InningsEndReasonRecord::OversComplete,
        InningsEndReason::Declared => InningsEndReasonRecord::Declared,
        InningsEndReason::TargetReached => InningsEndReasonRecord::TargetReached,
    }
}

fn innings_end_reason_from_record(rec: &InningsEndReasonRecord) -> InningsEndReason {
    match rec {
        InningsEndReasonRecord::AllOut => InningsEndReason::AllOut,
        InningsEndReasonRecord::OversComplete => InningsEndReason::OversComplete,
        InningsEndReasonRecord::Declared => InningsEndReason::Declared,
        InningsEndReasonRecord::TargetReached => InningsEndReason::TargetReached,
    }
}

pub fn live_event_to_record(event: &CricketLiveEvent) -> CricketLiveEventRecord {
    match event {
        CricketLiveEvent::Delivery(d) => CricketLiveEventRecord::Delivery(delivery_to_record(d)),
        CricketLiveEvent::Retire(r) => CricketLiveEventRecord::Retire(CricketRetireEventRecord {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEvent::InningsStart(s) => {
            CricketLiveEventRecord::InningsStart(CricketInningsStartEventRecord {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEvent::InningsEnd(e) => {
            CricketLiveEventRecord::InningsEnd(CricketInningsEndEventRecord {
                reason: innings_end_reason_to_record(&e.reason),
            })
        }
    }
}

pub fn live_event_from_record(rec: &CricketLiveEventRecord) -> CricketLiveEvent {
    match rec {
        CricketLiveEventRecord::Delivery(d) => CricketLiveEvent::Delivery(delivery_from_record(d)),
        CricketLiveEventRecord::Retire(r) => CricketLiveEvent::Retire(CricketRetireEvent {
            batter_player_id: r.batter_player_id.clone(),
            retired_out: r.retired_out,
        }),
        CricketLiveEventRecord::InningsStart(s) => {
            CricketLiveEvent::InningsStart(CricketInningsStartEvent {
                batting_side_id: s.batting_side_id.clone(),
                bowling_side_id: s.bowling_side_id.clone(),
            })
        }
        CricketLiveEventRecord::InningsEnd(e) => {
            CricketLiveEvent::InningsEnd(CricketInningsEndEvent {
                reason: innings_end_reason_from_record(&e.reason),
            })
        }
    }
}
