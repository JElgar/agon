//! Server-rendered "unfurl" pages for match/invite/join links.
//!
//! The web app (`agon_ui`) is a client-only SPA: `index.html` ships one
//! static `<title>Agon</title>` with no image, so a link pasted into
//! WhatsApp/iMessage/Slack/etc. always showed the same generic "Agon" card —
//! those apps generate a preview by fetching the URL and reading its `<head>`
//! Open Graph tags directly off the response HTML; they never run the SPA's
//! JS, so per-route tags baked in client-side are invisible to them.
//!
//! These routes serve a tiny real HTML page per link instead, with the
//! match/invite/join/team's own title, description and image baked in
//! server-side, then bounce a real visitor on to the actual SPA route via a
//! `<meta http-equiv="refresh">` (crawlers read the `<head>` tags and stop;
//! they don't fetch again or run the refresh).
//!
//! Reached two ways:
//!
//! - `agon_ui/nginx.conf` recognizes a link-preview crawler's user agent and
//!   proxies it here, in place of the SPA shell, for *any* canonical app URL
//!   — `/matches/:id`, `/teams/:id`, `/invite/:token`, `/join/:token`, or
//!   anything else (via the catch-all fallback below) — pasted anywhere, not
//!   just a link built by this app's own share buttons. This is the path
//!   that matters: it's what makes a copy-pasted address-bar URL unfurl.
//! - Directly, at `/api/share/...` through the existing `agon-api-ingress`
//!   `/api` prefix (see `agon_infra/index.ts`) — handy for checking a given
//!   link's preview by hand (no user-agent spoofing needed) without needing
//!   the nginx layer at all.
//!
//! No auth, mirroring the `by-token` preview endpoints these reuse: the
//! token/id in the URL is the only credential a visitor (human or crawler)
//! has before signing in, same as the acceptance flow itself.

use agon_core::dao::{self, match_ops::effective_max_players, records::InvitationContextRecord};
use poem::{
    IntoResponse, Route, get, handler,
    web::{Data, Html, Path},
};

use crate::{Api, Match, MatchType, Member, assets::Assets, mapping, sign_match_headers};

/// The web app's public base URL (`AGON_UI_URL`, trimmed of a trailing
/// slash), used to build the SPA URL a preview page bounces a real visitor
/// on to. Injected once at startup (see `main`'s `RunServer` branch) —
/// defaults to the local Vite dev server when unset, so `make run` renders
/// working (if locally-addressed) previews with no extra setup.
#[derive(Clone)]
pub struct UiBaseUrl(pub String);

pub fn routes() -> Route {
    Route::new()
        .at("/matches/:match_id", get(share_match))
        .at("/teams/:team_id", get(share_team))
        .at("/invite/:token", get(share_invite))
        .at("/join/:token", get(share_join))
        // Anything else (the home page, /feed, /profile, /users/:id, ...) —
        // a static, named/param route always outranks a catch-all in poem's
        // router regardless of registration order, so this only ever
        // catches requests none of the routes above matched. Renders the
        // generic Agon card rather than a 404: nginx's crawler-vs-asset `if`
        // (see nginx.conf) only screens out *existing files*, not unknown
        // app routes, so this is the backstop that keeps every crawler
        // request landing on a real (if plain) preview instead of an error.
        .at("/*path", get(share_fallback))
}

/// Everything the HTML template needs — assembled per link type below, then
/// rendered identically by `render`.
struct PreviewCard {
    /// The real SPA URL a human visitor is bounced on to.
    target_url: String,
    title: String,
    description: String,
    /// Absolute image URL. `render` falls back to the app icon when `None`.
    image_url: Option<String>,
}

#[handler]
async fn share_match(
    Data(dao): Data<&dao::Dao>,
    Data(assets): Data<&Assets>,
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
    Path(match_id): Path<String>,
) -> impl IntoResponse {
    let target_url = format!("{ui_base_url}/matches/{match_id}");
    let card = match match_card(dao, assets, &match_id, &target_url).await {
        Some(card) => card,
        None => not_found_card(target_url),
    };
    Html(render(&card, ui_base_url))
}

#[handler]
async fn share_team(
    Data(dao): Data<&dao::Dao>,
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
    Path(team_id): Path<String>,
) -> impl IntoResponse {
    let target_url = format!("{ui_base_url}/teams/{team_id}");
    let card = match team_card(dao, &team_id, &target_url).await {
        Some(card) => card,
        None => not_found_card(target_url),
    };
    Html(render(&card, ui_base_url))
}

#[handler]
async fn share_invite(
    Data(dao): Data<&dao::Dao>,
    Data(assets): Data<&Assets>,
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let target_url = format!("{ui_base_url}/invite/{token}");
    let card = match invite_card(dao, assets, &token, &target_url).await {
        Some(card) => card,
        None => not_found_card(target_url),
    };
    Html(render(&card, ui_base_url))
}

#[handler]
async fn share_join(
    Data(dao): Data<&dao::Dao>,
    Data(assets): Data<&Assets>,
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
    Path(token): Path<String>,
) -> impl IntoResponse {
    let target_url = format!("{ui_base_url}/join/{token}");
    let card = match join_card(dao, assets, &token, &target_url).await {
        Some(card) => card,
        None => not_found_card(target_url),
    };
    Html(render(&card, ui_base_url))
}

/// Catch-all for any app route with no dedicated preview above (the home
/// page, `/feed`, `/profile`, `/users/:id`, ...) — the neutral `generic_card`,
/// pointed at that same route. NOT `not_found_card`: nothing here is invalid
/// or expired, there's just no richer preview built for this route (yet) —
/// see `routes`'s doc comment for why this exists at all rather than 404ing.
#[handler]
async fn share_fallback(
    Data(UiBaseUrl(ui_base_url)): Data<&UiBaseUrl>,
    Path(path): Path<String>,
) -> impl IntoResponse {
    let target_url = format!("{ui_base_url}/{path}");
    Html(render(&generic_card(target_url), ui_base_url))
}

/// A plain match link's preview: the two (or more) sides as the title, the
/// sport/kickoff-time/description as the blurb, the header photo as the
/// image.
async fn match_card(
    dao: &dao::Dao,
    assets: &Assets,
    match_id: &str,
    target_url: &str,
) -> Option<PreviewCard> {
    let agg = dao.get_match(match_id).await.ok()??;
    let raw = mapping::match_from_records(&agg.match_, &agg.sides, &agg.players, false);
    // No signed-in viewer for a public preview: side-name resolution falls
    // straight through to the neutral "Team A"/"Team B" fallback rather than
    // ever claiming "Your side"/"Opposition" (see `Api::resolve_side_names`).
    let mut m = Api.hydrate_match(dao, raw, "").await.ok()?;
    sign_match_headers(assets, &mut m);
    Some(PreviewCard {
        target_url: target_url.to_string(),
        title: match_title(&m),
        description: match_blurb(&m),
        image_url: m.header_photos.first().map(|p| p.image_url.clone()),
    })
}

/// A single-use invite link's preview — the match/team it's to, plus (for a
/// match) which side the invitee lands on and how many spots remain.
async fn invite_card(
    dao: &dao::Dao,
    assets: &Assets,
    token: &str,
    target_url: &str,
) -> Option<PreviewCard> {
    let rec = dao.get_invitation_by_token(token).await.ok()??;
    match &rec.context {
        InvitationContextRecord::Match { match_id, .. } => {
            let agg = dao.get_match(match_id).await.ok()??;
            let raw = mapping::match_from_records(&agg.match_, &agg.sides, &agg.players, false);
            let mut m = Api.hydrate_match(dao, raw, "").await.ok()?;
            sign_match_headers(assets, &mut m);

            let side_name = player_for_invitation(&m, &rec.id).and_then(|p| {
                p.side_id
                    .as_deref()
                    .and_then(|sid| m.sides.iter().find(|s| s.id == sid))
                    .and_then(|s| s.name.as_deref())
            });
            let spots = spots_left(&agg.sides, agg.match_.total_player_count);

            let mut lines = vec![match side_name {
                Some(name) => format!("You're invited to play for {name}."),
                None => "You're invited to play.".to_string(),
            }];
            if let Some(spots_line) = spots_left_line(spots) {
                lines.push(spots_line);
            }
            lines.push(match_blurb(&m));

            Some(PreviewCard {
                target_url: target_url.to_string(),
                title: match_title(&m),
                description: lines.join(" "),
                image_url: m.header_photos.first().map(|p| p.image_url.clone()),
            })
        }
        InvitationContextRecord::Team { team_id, team_name } => {
            let logo = dao
                .get_team(team_id)
                .await
                .ok()
                .flatten()
                .and_then(|t| t.team.logo_url);
            Some(PreviewCard {
                target_url: target_url.to_string(),
                title: format!("Join {team_name}"),
                description: format!("You're invited to join {team_name} on Agon."),
                image_url: logo,
            })
        }
    }
}

/// A plain team page's preview: the team's name and logo, plus a member/
/// follower-count blurb — the only stats a team carries that are worth
/// surfacing with no further context.
async fn team_card(dao: &dao::Dao, team_id: &str, target_url: &str) -> Option<PreviewCard> {
    let agg = dao.get_team(team_id).await.ok()??;
    let member_count = agg.members.len();
    let mut lines = vec![format!(
        "{member_count} member{} on Agon.",
        if member_count == 1 { "" } else { "s" }
    )];
    if agg.team.follower_count > 0 {
        lines.push(format!(
            "{} follower{}.",
            agg.team.follower_count,
            if agg.team.follower_count == 1 {
                ""
            } else {
                "s"
            }
        ));
    }
    Some(PreviewCard {
        target_url: target_url.to_string(),
        title: agg.team.name,
        description: lines.join(" "),
        image_url: agg.team.logo_url,
    })
}

/// A many-use join link's preview — the match it joins, which side(s) (if
/// the scope pins it to one) and how many spots remain.
async fn join_card(
    dao: &dao::Dao,
    assets: &Assets,
    token: &str,
    target_url: &str,
) -> Option<PreviewCard> {
    let link = dao.get_join_link_by_token(token).await.ok()??;
    if link.revoked_at.is_some() {
        return None;
    }
    let InvitationContextRecord::Match { match_id, .. } = &link.context else {
        return None;
    };
    let agg = dao.get_match(match_id).await.ok()??;
    let raw = mapping::match_from_records(&agg.match_, &agg.sides, &agg.players, false);
    let mut m = Api.hydrate_match(dao, raw, "").await.ok()?;
    sign_match_headers(assets, &mut m);

    // A scope naming exactly one side auto-assigns it (mirroring
    // `JoinMatchPage`'s own `forcedSideId`) — worth calling out by name;
    // anything else (any side, or a pick among several) isn't specific
    // enough to mention.
    let forced_side_name = match link.scope.side_ids.as_deref() {
        Some([only]) => m
            .sides
            .iter()
            .find(|s| &s.id == only)
            .and_then(|s| s.name.as_deref()),
        _ => None,
    };
    let spots = spots_left(&agg.sides, agg.match_.total_player_count);

    let mut lines = vec![match forced_side_name {
        Some(name) => format!("Join {name} for this game."),
        None => "Join this game.".to_string(),
    }];
    if let Some(spots_line) = spots_left_line(spots) {
        lines.push(spots_line);
    }
    lines.push(match_blurb(&m));

    Some(PreviewCard {
        target_url: target_url.to_string(),
        title: match_title(&m),
        description: lines.join(" "),
        image_url: m.header_photos.first().map(|p| p.image_url.clone()),
    })
}

/// The invited player's roster row, found by matching the standalone
/// invitation's id against the embedded invitation carried on each hydrated
/// `MatchPlayer` — the two share an id (see `EmbeddedInvitationRecord`'s doc
/// comment), so this is how a match invite's target side is recovered:
/// `InvitationContextRecord::Match` itself carries no `side_id`.
fn player_for_invitation<'a>(m: &'a Match, invitation_id: &str) -> Option<&'a crate::MatchPlayer> {
    m.players.iter().find(|p| {
        let invitation = match &p.member {
            Member::User(u) => u.invitation.as_ref(),
            Member::External(e) => e.invitation.as_ref(),
        };
        invitation.is_some_and(|i| i.id == invitation_id)
    })
}

/// The match's overall roster cap minus how many have joined so far — `None`
/// when the match is uncapped (see `MatchAggregate::effective_max_players`),
/// in which case there's nothing worth saying about capacity at all.
fn spots_left(sides: &[dao::records::MatchSideRecord], total_player_count: u64) -> Option<u32> {
    effective_max_players(sides).map(|max| max.saturating_sub(total_player_count as u32))
}

fn spots_left_line(spots: Option<u32>) -> Option<String> {
    spots.map(|n| match n {
        0 => "This game is full.".to_string(),
        1 => "1 spot left.".to_string(),
        n => format!("{n} spots left."),
    })
}

/// "{side} vs {side}" for two or more named sides (the common case — every
/// sport here is played between opposing sides), falling back to the match's
/// own name when there's nothing to pair up (e.g. a not-yet-populated side).
fn match_title(m: &Match) -> String {
    let names: Vec<&str> = m.sides.iter().filter_map(|s| s.name.as_deref()).collect();
    if names.len() >= 2 {
        names.join(" vs ")
    } else {
        m.name.clone()
    }
}

/// "Cricket · Sun 14 Sep, 15:00 — {description}" — sport and kickoff time are
/// always shown (self-explanatory context for a preview reached with no app
/// state at all), the creator's own description tacked on when they wrote one.
fn match_blurb(m: &Match) -> String {
    let when = m.starts_at.format("%a %-d %b, %H:%M UTC");
    let heading = format!("{} · {when}", sport_label(&m.match_type));
    let description = m.description.trim();
    if description.is_empty() {
        heading
    } else {
        format!("{heading} — {description}")
    }
}

fn sport_label(t: &MatchType) -> &'static str {
    match t {
        MatchType::Tennis => "Tennis",
        MatchType::Badminton => "Badminton",
        MatchType::Squash => "Squash",
        MatchType::TableTennis => "Table Tennis",
        MatchType::Football => "Football",
        MatchType::Cricket => "Cricket",
        MatchType::Netball => "Netball",
        MatchType::Other => "Match",
    }
}

/// The card shown for a bad/expired/unknown token or id on an otherwise
/// recognized link shape (`/matches/:id`, `/invite/:token`, `/join/:token`,
/// `/teams/:id`) — nothing is lost versus today's behaviour (the SPA itself
/// renders the proper "invite not found"/"link not found" screen once a
/// visitor actually opens `target_url`), just no richer preview to offer.
/// Distinct from `generic_card`: this one specifically means "this link
/// doesn't work", which would be actively misleading on, say, the homepage.
fn not_found_card(target_url: String) -> PreviewCard {
    PreviewCard {
        target_url,
        title: "Agon".to_string(),
        description: "This link is invalid or has expired.".to_string(),
        image_url: None,
    }
}

/// The card shown for any route with no dedicated preview of its own (the
/// home page, `/feed`, `/profile`, `/users/:id`, ...) — just the app's name
/// and a neutral strapline, never a claim that anything is wrong. See
/// `routes`'s doc comment for why this exists at all rather than 404ing.
fn generic_card(target_url: String) -> PreviewCard {
    PreviewCard {
        target_url,
        title: "Agon".to_string(),
        description: "Organize matches, invite players, track the score.".to_string(),
        image_url: None,
    }
}

/// Minimal HTML-escaping, safe for both a text node and a double-quoted
/// attribute value (every interpolation site below is one or the other) —
/// covers match/side/team names and descriptions, all user-controlled text.
fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn render(card: &PreviewCard, ui_base_url: &str) -> String {
    let title = escape_html(&card.title);
    let description = escape_html(&card.description);
    let target = escape_html(&card.target_url);
    let default_image = format!("{ui_base_url}/pwa-512x512.png");
    let image = escape_html(card.image_url.as_deref().unwrap_or(&default_image));

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:type" content="website">
<meta property="og:site_name" content="Agon">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:image" content="{image}">
<meta property="og:url" content="{target}">
<meta name="twitter:card" content="summary_large_image">
<meta name="twitter:title" content="{title}">
<meta name="twitter:description" content="{description}">
<meta name="twitter:image" content="{image}">
<meta http-equiv="refresh" content="0; url={target}">
</head>
<body>
<p>Redirecting to Agon&hellip; <a href="{target}">Continue</a></p>
</body>
</html>"#
    )
}
