use poem_openapi::{Enum, Object};

/// Football detail is an ordered timeline of match events. The summary score
/// (goals per side) can be derived by counting Goal events.
#[derive(Object)]
pub struct FootballDetail {
    pub events: Vec<FootballEvent>,
}

#[derive(Enum)]
#[oai(rename_all = "snake_case")]
pub enum FootballEventKind {
    Goal,
    OwnGoal,
    Penalty,
    YellowCard,
    RedCard,
    Substitution,
}

#[derive(Object)]
pub struct FootballEvent {
    pub kind: FootballEventKind,
    /// The side this event belongs to (references MatchSide.id).
    pub side_id: String,
    /// Match minute, if recorded.
    pub minute: Option<u32>,
    /// Player who performed the action: scorer, card recipient, player coming
    /// on. References MatchPlayer's member id.
    pub player_id: Option<String>,
    /// Assisting player for a goal. None for own goals or unassisted goals.
    pub assist_player_id: Option<String>,
    /// Player coming off, for a substitution (paired with `player_id` coming on).
    pub substituted_player_id: Option<String>,
}
