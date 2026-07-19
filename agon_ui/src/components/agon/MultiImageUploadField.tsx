import { useRef, useState } from 'react'
import { ImagePlus, Loader2, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type UploadPurpose = components['schemas']['UploadPurpose']

const ACCEPT = 'image/png,image/jpeg,image/webp,image/gif'
const MAX_BYTES = 10 * 1024 * 1024 // 10 MB
const POLL_INTERVAL_MS = 800
const POLL_TIMEOUT_MS = 20_000

/** One image tile's lifecycle in the grid. */
interface Item {
  /** Stable local key for React (object URL doubles as it). */
  key: string
  previewUrl: string
  status: 'uploading' | 'processing' | 'done' | 'error'
  assetId: string | null
  error: string | null
}

export interface MultiImageUploadFieldProps {
  purpose: UploadPurpose
  /** Reports the ordered list of successfully-uploaded asset ids whenever it
   *  changes. Attach this to the resource (e.g. `header_photo_asset_ids`). */
  onChange: (assetIds: string[]) => void
  /** Cap on how many images can be added. */
  max?: number
  label?: string
  className?: string
}

/**
 * A multi-image picker that uploads each chosen image directly to storage (via
 * the Asset API) and reports the ordered list of uploaded asset ids. Renders a
 * grid of preview tiles, each with its own uploading/processing spinner, error,
 * and a remove button, plus an "add" tile until `max` is reached. Order in the
 * grid is the order sent to the server (and shown in the carousel).
 */
export function MultiImageUploadField({
  purpose,
  onChange,
  max = 6,
  label = 'Add images',
  className,
}: MultiImageUploadFieldProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const [items, setItems] = useState<Item[]>([])
  const [error, setError] = useState<string | null>(null)

  /** Recompute + report the ordered done asset ids from the latest items. */
  const report = (next: Item[]) => {
    onChange(next.filter((i) => i.status === 'done' && i.assetId).map((i) => i.assetId!))
  }

  const patch = (key: string, changes: Partial<Item>) => {
    setItems((prev) => {
      const next = prev.map((i) => (i.key === key ? { ...i, ...changes } : i))
      report(next)
      return next
    })
  }

  const uploadOne = async (file: File, key: string) => {
    try {
      const { data: asset, error: createErr } = await fetchClient.POST('/assets', {
        body: { purpose, content_type: file.type, content_length: file.size },
      })
      if (createErr || !asset?.upload) throw new Error('Could not start the upload')

      const headers = new Headers()
      for (const h of asset.upload.headers) headers.set(h.name, h.value)
      const putRes = await fetch(asset.upload.upload_url, {
        method: asset.upload.method,
        headers,
        body: file,
      })
      if (!putRes.ok) throw new Error(`Upload failed (${putRes.status})`)

      patch(key, { status: 'processing' })
      await pollUntilUploaded(asset.id)
      patch(key, { status: 'done', assetId: asset.id })
    } catch (e) {
      patch(key, {
        status: 'error',
        error: e instanceof Error ? e.message : 'Upload failed',
      })
    }
  }

  const addFiles = (files: FileList) => {
    setError(null)
    const room = max - items.length
    const chosen = Array.from(files).slice(0, Math.max(0, room))
    if (files.length > chosen.length) {
      setError(`You can add up to ${max} images`)
    }
    for (const file of chosen) {
      if (!file.type.startsWith('image/')) {
        setError('Please choose image files only')
        continue
      }
      if (file.size > MAX_BYTES) {
        setError('Each image must be under 10 MB')
        continue
      }
      const previewUrl = URL.createObjectURL(file)
      const item: Item = {
        key: previewUrl,
        previewUrl,
        status: 'uploading',
        assetId: null,
        error: null,
      }
      setItems((prev) => [...prev, item])
      void uploadOne(file, item.key)
    }
    if (inputRef.current) inputRef.current.value = ''
  }

  const remove = (key: string) => {
    setItems((prev) => {
      const target = prev.find((i) => i.key === key)
      if (target) URL.revokeObjectURL(target.previewUrl)
      const next = prev.filter((i) => i.key !== key)
      report(next)
      return next
    })
  }

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <div className="grid grid-cols-3 gap-2">
        {items.map((item) => (
          <div
            key={item.key}
            className="relative aspect-video overflow-hidden rounded-lg border bg-muted"
          >
            <img src={item.previewUrl} alt="" className="size-full object-cover" />
            {(item.status === 'uploading' || item.status === 'processing') && (
              <span className="absolute inset-0 flex items-center justify-center bg-background/60">
                <Loader2 className="size-5 animate-spin" />
              </span>
            )}
            {item.status === 'error' && (
              <span className="absolute inset-0 flex items-center justify-center bg-destructive/70 p-1 text-center text-[10px] text-white">
                {item.error}
              </span>
            )}
            <button
              type="button"
              onClick={() => remove(item.key)}
              aria-label="Remove image"
              className="absolute right-1 top-1 rounded-full bg-background/80 p-0.5 text-foreground hover:bg-background"
            >
              <X className="size-3.5" />
            </button>
          </div>
        ))}

        {items.length < max && (
          <button
            type="button"
            onClick={() => inputRef.current?.click()}
            className="flex aspect-video items-center justify-center rounded-lg border border-dashed bg-muted text-muted-foreground transition-colors hover:border-primary hover:text-primary"
            aria-label={label}
          >
            <ImagePlus className="size-5" />
          </button>
        )}
      </div>

      {error && <p className="text-xs text-destructive">{error}</p>}

      <input
        ref={inputRef}
        type="file"
        accept={ACCEPT}
        multiple
        className="hidden"
        onChange={(e) => {
          if (e.target.files?.length) addFiles(e.target.files)
        }}
      />
    </div>
  )
}

/** Poll `GET /assets/:id` until it reports `uploaded`, or throw on timeout/failure. */
async function pollUntilUploaded(assetId: string): Promise<void> {
  const deadline = Date.now() + POLL_TIMEOUT_MS
  for (;;) {
    const { data, error } = await fetchClient.GET('/assets/{asset_id}', {
      params: { path: { asset_id: assetId } },
    })
    if (error || !data) throw new Error('Could not check upload status')
    if (data.status === 'uploaded') return
    if (data.status === 'failed') throw new Error('The upload was rejected')
    if (Date.now() > deadline) throw new Error('Upload timed out — try again')
    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))
  }
}
