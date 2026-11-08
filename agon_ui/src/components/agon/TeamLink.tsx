import { Link, useNavigate } from 'react-router-dom'

interface TeamLinkProps {
  /** `MatchSide.team_id` — undefined for an ad-hoc side with no team,
   *  which renders `children` unlinked, same as a side with no team gets
   *  no crest. */
  teamId: string | undefined
  className?: string
  children: React.ReactNode
}

/**
 * Makes a team's name/crest open the team's page, mirroring how clicking a
 * player opens their profile (`Link to={/users/:id}`). Use this wherever the
 * wrapped content isn't itself nested inside another clickable element; for
 * that case use `NestedTeamLink` instead.
 */
export function TeamLink({ teamId, className, children }: TeamLinkProps) {
  if (!teamId) return <span className={className}>{children}</span>
  return (
    <Link to={`/teams/${teamId}`} className={className}>
      {children}
    </Link>
  )
}

/**
 * Same as `TeamLink`, for a team name/crest that sits inside another
 * clickable element (e.g. a feed card's "open match" button) — nesting an
 * `<a>`/`<button>` inside a `<button>` is invalid, so this renders a
 * focusable span instead, and stops its click from also bubbling up to the
 * surrounding one.
 */
export function NestedTeamLink({ teamId, className, children }: TeamLinkProps) {
  const navigate = useNavigate()
  if (!teamId) return <span className={className}>{children}</span>

  const go = (event: { stopPropagation: () => void }) => {
    event.stopPropagation()
    navigate(`/teams/${teamId}`)
  }

  return (
    <span
      role="link"
      tabIndex={0}
      className={className}
      onClick={go}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          go(e)
        }
      }}
    >
      {children}
    </span>
  )
}
