//! Shared per-event/per-entry types used by `Score::Football`/`Score::Cricket`/
//! `Score::Netball`'s optional rich-detail fields (goals/cards/substitutions;
//! batting/bowling cards, extras, fall-of-wickets; netball goals/fouls) —
//! reused verbatim by both the live-scoring event vocabulary (`live_score`)
//! and the confirmable `Score` (`main.rs`). There's no separate "detailed
//! score" type anymore: `Score` itself carries everything, live or finished,
//! confirmed or not (see `Score`'s doc comment on `main.rs`).

pub mod cricket;
pub mod football;
pub mod netball;
