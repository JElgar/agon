import { ChevronLeft, Swords, Trophy } from 'lucide-react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'

/**
 * First step behind the "+" button: what do you want to create. Picking a
 * tile just navigates on to that flow's own first screen (`/matches/new`'s
 * sport-picker step, or `/tournaments/new`'s form) — this page owns no
 * create-flow state of its own.
 */
export function CreatePage() {
  const navigate = useNavigate()

  return (
    <div className="mx-auto flex w-full max-w-xl flex-col gap-5 md:max-w-2xl">
      <div className="flex items-center gap-1">
        <Button variant="ghost" size="icon" className="-ml-2" onClick={() => navigate(-1)} aria-label="Back">
          <ChevronLeft className="size-5" />
        </Button>
        <h1 className="font-display text-xl font-extrabold md:text-2xl">What are you creating?</h1>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <CreateTile
          icon={Swords}
          label="Match"
          description="Log or schedule a single match"
          onClick={() => navigate('/matches/new')}
        />
        <CreateTile
          icon={Trophy}
          label="Tournament"
          description="Knockout or league, with teams or players"
          onClick={() => navigate('/tournaments/new')}
        />
      </div>
    </div>
  )
}

function CreateTile({
  icon: Icon,
  label,
  description,
  onClick,
}: {
  icon: typeof Swords
  label: string
  description: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex flex-col items-start gap-2.5 rounded-[20px] border bg-card p-4 text-left transition-colors hover:bg-accent/50"
    >
      <span className="flex size-10 items-center justify-center rounded-full bg-accent text-accent-foreground">
        <Icon className="size-5" />
      </span>
      <span className="font-display text-base font-bold">{label}</span>
      <span className="text-xs text-muted-foreground">{description}</span>
    </button>
  )
}
