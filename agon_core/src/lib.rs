//! Logic shared across every target (backend, web via wasm, mobile later):
//! sport scoring — folding live events into a score, and deriving a winner
//! from one — and the auto team builder.
//! Plain data in, data out — no DAO/API types, so a backend crate maps its
//! own wire types onto these to use them (see `agon_service::sports`).

pub mod football;
pub mod team_builder;
