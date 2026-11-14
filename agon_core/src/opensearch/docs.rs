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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::{DateTime, Datelike, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::dao::match_ops::MatchAggregate;
use crate::dao::records::{ScoreRecord, TournamentFormatRecord, TournamentRecord};

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

/// The calendar year a match belongs to: its start time in the tournament's
/// timezone when there is one, else UTC (a 00:30 kick-off on 1 January in
/// London stays in the new year).
pub fn match_year(starts_at: &str, timezone: Option<&str>) -> Option<i32> {
    let utc: DateTime<Utc> = DateTime::parse_from_rfc3339(starts_at)
        .ok()?
        .with_timezone(&Utc);
    match timezone.and_then(|tz| tz.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => Some(utc.with_timezone(&tz).year()),
        None => Some(utc.year()),
    }
}

/// Points per side from a confirmed score: goals (football, netball), runs
/// (cricket, summed over a side's innings), points (simple scores) or sets won
/// (set-based scores). Sides missing from the score are absent here.
pub fn side_points(score: &ScoreRecord) -> HashMap<String, u64> {
    let mut out: HashMap<String, u64> = HashMap::new();
    match score {
        ScoreRecord::Simple { entries } => {
            for (side, pts) in entries {
                out.insert(side.clone(), u64::from(*pts));
            }
        }
        ScoreRecord::Sets { entries } => {
            let sets = entries.values().map(Vec::len).max().unwrap_or(0);
            for side in entries.keys() {
                out.insert(side.clone(), 0);
            }
            for i in 0..sets {
                let games: Vec<(&String, u32)> = entries
                    .iter()
                    .map(|(side, v)| (side, v.get(i).copied().unwrap_or(0)))
                    .collect();
                let best = games.iter().map(|(_, g)| *g).max().unwrap_or(0);
                let leaders: Vec<&&String> = games
                    .iter()
                    .filter(|(_, g)| *g == best)
                    .map(|(s, _)| s)
                    .collect();
                // A set only counts for its outright winner.
                if leaders.len() == 1 {
                    *out.entry((*leaders[0]).clone()).or_default() += 1;
                }
            }
        }
        ScoreRecord::Football(s) => {
            for (side, goals) in &s.score {
                out.insert(side.clone(), u64::from(*goals));
            }
        }
        ScoreRecord::Netball(s) => {
            for (side, goals) in &s.score {
                out.insert(side.clone(), u64::from(*goals));
            }
        }
        ScoreRecord::Cricket(s) => {
            for innings in &s.innings {
                *out.entry(innings.batting_side_id.clone()).or_default() += u64::from(innings.runs);
            }
        }
    }
    out
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conceded: Option<u64>,
    /// Football only: conceded nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clean_sheet: Option<bool>,
    /// Account holders who took a spot on this side.
    pub player_ids: Vec<String>,
    pub player_count: u32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_id: Option<String>,
    pub starts_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    pub team_ids: Vec<String>,
    pub participant_ids: Vec<String>,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winner_side_id: Option<String>,
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
    pub starts_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i32>,
    pub played: u32,
    pub won: u32,
    pub drawn: u32,
    pub lost: u32,
    /// Sport counters, e.g. `{"goals": 2, "assists": 1}`.
    pub stats: BTreeMap<String, u64>,
    pub indexed_at: String,
}

impl PlayerStatDoc {
    pub fn doc_id(&self) -> String {
        player_doc_id(&self.match_id, &self.user_id)
    }
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
/// tournament when it has one (for league vs cup, and the timezone the year is
/// judged in). `now` is stamped as `indexed_at`.
pub fn build_match_docs(
    agg: &MatchAggregate,
    tournament: Option<&TournamentRecord>,
    now: &str,
) -> MatchDocs {
    let m = &agg.match_;
    let kind = match_kind(agg, tournament);
    let year = match_year(&m.starts_at, tournament.map(|t| t.timezone.as_str()));
    let confirmed = m.confirmed_score.as_ref();
    let winner = confirmed.and_then(|c| c.winner_side_id.clone());
    let points = confirmed.map(|c| side_points(&c.score)).unwrap_or_default();
    let is_football = m.match_type == "football";

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

    let sides: Vec<SideDoc> = agg
        .sides
        .iter()
        .map(|s| {
            let score = points.get(&s.side_id).copied();
            let conceded = confirmed.map(|_| {
                points
                    .iter()
                    .filter(|(id, _)| **id != s.side_id)
                    .map(|(_, p)| *p)
                    .sum::<u64>()
            });
            let players: Vec<String> = roster
                .get(s.side_id.as_str())
                .into_iter()
                .flatten()
                .map(|u| u.to_string())
                .collect();
            SideDoc {
                side_id: s.side_id.clone(),
                team_id: s.team_id.clone(),
                name: s.name.clone(),
                outcome: outcome_for(&s.side_id).map(Outcome::as_str),
                score: confirmed.map(|_| score.unwrap_or(0)),
                conceded,
                clean_sheet: if is_football {
                    conceded.map(|c| c == 0)
                } else {
                    None
                },
                player_count: players.len() as u32,
                player_ids: players,
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

    let match_doc = MatchDoc {
        match_id: m.id.clone(),
        name: m.name.clone(),
        sport: m.match_type.clone(),
        status: m.status.clone(),
        kind,
        ranked: m.ranked,
        tournament_id: m.tournament_id.clone(),
        series_id: m.series_id.clone(),
        starts_at: m.starts_at.clone(),
        year,
        team_ids,
        participant_ids,
        confirmed: confirmed.is_some(),
        winner_side_id: winner.clone(),
        sides,
        indexed_at: now.to_string(),
    };

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
                    let contribution = crate::sports::contribution(
                        &m.match_type,
                        &cs.score,
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
                        starts_at: m.starts_at.clone(),
                        year,
                        played: 1,
                        won: u32::from(outcome == Outcome::Won),
                        drawn: u32::from(outcome == Outcome::Drawn),
                        lost: u32::from(outcome == Outcome::Lost),
                        stats: contribution.counters.into_iter().collect(),
                        indexed_at: now.to_string(),
                    }
                })
                .collect()
        }
    };

    MatchDocs { match_doc, players }
}

/// Serialize helper for callers that want the documents as JSON values.
pub fn to_value<T: Serialize>(doc: &T) -> Value {
    serde_json::to_value(doc).expect("documents always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::records::{
        ConfirmedScoreRecord, EmbeddedInvitationRecord, InvitationKindRecord, MatchPlayerRecord,
        MatchRecord, MatchSideRecord,
    };

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
        if let Some((pts, winner)) = score {
            match_.confirmed_score = Some(ConfirmedScoreRecord {
                score: ScoreRecord::Simple {
                    entries: pts.iter().map(|(s, p)| (s.to_string(), *p)).collect(),
                },
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
    fn year_uses_the_tournament_timezone() {
        // 23:30 UTC on 31 Dec is already 1 Jan in Auckland.
        assert_eq!(
            match_year("2025-12-31T23:30:00Z", Some("Pacific/Auckland")),
            Some(2026)
        );
        assert_eq!(match_year("2025-12-31T23:30:00Z", None), Some(2025));
        assert_eq!(match_year("not a date", None), None);
    }

    #[test]
    fn sets_count_only_outright_set_wins() {
        let score = ScoreRecord::Sets {
            entries: [
                ("a".to_string(), vec![6, 4, 7]),
                ("b".to_string(), vec![3, 6, 5]),
            ]
            .into_iter()
            .collect(),
        };
        let pts = side_points(&score);
        assert_eq!(pts["a"], 2);
        assert_eq!(pts["b"], 1);
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
                .all(|s| s.outcome.is_none() && s.score.is_none())
        );
        assert_eq!(docs.match_doc.participant_ids, vec!["u1".to_string()]);
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
        assert_eq!(
            (a_side.outcome, a_side.score, a_side.conceded),
            (Some("won"), Some(3), Some(1))
        );
        assert_eq!(a_side.clean_sheet, Some(false));
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
