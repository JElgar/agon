import * as React from "react"
import { format } from "date-fns"
import { CalendarIcon } from "lucide-react"

import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Calendar } from "@/components/ui/calendar"
import { Input } from "@/components/ui/input"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"

/** Split a "YYYY-MM-DDTHH:mm" datetime-local value into its date and time
 *  parts (mirrors the format `<input type="datetime-local">` uses). */
function splitValue(value: string): { date: Date | undefined; time: string } {
  const [datePart, timePart] = value.split("T")
  if (!datePart) return { date: undefined, time: "" }
  const [y, m, d] = datePart.split("-").map(Number)
  const date = new Date(y, (m || 1) - 1, d || 1)
  return { date: Number.isNaN(date.getTime()) ? undefined : date, time: timePart ?? "" }
}

function joinValue(date: Date, time: string): string {
  const pad = (n: number) => String(n).padStart(2, "0")
  const datePart = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
  return `${datePart}T${time || "00:00"}`
}

/**
 * A date + time picker that reads and writes the same "YYYY-MM-DDTHH:mm"
 * wall-clock string as `<input type="datetime-local">`, so it's a drop-in
 * replacement — a `Popover`-backed `Calendar` for the date (shadcn's
 * documented date-picker pattern) alongside a native time input, since
 * there's no equivalent widget for the time half.
 */
export function DateTimePicker({
  id,
  value,
  onChange,
  className,
}: {
  id?: string
  value: string
  onChange: (value: string) => void
  className?: string
}) {
  const [open, setOpen] = React.useState(false)
  const { date, time } = splitValue(value)

  return (
    <div className={cn("flex gap-2", className)}>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            id={id}
            type="button"
            variant="outline"
            className="flex-1 justify-start font-normal"
          >
            <CalendarIcon className="size-4 opacity-60" />
            {date ? (
              format(date, "PPP")
            ) : (
              <span className="text-muted-foreground">Pick a date</span>
            )}
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={date}
            onSelect={(d) => {
              if (!d) return
              onChange(joinValue(d, time || "00:00"))
              setOpen(false)
            }}
          />
        </PopoverContent>
      </Popover>
      <Input
        type="time"
        value={time}
        onChange={(e) => onChange(joinValue(date ?? new Date(), e.target.value))}
        className="w-[110px]"
        aria-label="Time"
      />
    </div>
  )
}
