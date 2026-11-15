//! Sport scoring logic shared across every target that scores a match:
//! folding live events into a score, and deriving a winner from one.
//! Plain data in, data out — no DAO/API types, so a backend crate maps its
//! own wire types onto these to use them (see `agon_service::sports`).

pub mod football;
