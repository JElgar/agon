//! The typed team-stats reads (`opensearch::team_stats`) against a real
//! OpenSearch: records per kind, recent results and streak, next match,
//! leaderboards per scope, filtered match lists and head-to-head. Uses a random
//! index prefix and deletes its indices afterwards.
//!
//! Skipped unless `OPENSEARCH_URL` is set (`make test-opensearch-local`).

use agon_services_core::dao::match_ops::MatchAggregate;
use agon_services_core::dao::records::{
    ConfirmedScoreRecord, MatchPlayerRecord, MatchRecord, MatchSideRecord,
};
use agon_services_core::opensearch::team_stats::{self as ts, LeaderboardScope};
use agon_services_core::opensearch::{
    BulkOp, MATCH_STATS, MATCHES, OpenSearchClient, all_indices, build_match_docs,
};
use serde_json::json;

const NOW: &str = "2026-11-15T12:00:00Z";

fn side(id: &str, team: &str) -> MatchSideRecord {
    serde_json::from_value(json!({ "side_id": id, "team_id": team })).unwrap()
}

fn player(pid: &str, user: &str, side: &str) -> MatchPlayerRecord {
    MatchPlayerRecord {
        player_id: pid.into(),
        user_id: Some(user.into()),
        display_name: None,
        side_id: Some(side.into()),
        is_member_of_team: None,
        invitation: None,
        joined_via: None,
    }
}

fn record(id: &str, status: &str, starts_at: &str, sides: &[MatchSideRecord]) -> MatchRecord {
    let mut m: MatchRecord = serde_json::from_value(json!({
        "id": id, "name": id, "description": "", "match_type": "football", "status": status,
        "starts_at": starts_at, "sides": {}, "created_at": "2025-01-01T00:00:00Z"
    }))
    .unwrap();
    m.sides = sides
        .iter()
        .map(|s| (s.side_id.clone(), s.clone()))
        .collect();
    m
}

fn confirmed(
    id: &str,
    starts_at: &str,
    sides: Vec<MatchSideRecord>,
    players: Vec<MatchPlayerRecord>,
    pts: &[(&str, u32)],
    winner: Option<&str>,
) -> MatchAggregate {
    let mut m = record(id, "completed", starts_at, &sides);
    m.confirmed_score = Some(ConfirmedScoreRecord {
        // A goal tally only: enough for the side's football scoreline.
        score: serde_json::from_value(json!({
            "type": "football",
            "score": pts.iter().map(|(s, p)| (s.to_string(), *p)).collect::<std::collections::HashMap<_, _>>(),
        }))
        .unwrap(),
        winner_side_id: winner.map(Into::into),
    });
    MatchAggregate {
        match_: m,
        sides,
        players,
    }
}

#[tokio::test]
async fn team_stats_reads_against_opensearch() {
    let Ok(url) = std::env::var("OPENSEARCH_URL") else {
        eprintln!("OPENSEARCH_URL unset; skipping");
        return;
    };
    let prefix = format!("itest_{}_", rand::random::<u32>());
    let os = OpenSearchClient::new(&url, None, prefix.clone()).unwrap();
    for def in all_indices() {
        os.ensure_index(&def).await.unwrap();
    }

    // HW's history, oldest first:
    //   2025-06  friendly  HW 2-0 Leyton        (won, clean sheet)
    //   2026-10  friendly  HW 1-1 Leyton        (drawn)
    //   2026-11  friendly  HW 0-3 Bow           (lost)
    //   2026-11  squad     HW v HW 4-2          (no W/D/L for the team)
    //   2026-12  scheduled friendly v Leyton    (next match)
    let a = || side("a", "hw");
    let leyton = || side("b", "leyton");
    let bow = || side("b", "bow");
    let matches = [
        confirmed(
            "m1",
            "2025-06-01T10:00:00Z",
            vec![a(), leyton()],
            vec![player("p1", "u1", "a"), player("p2", "u2", "a")],
            &[("a", 2), ("b", 0)],
            Some("a"),
        ),
        confirmed(
            "m2",
            "2026-10-01T10:00:00Z",
            vec![a(), leyton()],
            vec![player("p1", "u1", "a"), player("p3", "u3", "a")],
            &[("a", 1), ("b", 1)],
            None,
        ),
        confirmed(
            "m3",
            "2026-11-08T10:00:00Z",
            vec![a(), bow()],
            vec![player("p1", "u1", "a"), player("p9", "u9", "b")],
            &[("a", 0), ("b", 3)],
            Some("b"),
        ),
        confirmed(
            "m4",
            "2026-11-10T10:00:00Z",
            vec![side("l", "hw"), side("d", "hw")],
            vec![player("p1", "u1", "l"), player("p2", "u2", "d")],
            &[("l", 4), ("d", 2)],
            Some("l"),
        ),
    ];
    let upcoming = {
        let sides = vec![a(), leyton()];
        MatchAggregate {
            match_: record("m5", "scheduled", "2026-12-01T10:00:00Z", &sides),
            sides,
            players: vec![player("p1", "u1", "a")],
        }
    };

    let mut ops = Vec::new();
    for agg in matches.iter().chain(std::iter::once(&upcoming)) {
        let mut docs = build_match_docs(agg, None, NOW);
        ops.push(
            BulkOp::index(
                &os.write_index(&MATCHES),
                &docs.match_doc.match_id,
                &docs.match_doc,
            )
            .unwrap(),
        );
        for row in &mut docs.players {
            // A Simple score carries no counters; give u1 a goal per game so
            // the stat sums are exercised.
            if row.user_id == "u1" {
                row.stats
                    .football
                    .get_or_insert_with(Default::default)
                    .goals = 1;
            }
            ops.push(BulkOp::index(&os.write_index(&MATCH_STATS), &row.doc_id(), row).unwrap());
        }
    }
    os.bulk_with(&ops, true).await.unwrap();

    // Records, all time.
    let r = ts::team_records(&os, "hw", "football", None, NOW)
        .await
        .unwrap();
    assert_eq!(r.years, vec![2026, 2025]);
    assert_eq!(
        (
            r.friendly.played,
            r.friendly.won,
            r.friendly.drawn,
            r.friendly.lost
        ),
        (3, 1, 1, 1)
    );
    assert_eq!(
        r.friendly
            .result
            .football
            .as_ref()
            .map(|f| (f.goals_for, f.goals_against)),
        Some((3, 4))
    );
    assert_eq!(
        r.friendly.stats.football.as_ref().map(|f| f.clean_sheets),
        Some(1)
    );
    assert_eq!(r.ext, r.friendly, "no league or cup games");
    assert_eq!(r.squad.games, 1);
    assert_eq!(
        r.squad.result.football.as_ref().map(|f| f.goals_for),
        Some(6)
    );
    // m1 is more than 12 months before NOW; m2 and m3 are inside.
    assert_eq!(r.ext_last_12_months, 2);

    // Records for 2025 only: just m1. `years` is unaffected.
    let r25 = ts::team_records(&os, "hw", "football", Some(2025), NOW)
        .await
        .unwrap();
    assert_eq!(r25.ext.played, 1);
    assert_eq!(r25.squad.games, 0);
    assert_eq!(r25.years, vec![2026, 2025]);

    // Recent results: newest first, squad game left out.
    let recent = ts::recent_results(&os, "hw", "football", 10).await.unwrap();
    let ids: Vec<&str> = recent.iter().map(|r| r.match_id.as_str()).collect();
    assert_eq!(ids, vec!["m3", "m2", "m1"]);
    assert_eq!(
        recent[0].opponent.as_ref().unwrap().team_id.as_deref(),
        Some("bow")
    );
    assert_eq!(ts::streak(&recent), Some(("lost".into(), 1)));

    // Next match.
    let next = ts::next_match(&os, "hw", "football", NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.match_id, "m5");
    assert_eq!(next.opponent.unwrap().team_id.as_deref(), Some("leyton"));

    // Leaderboard, all: u1 played all four (squad included) and scored 4.
    let all = ts::leaderboard(&os, "hw", "football", &LeaderboardScope::All, None)
        .await
        .unwrap();
    assert_eq!(all[0].user_id, "u1");
    assert_eq!(
        (all[0].played, all[0].won, all[0].drawn, all[0].lost),
        (4, 2, 1, 1)
    );
    assert_eq!(all[0].stats.football.as_ref().map(|f| f.goals), Some(4));
    assert!(
        all.iter().all(|r| r.user_id != "u9"),
        "Bow's player isn't on HW's board"
    );

    // Leaderboard, squad only, and 2025 only.
    let squad = ts::leaderboard(
        &os,
        "hw",
        "football",
        &LeaderboardScope::Kind("squad".into()),
        None,
    )
    .await
    .unwrap();
    let mut squad_users: Vec<&str> = squad.iter().map(|r| r.user_id.as_str()).collect();
    squad_users.sort();
    assert_eq!(squad_users, vec!["u1", "u2"]);
    let y25 = ts::leaderboard(&os, "hw", "football", &LeaderboardScope::All, Some(2025))
        .await
        .unwrap();
    assert_eq!(y25.len(), 2);
    assert!(y25.iter().all(|r| r.played == 1));

    // Another sport has nothing.
    let none = ts::team_records(&os, "hw", "netball", None, NOW)
        .await
        .unwrap();
    assert_eq!(none.ext.played, 0);
    assert!(none.years.is_empty());

    // Head to head: HW v Leyton is m1 (won 2-0) and m2 (drawn 1-1); the
    // scheduled m5 doesn't count yet.
    let h2h = ts::head_to_head(&os, "hw", "leyton", "football")
        .await
        .unwrap();
    let r = &h2h.record;
    assert_eq!((r.played, r.won, r.drawn, r.lost), (2, 1, 1, 0));
    assert_eq!(
        r.result
            .football
            .as_ref()
            .map(|f| (f.goals_for, f.goals_against)),
        Some((3, 1))
    );
    // From Leyton's side it's the mirror image.
    let back = ts::head_to_head(&os, "leyton", "hw", "football")
        .await
        .unwrap();
    let r = &back.record;
    assert_eq!((r.played, r.won, r.drawn, r.lost), (2, 0, 1, 1));
    // Never met.
    let never = ts::head_to_head(&os, "leyton", "bow", "football")
        .await
        .unwrap();
    assert_eq!(never.record.played, 0);

    for def in all_indices() {
        os.delete_index(&def.physical(&prefix)).await.unwrap();
    }
}
