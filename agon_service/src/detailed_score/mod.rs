use poem_openapi::Union;

pub mod cricket;
pub mod football;

pub use cricket::CricketDetail;
pub use football::FootballDetail;

/// Optional, sport-specific detailed score (a full breakdown beyond the summary
/// `Score`). One variant per sport; clients switch on `type`. Only shown on a
/// match detail view, not the feed. New sports are added as new variants
/// without breaking existing clients.
#[derive(Union)]
#[oai(discriminator_name = "type")]
pub enum DetailedScore {
    Football(FootballDetail),
    Cricket(CricketDetail),
}
