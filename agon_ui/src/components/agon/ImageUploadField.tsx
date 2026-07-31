import { useEffect, useRef, useState } from 'react'
import { ImagePlus, Loader2, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import { useAssetUpload } from '@/hooks/useAssetUpload'
import type { components } from '@/types/api'

type UploadPurpose = components['schemas']['UploadPurpose']

/** Client-side guard mirroring the server (`content_type` must be `image/*`). */
const ACCEPT = 'image/png,image/jpeg,image/webp,image/gif'
const MAX_BYTES = 10 * 1024 * 1024 // 10 MB

export interface ImageUploadFieldProps {
  purpose: UploadPurpose
  /** Called with the uploaded asset id once the upload completes, or null when
   *  cleared. Attach this to the resource (profile / match). */
  onUploaded: (assetId: string | null) => void
  /** An existing image URL to show before a new one is picked (e.g. the current
   *  profile picture on an edit form). */
  initialUrl?: string
  /** Round preview (profile pictures) vs. wide preview (match headers). */
  shape?: 'circle' | 'wide'
  label?: string
  className?: string
}

/**
 * A file picker that uploads the chosen image directly to storage (via the Asset
 * API) and reports the resulting asset id. Shows a local preview immediately, an
 * uploading/processing spinner while the bytes land and the storage event fires,
 * and an error with retry on failure. Purely presentational state lives here; the
 * upload lifecycle is owned by `useAssetUpload`.
 */
export function ImageUploadField({
  purpose,
  onUploaded,
  initialUrl,
  shape = 'circle',
  label = 'Upload image',
  className,
}: ImageUploadFieldProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const { status, error, upload, reset } = useAssetUpload()
  // Local object-URL preview of the picked file; falls back to the initial URL.
  const [previewUrl, setPreviewUrl] = useState<string | null>(null)
  const [localError, setLocalError] = useState<string | null>(null)

  // Revoke the object URL when it changes / on unmount to avoid leaking blobs.
  useEffect(() => {
    return () => {
      if (previewUrl) URL.revokeObjectURL(previewUrl)
    }
  }, [previewUrl])

  const busy = status === 'uploading' || status === 'processing'
  const shownUrl = previewUrl ?? initialUrl ?? null

  const handlePick = async (file: File) => {
    setLocalError(null)
    if (!file.type.startsWith('image/')) {
      setLocalError('Please choose an image file')
      return
    }
    if (file.size > MAX_BYTES) {
      setLocalError('Image must be under 10 MB')
      return
    }
    // Swap in a local preview immediately; upload in the background.
    const objectUrl = URL.createObjectURL(file)
    setPreviewUrl((prev) => {
      if (prev) URL.revokeObjectURL(prev)
      return objectUrl
    })
    try {
      const assetId = await upload(file, purpose)
      onUploaded(assetId)
    } catch {
      // The hook surfaces the message via `error`; nothing else to do here.
    }
  }

  const handleClear = () => {
    reset()
    setLocalError(null)
    setPreviewUrl((prev) => {
      if (prev) URL.revokeObjectURL(prev)
      return null
    })
    onUploaded(null)
    if (inputRef.current) inputRef.current.value = ''
  }

  const shapeCls =
    shape === 'circle'
      ? 'size-20 rounded-full'
      : 'h-32 w-full rounded-xl'

  return (
    <div className={cn('flex flex-col gap-2', className)}>
      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => inputRef.current?.click()}
          disabled={busy}
          className={cn(
            'relative flex shrink-0 items-center justify-center overflow-hidden border border-dashed bg-muted text-muted-foreground transition-colors hover:border-primary hover:text-primary disabled:cursor-not-allowed disabled:opacity-70',
            shapeCls,
          )}
          aria-label={label}
        >
          {shownUrl ? (
            <img src={shownUrl} alt="" className="size-full object-cover" />
          ) : (
            <ImagePlus className="size-6" />
          )}
          {busy && (
            <span className="absolute inset-0 flex items-center justify-center bg-background/60">
              <Loader2 className="size-5 animate-spin" />
            </span>
          )}
        </button>

        <div className="flex flex-col gap-1">
          <button
            type="button"
            onClick={() => inputRef.current?.click()}
            disabled={busy}
            className="text-sm font-medium text-primary hover:underline disabled:opacity-70"
          >
            {shownUrl ? 'Change' : label}
          </button>
          {shownUrl && !busy && (
            <button
              type="button"
              onClick={handleClear}
              className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-destructive"
            >
              <X className="size-3" /> Remove
            </button>
          )}
          {status === 'processing' && (
            <span className="text-xs text-muted-foreground">Processing…</span>
          )}
        </div>
      </div>

      {(localError || error) && (
        <p className="text-xs text-destructive">{localError ?? error}</p>
      )}

      <input
        ref={inputRef}
        type="file"
        accept={ACCEPT}
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0]
          if (file) void handlePick(file)
        }}
      />
    </div>
  )
}
