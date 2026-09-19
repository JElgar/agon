import { Link } from 'react-router-dom'

export interface TeamLinkProps {
  /** A match side's `team_id`. Renders plain, inert `children` when absent —
   *  an ad-hoc side (no linked team) has no page to send anyone to. */
  teamId?: string
  className?: string
  children: React.ReactNode
}

/**
 * Wraps a side's logo/name (in a match tile or match detail header) so
 * clicking either navigates to that team's page, without also triggering
 * whatever click handler the surrounding card/row uses to open the match
 * itself — callers nest this inside an `onOpen`-style click target, so the
 * navigation here must stop that bubble.
 */
export function TeamLink({ teamId, className, children }: TeamLinkProps) {
  if (!teamId) {
    return <div className={className}>{children}</div>
  }
  return (
    <Link to={`/teams/${teamId}`} onClick={(e) => e.stopPropagation()} className={className}>
      {children}
    </Link>
  )
}
