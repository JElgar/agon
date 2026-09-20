//! Netball's `SportRecord` implementation.

use crate::dao::records::ScoreRecord;
use crate::sport::{SportContribution, SportRecord};

/// Marker type for netball's `SportRecord` impl — no fields, just a home for
/// the trait's associated const/methods (same pattern `agon_service::sports::
/// netball::NetballSport` uses on the API side).
pub struct NetballRecord;

impl SportRecord for NetballRecord {
    const NAME: &'static str = "netball";

    /// Netball has no dedicated stats record yet — a per-player goal/foul log
    /// exists on `ScoreRecord::Netball` (see `NetballGoalEventRecord`) that a
    /// future `NetballStatsRecord` (mirroring cricket/football) could derive
    /// best-figures from, same as cricket/football — just not built out yet.
    /// Matches today's behavior (netball falls through the worker's `_ =>
    /// empty` default) exactly.
    fn contribution(
        _score: &ScoreRecord,
        _player_id: &str,
        _balls_per_over: u32,
    ) -> SportContribution {
        SportContribution::default()
    }
}
