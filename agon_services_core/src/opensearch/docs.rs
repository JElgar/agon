//! Pure builders from a match (as read from DynamoDB) to the documents
//! OpenSearch stores for it. No I/O here, so every rule is unit-tested.
//!
//! One match produces:
//! - one [`MatchDoc`] (always, whatever its status), and
//! - one [`PlayerStatDoc`] per account holder who played, but only once the
//!   score is confirmed. That's the same rule as lifetime stats
//!   (`agon_worker/src/handlers/stats.rs`): a pending or disputed score doesn't
//!   count yet.
//!
//! Document ids are deterministic (`<match_id>` and `<match_id>:<user_id>`), so
//! re-running the builder and re-indexing is idempotent.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::dao::match_ops::MatchAggregate;
use crate::dao::open_join::open_has_space;
use crate::dao::records::{TournamentFormatRecord, TournamentRecord};
use crate::sports::{MatchStatsRecord, SideResultRecord};

/// What kind of match this is. Decides where it counts on the team pages: the
/// team record covers league, cup and friendly; squad games are counted on
/// their own; pickups have no team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    League,
    Cup,
    Friendly,
    Squad,
    Pickup,
}

/// Derive a match's kind from its tournament and sides.
///
/// - In a tournament: `league` or `cup` (knockout), by the tournament's format.
/// - No tournament, two or more sides, every side the same team: `squad`.
/// - No tournament, at least one side with a team: `friendly` (a team against
///   another team, or against an ad-hoc side).
/// - Otherwise: `pickup`.
///
/// Once match setup stores the kind explicitly this stays as the fallback for
/// older matches.
pub fn match_kind(agg: &MatchAggregate, tournament: Option<&TournamentRecord>) -> MatchKind {
    if agg.match_.tournament_id.is_some() {
        return match tournament.map(|t| t.format) {
            Some(TournamentFormatRecord::Knockout) => MatchKind::Cup,
            // A league, or a tournament we couldn't load: league is the safer
            // guess (it still lands in the team record either way).
            _ => MatchKind::League,
        };
    }
    let team_ids: Vec<&str> = agg
        .sides
        .iter()
        .filter_map(|s| s.team_id.as_deref())
        .collect();
    if agg.sides.len() >= 2 && team_ids.len() == agg.sides.len() {
        let first = team_ids[0];
        if team_ids.iter().all(|t| *t == first) {
            return MatchKind::Squad;
        }
    }
    if team_ids.is_empty() {
        MatchKind::Pickup
    } else {
        MatchKind::Friendly
    }
}

/// The calendar year a match belongs to, in UTC — the API is timezone
/// agnostic and always stores and judges `starts_at` in UTC, a tournament's
/// own timezone included (that's only ever used to pick the local wall-clock
/// time a round starts at, never to reinterpret a stored instant).
pub fn match_year(starts_at: &str) -> Option<i32> {
    let utc: DateTime<Utc> = DateTime::parse_from_rfc3339(starts_at)
        .ok()?
        .with_timezone(&Utc);
    Some(utc.year())
}

/// One side of a match, nested in [`MatchDoc`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SideDoc {
    pub side_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `won` / `drawn` / `lost`, once the score is confirmed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<&'static str>,
    /// This side's scoreline in the match's sport (`result.football` goals
    /// for and against, `result.cricket` runs and wickets, ...). Omitted
    /// until the score is confirmed, and for a generic sport.
    #[serde(skip_serializing_if = "no_result")]
    pub result: SideResultRecord,
    /// Account holders who took a spot on this side.
    pub player_ids: Vec<String>,
    pub player_count: u32,
    /// This side's counters in the match's sport (`stats.football`,
    /// `stats.cricket`, ...): its account holders' [`PlayerStatDoc::stats`]
    /// summed, with side-level counters such as football's clean sheet set
    /// by the sport (`SportRecord::side_match_stats`). Omitted until the
    /// score is confirmed, and for a generic sport.
    #[serde(skip_serializing_if = "no_stats")]
    pub stats: MatchStatsRecord,
}

/// One match (index `matches`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchDoc {
    pub match_id: String,
    pub name: String,
    pub sport: String,
    pub status: String,
    pub kind: MatchKind,
    pub ranked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    /// 0-based round (knockout) or matchday (league) within the tournament.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament_round: Option<u32>,
    /// Bracket position within the round. Knockout only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament_slot: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
    pub starts_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    pub team_ids: Vec<String>,
    pub participant_ids: Vec<String>,
    /// Every id on the match's roster, whatever the invitation status: each
    /// player's stable player id and, when they have an account, their user
    /// id. Unlike `participant_ids` (account holders who took a spot), this
    /// still includes pending and declined invitees and people without an
    /// account, so the `participant` filter of `GET /matches` finds a match
    /// by either id.
    pub roster_ids: Vec<String>,
    /// The `roster_ids` of players on the winning side. All three outcome
    /// lists are empty until the score is confirmed (a missing result, not
    /// a draw), and a player with no side is in none of them.
    pub winning_participant_ids: Vec<String>,
    pub losing_participant_ids: Vec<String>,
    /// Filled only when the confirmed score has no winning side.
    pub drawing_participant_ids: Vec<String>,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner_side_id: Option<String>,
    /// Whether this match has an open-join policy set at all (`Match.
    /// open_join.is_some()`) — the same facet `GET /matches`' Meilisearch
    /// doc carries, mirrored here for a future OpenSearch-backed version of
    /// that discovery filter.
    pub open: bool,
    /// `open` and currently has room for another individual arrival — see
    /// `crate::dao::open_join::open_has_space`'s doc comment on what this
    /// approximates. Meaningless (always `false`) when `open` is `false`.
    pub open_has_space: bool,
    pub sides: Vec<SideDoc>,
    pub indexed_at: String,
}

/// One player in one confirmed match (index `match_stats`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlayerStatDoc {
    pub match_id: String,
    pub user_id: String,
    pub side_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    pub opponent_team_ids: Vec<String>,
    pub in_house: bool,
    pub kind: MatchKind,
    pub sport: String,
    pub ranked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament_id: Option<String>,
    /// 0-based round (knockout) or matchday (league) within the tournament.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament_round: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
    pub starts_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
    /// This player's counters in the match's sport (`stats.football`,
    /// `stats.cricket`, ...). Omitted for a generic sport.
    #[serde(skip_serializing_if = "no_stats")]
    pub stats: MatchStatsRecord,
    pub indexed_at: String,
}

impl PlayerStatDoc {
    pub fn doc_id(&self) -> String {
        player_doc_id(&self.match_id, &self.user_id)
    }
}

fn no_stats(stats: &MatchStatsRecord) -> bool {
    *stats == MatchStatsRecord::default()
}

fn no_result(result: &SideResultRecord) -> bool {
    *result == SideResultRecord::default()
}

pub fn player_doc_id(match_id: &str, user_id: &str) -> String {
    format!("{match_id}:{user_id}")
}

/// Everything OpenSearch stores for one match.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchDocs {
    pub match_doc: MatchDoc,
    pub players: Vec<PlayerStatDoc>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Lost,
    Drawn,
    Won,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Won => "won",
            Outcome::Drawn => "drawn",
            Outcome::Lost => "lost",
        }
    }
}

/// Build the match document and its player rows. `tournament` is the match's
/// tournament when it has one (for league vs cup). `now` is stamped as
/// `indexed_at`.
pub fn build_match_docs(
    agg: &MatchAggregate,
    tournament: Option<&TournamentRecord>,
    now: &str,
) -> MatchDocs {
    let m = &agg.match_;
    let kind = match_kind(agg, tournament);
    let year = match_year(&m.starts_at);
    let confirmed = m.confirmed_score.as_ref();
    let winner = confirmed.and_then(|c| c.winner_side_id.clone());

    let outcome_for = |side_id: &str| -> Option<Outcome> {
        confirmed.map(|_| match &winner {
            None => Outcome::Drawn,
            Some(w) if w == side_id => Outcome::Won,
            Some(_) => Outcome::Lost,
        })
    };

    // Who took a spot on each side (account holders only: a player without an
    // account has no stats identity).
    let mut roster: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for p in &agg.players {
        if !p.occupies_slot() {
            continue;
        }
        if let (Some(side), Some(user)) = (p.side_id.as_deref(), p.user_id.as_deref()) {
            roster.entry(side).or_default().insert(user);
        }
    }

    let players = match confirmed {
        None => Vec::new(),
        Some(cs) => {
            // Each user's roster entries in this match (almost always one), the
            // side of their best outcome, and that outcome. Mirrors the lifetime
            // stats handler so the two can't disagree.
            let mut by_user: BTreeMap<&str, (Outcome, &str, Vec<&str>)> = BTreeMap::new();
            for p in &agg.players {
                let (Some(user), Some(side)) = (p.user_id.as_deref(), p.side_id.as_deref()) else {
                    continue;
                };
                if !p.occupies_slot() {
                    continue;
                }
                let outcome = outcome_for(side).unwrap_or(Outcome::Drawn);
                let entry = by_user.entry(user).or_insert((outcome, side, Vec::new()));
                if outcome > entry.0 {
                    entry.0 = outcome;
                    entry.1 = side;
                }
                entry.2.push(p.player_id.as_str());
            }
            by_user
                .into_iter()
                .map(|(user, (outcome, side_id, player_ids))| {
                    let team_id = agg
                        .sides
                        .iter()
                        .find(|s| s.side_id == side_id)
                        .and_then(|s| s.team_id.clone());
                    let opponent_team_ids: Vec<String> = agg
                        .sides
                        .iter()
                        .filter(|s| s.side_id != side_id)
                        .filter_map(|s| s.team_id.clone())
                        .filter(|t| Some(t) != team_id.as_ref())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let stats = MatchStatsRecord::for_player(
                        &m.match_type,
                        &cs.score,
                        side_id,
                        &player_ids,
                        m.format.as_ref(),
                        &m.id,
                    );
                    PlayerStatDoc {
                        match_id: m.id.clone(),
                        user_id: user.to_string(),
                        side_id: side_id.to_string(),
                        team_id,
                        opponent_team_ids,
                        in_house: kind == MatchKind::Squad,
                        kind,
                        sport: m.match_type.clone(),
                        ranked: m.ranked,
                        tournament_id: m.tournament_id.clone(),
                        tournament_round: m.tournament_round.map(|r| r.round),
                        series_id: m.series_id.clone(),
                        starts_at: m.starts_at.clone(),
                        year,
                        played: 1,
                        won: u32::from(outcome == Outcome::Won),
                        drawn: u32::from(outcome == Outcome::Drawn),
                        lost: u32::from(outcome == Outcome::Lost),
                        stats,
                        indexed_at: now.to_string(),
                    }
                })
                .collect()
        }
    };

    // Each side's counters are the sum of its account holders' own (already
    // computed above), so a side's cricket wickets or football assists never
    // drift from the player rows they're built from.
    let mut side_stats: BTreeMap<&str, MatchStatsRecord> = BTreeMap::new();
    for p in &players {
        side_stats
            .entry(p.side_id.as_str())
            .or_default()
            .add(&p.stats);
    }

    let sides: Vec<SideDoc> = agg
        .sides
        .iter()
        .map(|s| {
            let side_players: Vec<String> = roster
                .get(s.side_id.as_str())
                .into_iter()
                .flatten()
                .map(|u| u.to_string())
                .collect();
            let (result, stats) = match confirmed {
                Some(cs) => (
                    SideResultRecord::for_side(&m.match_type, &cs.score, &s.side_id),
                    MatchStatsRecord::for_side(
                        &m.match_type,
                        &cs.score,
                        &s.side_id,
                        side_stats.remove(s.side_id.as_str()).unwrap_or_default(),
                    ),
                ),
                None => Default::default(),
            };
            SideDoc {
                side_id: s.side_id.clone(),
                team_id: s.team_id.clone(),
                name: s.name.clone(),
                outcome: outcome_for(&s.side_id).map(Outcome::as_str),
                result,
                player_count: side_players.len() as u32,
                player_ids: side_players,
                stats,
            }
        })
        .collect();

    let team_ids: Vec<String> = agg
        .sides
        .iter()
        .filter_map(|s| s.team_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let participant_ids: Vec<String> = roster
        .values()
        .flatten()
        .map(|u| u.to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut roster_ids = BTreeSet::new();
    let mut winning = BTreeSet::new();
    let mut losing = BTreeSet::new();
    let mut drawing = BTreeSet::new();
    for p in &agg.players {
        let ids = std::iter::once(p.player_id.as_str()).chain(p.user_id.as_deref());
        roster_ids.extend(ids.clone().map(str::to_string));
        if let (Some(side), true) = (p.side_id.as_deref(), confirmed.is_some()) {
            let bucket = match winner.as_deref() {
                Some(w) if w == side => &mut winning,
                Some(_) => &mut losing,
                None => &mut drawing,
            };
            bucket.extend(ids.map(str::to_string));
        }
    }

    let match_doc = MatchDoc {
        match_id: m.id.clone(),
        name: m.name.clone(),
        sport: m.match_type.clone(),
        status: m.status.clone(),
        kind,
        ranked: m.ranked,
        tournament_id: m.tournament_id.clone(),
        tournament_round: m.tournament_round.map(|r| r.round),
        tournament_slot: m
            .tournament_round
            .filter(|_| kind == MatchKind::Cup)
            .map(|r| r.slot),
        series_id: m.series_id.clone(),
        starts_at: m.starts_at.clone(),
        year,
        team_ids,
        participant_ids,
        roster_ids: roster_ids.into_iter().collect(),
        winning_participant_ids: winning.into_iter().collect(),
        losing_participant_ids: losing.into_iter().collect(),
        drawing_participant_ids: drawing.into_iter().collect(),
        confirmed: confirmed.is_some(),
        winner_side_id: winner.clone(),
        open: m.open_join.is_some(),
        open_has_space: open_has_space(agg),
        sides,
        indexed_at: now.to_string(),
    };

    MatchDocs { match_doc, players }
}

/// Serialize helper for callers that want the documents as JSON values.
pub fn to_value<T: Serialize>(doc: &T) -> Value {
    serde_json::to_value(doc).expect("documents always serialize")
}

/// One user (index `users`). Backs the admin panel's total-users count, not
/// a search index — `name`/`email` are carried so the document is readable
/// on its own (e.g. in OpenSearch Dashboards), not to make this a second
/// place that searches users.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UserDoc {
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub created_at: String,
    pub is_admin: bool,
    pub indexed_at: String,
}

/// Build a user's document. `now` is stamped as `indexed_at`.
pub fn build_user_doc(user: &crate::dao::records::UserRecord, now: &str) -> UserDoc {
    UserDoc {
        user_id: user.id.clone(),
        name: user.name.clone(),
        email: user.email.clone(),
        created_at: user.created_at.clone(),
        is_admin: user.is_admin,
        indexed_at: now.to_string(),
    }
}

/// One searchable team (index `teams`). Holds only what the search matches
/// and sorts on; the endpoint hydrates the rest from DynamoDB.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TeamDoc {
    pub team_id: String,
    pub name: String,
    pub created_at: String,
    pub indexed_at: String,
}

/// Build a team's document, or `None` for a tournament squad team (which only
/// exists inside its tournament and is never searchable). `now` is stamped as
/// `indexed_at`.
pub fn build_team_doc(team: &crate::dao::records::TeamRecord, now: &str) -> Option<TeamDoc> {
    if team.is_tournament_team() {
        return None;
    }
    Some(TeamDoc {
        team_id: team.id.clone(),
        name: team.name.clone(),
        created_at: team.created_at.clone(),
        indexed_at: now.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::records::{
        ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationKindRecord, MatchPlayerRecord,
        MatchRecord, MatchSideRecord, ScoreRecord, UserRecord,
    };
    use crate::sports::football::FootballSideResultRecord;

    #[test]
    fn team_doc_skips_tournament_squads() {
        let team = |extra: serde_json::Value| -> crate::dao::records::TeamRecord {
            let mut v = serde_json::json!({
                "id": "t1", "name": "Rovers", "created_at": "2026-01-01T00:00:00Z"
            });
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value(v).unwrap()
        };
        let doc = build_team_doc(&team(serde_json::json!({})), "now").unwrap();
        assert_eq!(doc.team_id, "t1");
        assert_eq!(doc.name, "Rovers");
        assert_eq!(doc.indexed_at, "now");
        assert!(
            build_team_doc(&team(serde_json::json!({"tournament_id": "tour"})), "now").is_none()
        );
    }

    #[test]
    fn user_doc_carries_name_email_admin_flag_and_creation_time() {
        let user: UserRecord = serde_json::from_value(serde_json::json!({
            "id": "u1",
            "email": "u1@example.com",
            "name": "User One",
            "created_at": "2026-01-01T00:00:00Z",
            "is_admin": true,
        }))
        .expect("user");
        let doc = build_user_doc(&user, "2026-11-16T00:00:00Z");
        assert_eq!(doc.user_id, "u1");
        assert_eq!(doc.name, "User One");
        assert_eq!(doc.email, "u1@example.com");
        assert_eq!(doc.created_at, "2026-01-01T00:00:00Z");
        assert!(doc.is_admin);
        assert_eq!(doc.indexed_at, "2026-11-16T00:00:00Z");
    }

    fn side(id: &str, team: Option<&str>) -> MatchSideRecord {
        serde_json::from_value(serde_json::json!({ "side_id": id, "team_id": team })).expect("side")
    }

    fn player(pid: &str, user: Option<&str>, side: &str) -> MatchPlayerRecord {
        MatchPlayerRecord {
            player_id: pid.into(),
            user_id: user.map(Into::into),
            display_name: None,
            side_id: Some(side.into()),
            is_member_of_team: None,
            invitation: None,
            joined_via: None,
        }
    }

    fn agg(
        sides: Vec<MatchSideRecord>,
        players: Vec<MatchPlayerRecord>,
        score: Option<(&[(&str, u32)], Option<&str>)>,
    ) -> MatchAggregate {
        let mut match_: MatchRecord = serde_json::from_value(serde_json::json!({
            "id": "m1", "name": "Test", "description": "", "match_type": "football",
            "status": "completed", "starts_at": "2026-11-08T10:30:00Z", "sides": {},
            "created_at": "2026-11-01T00:00:00Z"
        }))
        .expect("match");
        match_.sides = sides
            .iter()
            .map(|s| (s.side_id.clone(), s.clone()))
            .collect();
        if let Some((goals, winner)) = score {
            let tally: serde_json::Map<String, serde_json::Value> = goals
                .iter()
                .map(|(s, g)| (s.to_string(), (*g).into()))
                .collect();
            match_.confirmed_score = Some(ConfirmedScoreRecord {
                score: serde_json::from_value(
                    serde_json::json!({ "type": "football", "score": tally }),
                )
                .expect("football score"),
                winner_side_id: winner.map(Into::into),
            });
        }
        MatchAggregate {
            match_,
            sides,
            players,
        }
    }

    #[test]
    fn kind_is_squad_only_when_every_side_is_the_same_team() {
        let a = agg(
            vec![side("a", Some("t")), side("b", Some("t"))],
            vec![],
            None,
        );
        assert_eq!(match_kind(&a, None), MatchKind::Squad);
        let b = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![],
            None,
        );
        assert_eq!(match_kind(&b, None), MatchKind::Friendly);
        let c = agg(vec![side("a", Some("t")), side("b", None)], vec![], None);
        assert_eq!(match_kind(&c, None), MatchKind::Friendly);
        let d = agg(vec![side("a", None), side("b", None)], vec![], None);
        assert_eq!(match_kind(&d, None), MatchKind::Pickup);
    }

    #[test]
    fn tournament_round_is_on_both_documents_and_slot_is_knockout_only() {
        let mut a = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![player("p1", Some("u1"), "a")],
            Some((&[("a", 1), ("b", 0)], Some("a"))),
        );
        a.match_.tournament_id = Some("tour".into());
        a.match_.tournament_round =
            Some(crate::dao::records::TournamentRoundInfo { round: 4, slot: 2 });
        // No tournament record, so this reads as a league: the slot is meaningless.
        let docs = build_match_docs(&a, None, "now");
        assert_eq!(docs.match_doc.kind, MatchKind::League);
        assert_eq!(docs.match_doc.tournament_round, Some(4));
        assert_eq!(docs.match_doc.tournament_slot, None);
        assert_eq!(docs.players[0].tournament_round, Some(4));
    }

    #[test]
    fn year_is_always_the_utc_calendar_year() {
        // 23:30 UTC on 31 Dec is still 31 Dec in UTC, whatever a tournament's
        // own timezone would make of it.
        assert_eq!(match_year("2025-12-31T23:30:00Z"), Some(2025));
        assert_eq!(match_year("not a date"), None);
    }

    #[test]
    fn a_generic_sport_has_an_outcome_but_no_scoreline_or_counters() {
        let mut a = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![player("p1", Some("u1"), "a")],
            None,
        );
        a.match_.match_type = "tennis".into();
        a.match_.confirmed_score = Some(ConfirmedScoreRecord {
            score: ScoreRecord::Sets {
                entries: [("a".to_string(), vec![6, 6]), ("b".to_string(), vec![3, 4])]
                    .into_iter()
                    .collect(),
            },
            winner_side_id: Some("a".into()),
        });
        let docs = build_match_docs(&a, None, "now");
        let a_side = &docs.match_doc.sides[0];
        assert_eq!(a_side.outcome, Some("won"));
        assert_eq!(a_side.result, SideResultRecord::default());
        assert_eq!(a_side.stats, MatchStatsRecord::default());
        let doc = to_value(&docs.match_doc);
        assert!(doc["sides"][0].get("result").is_none());
        assert!(doc["sides"][0].get("stats").is_none());
    }

    #[test]
    fn unconfirmed_match_has_no_player_rows_and_no_outcomes() {
        let a = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![player("p1", Some("u1"), "a")],
            None,
        );
        let docs = build_match_docs(&a, None, "now");
        assert!(docs.players.is_empty());
        assert!(!docs.match_doc.confirmed);
        assert!(
            docs.match_doc
                .sides
                .iter()
                .all(|s| s.outcome.is_none() && s.result == SideResultRecord::default())
        );
        assert_eq!(docs.match_doc.participant_ids, vec!["u1".to_string()]);
    }

    #[test]
    fn open_and_open_has_space_mirror_the_matchs_open_join_policy() {
        use crate::dao::records::{
            OpenJoinAudienceRecord, OpenJoinPolicyRecord, OpenJoinScopeRecord,
        };

        let a = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![],
            None,
        );
        let docs = build_match_docs(&a, None, "now");
        assert!(!docs.match_doc.open);
        assert!(!docs.match_doc.open_has_space);

        let mut b = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![],
            None,
        );
        b.match_.open_join = Some(OpenJoinScopeRecord::MatchWide(OpenJoinPolicyRecord {
            audience: OpenJoinAudienceRecord::Individuals,
            rules: Vec::new(),
            requires_approval: false,
            max_open_spots: None,
        }));
        b.sides[0].open_join_enabled = true;
        let docs = build_match_docs(&b, None, "now");
        assert!(docs.match_doc.open);
        assert!(docs.match_doc.open_has_space);
    }

    #[test]
    fn roster_and_outcome_ids_cover_every_player_id_and_user_id() {
        let mut declined = player("p3", Some("u3"), "b");
        declined.invitation = Some(EmbeddedInvitationRecord {
            id: "inv".into(),
            status: "declined".into(),
            invited_by_user_id: "host".into(),
            invited_at: "2026-11-01T00:00:00Z".into(),
            responded_at: None,
            kind: InvitationKindRecord::User {
                invited_user_id: "u3".into(),
            },
            reserve_spot: false,
        });
        let players = vec![
            player("p1", Some("u1"), "a"),
            player("p2", None, "b"), // no account: only its player id
            declined,
        ];
        let sides = || vec![side("a", Some("t")), side("b", Some("u"))];

        let unconfirmed = build_match_docs(&agg(sides(), players.clone(), None), None, "now");
        let m = &unconfirmed.match_doc;
        assert_eq!(m.roster_ids, ["p1", "p2", "p3", "u1", "u3"]);
        assert_eq!(m.participant_ids, ["u1"]);
        assert!(m.winning_participant_ids.is_empty());
        assert!(m.losing_participant_ids.is_empty());
        assert!(m.drawing_participant_ids.is_empty());

        let won = build_match_docs(
            &agg(
                sides(),
                players.clone(),
                Some((&[("a", 2), ("b", 1)], Some("a"))),
            ),
            None,
            "now",
        );
        assert_eq!(won.match_doc.winning_participant_ids, ["p1", "u1"]);
        assert_eq!(won.match_doc.losing_participant_ids, ["p2", "p3", "u3"]);
        assert!(won.match_doc.drawing_participant_ids.is_empty());

        let drawn = build_match_docs(
            &agg(sides(), players, Some((&[("a", 1), ("b", 1)], None))),
            None,
            "now",
        );
        assert_eq!(
            drawn.match_doc.drawing_participant_ids,
            ["p1", "p2", "p3", "u1", "u3"]
        );
        assert!(drawn.match_doc.winning_participant_ids.is_empty());
    }

    #[test]
    fn confirmed_match_sets_side_results_and_one_row_per_account_holder() {
        let a = agg(
            vec![side("a", Some("hw")), side("b", Some("lo"))],
            vec![
                player("p1", Some("u1"), "a"),
                player("p2", Some("u2"), "b"),
                player("p3", None, "a"), // no account: on the roster, no stats row
            ],
            Some((&[("a", 3), ("b", 1)], Some("a"))),
        );
        let docs = build_match_docs(&a, None, "now");
        let a_side = docs
            .match_doc
            .sides
            .iter()
            .find(|s| s.side_id == "a")
            .unwrap();
        assert_eq!(a_side.outcome, Some("won"));
        assert_eq!(
            a_side.result.football,
            Some(FootballSideResultRecord {
                goals_for: 3,
                goals_against: 1,
            })
        );
        assert_eq!(
            a_side.stats.football.as_ref().map(|f| f.clean_sheets),
            Some(0)
        );
        assert_eq!(docs.match_doc.kind, MatchKind::Friendly);
        assert_eq!(docs.players.len(), 2);
        let u1 = docs.players.iter().find(|p| p.user_id == "u1").unwrap();
        assert_eq!((u1.won, u1.lost, u1.played), (1, 0, 1));
        assert_eq!(u1.team_id.as_deref(), Some("hw"));
        assert_eq!(u1.opponent_team_ids, vec!["lo".to_string()]);
        assert_eq!(u1.doc_id(), "m1:u1");
        let u2 = docs.players.iter().find(|p| p.user_id == "u2").unwrap();
        assert_eq!((u2.won, u2.lost), (0, 1));
    }

    #[test]
    fn squad_game_rows_have_no_opponent_team_and_are_in_house() {
        let a = agg(
            vec![side("l", Some("t")), side("d", Some("t"))],
            vec![player("p1", Some("u1"), "l"), player("p2", Some("u2"), "d")],
            Some((&[("l", 2), ("d", 2)], None)),
        );
        let docs = build_match_docs(&a, None, "now");
        assert_eq!(docs.match_doc.kind, MatchKind::Squad);
        for p in &docs.players {
            assert!(p.in_house);
            assert!(p.opponent_team_ids.is_empty());
            assert_eq!((p.drawn, p.won, p.lost), (1, 0, 0));
        }
    }

    #[test]
    fn side_stats_sum_their_players_contributions() {
        use crate::sports::football::{
            FootballGoalEventRecord, FootballMatchStatsRecord, FootballScoreRecord,
        };

        let mut a = agg(
            vec![side("a", Some("hw")), side("b", Some("lo"))],
            vec![
                player("p1", Some("u1"), "a"),
                player("p2", Some("u2"), "a"),
                player("p3", Some("u3"), "b"),
            ],
            None,
        );
        a.match_.confirmed_score = Some(ConfirmedScoreRecord {
            score: ScoreRecord::Football(FootballScoreRecord {
                score: [("a".to_string(), 2), ("b".to_string(), 0)]
                    .into_iter()
                    .collect(),
                goals: Some(vec![
                    FootballGoalEventRecord {
                        side_id: "a".into(),
                        scorer_player_id: Some("p1".into()),
                        assist_player_id: Some("p2".into()),
                        own_goal: false,
                        penalty: false,
                        minute: None,
                        occurred_at: None,
                    },
                    FootballGoalEventRecord {
                        side_id: "a".into(),
                        scorer_player_id: Some("p2".into()),
                        assist_player_id: None,
                        own_goal: false,
                        penalty: false,
                        minute: None,
                        occurred_at: None,
                    },
                ]),
                cards: None,
                substitutions: None,
                period: None,
                period_times: None,
                penalty_shootout: None,
                penalty_shootout_score: None,
            }),
            winner_side_id: Some("a".into()),
        });

        let docs = build_match_docs(&a, None, "now");
        let a_side = docs
            .match_doc
            .sides
            .iter()
            .find(|s| s.side_id == "a")
            .unwrap();
        // u1 scored once (1 goal, 0 assists), u2 scored once and assisted
        // once (1 goal, 1 assist): the side's total is both players'
        // summed. Both played in the clean sheet, but the side kept one.
        assert_eq!(
            a_side.stats.football,
            Some(FootballMatchStatsRecord {
                goals: 2,
                assists: 1,
                clean_sheets: 1,
            })
        );
        assert_eq!(a_side.stats.cricket, None);
        let u2 = docs.players.iter().find(|p| p.user_id == "u2").unwrap();
        assert_eq!(
            u2.stats.football,
            Some(FootballMatchStatsRecord {
                goals: 1,
                assists: 1,
                clean_sheets: 1,
            })
        );
        let b_side = docs
            .match_doc
            .sides
            .iter()
            .find(|s| s.side_id == "b")
            .unwrap();
        assert_eq!(
            b_side.stats.football,
            Some(FootballMatchStatsRecord::default())
        );

        let doc = to_value(&docs.match_doc);
        assert_eq!(doc["sides"][0]["stats"]["football"]["goals"], 2);
        assert!(doc["sides"][0]["stats"].get("cricket").is_none());
    }

    #[test]
    fn declined_invitee_takes_no_spot() {
        let mut declined = player("p2", Some("u2"), "a");
        declined.invitation = Some(EmbeddedInvitationRecord {
            id: "inv".into(),
            status: "declined".into(),
            invited_by_user_id: "host".into(),
            invited_at: "2026-11-01T00:00:00Z".into(),
            responded_at: None,
            kind: InvitationKindRecord::User {
                invited_user_id: "u2".into(),
            },
            reserve_spot: false,
        });
        let a = agg(
            vec![side("a", Some("t")), side("b", Some("u"))],
            vec![player("p1", Some("u1"), "a"), declined],
            Some((&[("a", 1), ("b", 0)], Some("a"))),
        );
        let docs = build_match_docs(&a, None, "now");
        assert_eq!(docs.players.len(), 1);
        assert_eq!(docs.match_doc.sides[0].player_ids, vec!["u1".to_string()]);
    }
}
