import { useCallback, useRef, useState } from 'react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'

type UploadPurpose = components['schemas']['UploadPurpose']
type Asset = components['schemas']['Asset']

/** Where the upload is in the create → PUT → confirm lifecycle. */
export type UploadStatus = 'idle' | 'uploading' | 'processing' | 'done' | 'error'

/** How long to keep polling `GET /assets/:id` for the storage event to flip the
 *  asset to `uploaded`, and how often. The event round-trips through S3 →
 *  EventBridge → SQS → worker, so allow a few seconds. */
const POLL_INTERVAL_MS = 800
const POLL_TIMEOUT_MS = 20_000

export interface UseAssetUploadResult {
  status: UploadStatus
  /** 0–1 while the bytes are being PUT (best-effort; falls back to indeterminate). */
  error: string | null
  /** The uploaded asset id, once `status === 'done'`. Pass this to the resource. */
  assetId: string | null
  /**
   * Run the full flow for a picked file and resolve with the uploaded asset id
   * (or throw). Also updates the hook's reactive state for rendering.
   */
  upload: (file: File, purpose: UploadPurpose) => Promise<string>
  reset: () => void
}

/**
 * Direct-to-storage image upload, as the Asset API intends:
 *   1. `POST /assets` → an `Asset` (pending) with an `upload` target.
 *   2. PUT the raw bytes to `upload.upload_url`, replaying `upload.headers`.
 *   3. Poll `GET /assets/:id` until the storage event flips it to `uploaded`.
 *
 * The bytes never pass through our API. The returned `assetId` is then attached
 * to a resource (e.g. `profile_image_asset_id`, `header_photo_asset_ids`).
 */
export function useAssetUpload(): UseAssetUploadResult {
  const [status, setStatus] = useState<UploadStatus>('idle')
  const [error, setError] = useState<string | null>(null)
  const [assetId, setAssetId] = useState<string | null>(null)
  // Guards against a resolved/aborted flow updating state after unmount/reset.
  const activeRef = useRef(0)

  const reset = useCallback(() => {
    activeRef.current += 1
    setStatus('idle')
    setError(null)
    setAssetId(null)
  }, [])

  const upload = useCallback(async (file: File, purpose: UploadPurpose): Promise<string> => {
    const run = activeRef.current + 1
    activeRef.current = run
    const isStale = () => activeRef.current !== run

    setStatus('uploading')
    setError(null)
    setAssetId(null)

    try {
      // 1. Create the pending asset. `content_length` is baked into the presigned
      //    PUT, so the file we send must be exactly this size.
      const { data: asset, error: createErr } = await fetchClient.POST('/assets', {
        body: { purpose, content_type: file.type, content_length: file.size },
      })
      if (createErr || !asset) throw new Error('Could not start the upload')
      if (!asset.upload) throw new Error('No upload target was issued')

      // 2. PUT the bytes straight to storage, replaying the signed headers.
      const headers = new Headers()
      for (const h of asset.upload.headers) headers.set(h.name, h.value)
      const putRes = await fetch(asset.upload.upload_url, {
        method: asset.upload.method,
        headers,
        body: file,
      })
      if (!putRes.ok) throw new Error(`Upload failed (${putRes.status})`)
      if (isStale()) throw new Error('cancelled')

      // 3. Poll until the storage event marks it uploaded.
      setStatus('processing')
      const uploaded = await pollUntilUploaded(asset.id, isStale)
      if (isStale()) throw new Error('cancelled')

      setAssetId(uploaded.id)
      setStatus('done')
      return uploaded.id
    } catch (e) {
      if (isStale()) throw e // a newer upload/reset owns the state now
      const message = e instanceof Error ? e.message : 'Upload failed'
      setError(message)
      setStatus('error')
      throw e
    }
  }, [])

  return { status, error, assetId, upload, reset }
}

/** Poll `GET /assets/:id` until it reports `uploaded`, or throw on timeout/failure. */
async function pollUntilUploaded(
  assetId: string,
  isStale: () => boolean,
): Promise<Asset> {
  const deadline = Date.now() + POLL_TIMEOUT_MS
  // Note: Date.now() is fine in the browser; only the workflow sandbox forbids it.
  for (;;) {
    if (isStale()) throw new Error('cancelled')
    const { data, error } = await fetchClient.GET('/assets/{asset_id}', {
      params: { path: { asset_id: assetId } },
    })
    if (error || !data) throw new Error('Could not check upload status')
    if (data.status === 'uploaded') return data
    if (data.status === 'failed') throw new Error('The upload was rejected')
    if (Date.now() > deadline) {
      throw new Error('Upload is taking longer than expected — try again')
    }
    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))
  }
}
