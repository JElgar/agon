/**
 * Every non-2xx API response now carries a JSON `{ message: string }` body
 * (see `agon_service`'s `ErrorMessage`) — this pulls that message out of
 * whatever `openapi-fetch`'s typed `error` field turns out to be, falling
 * back for anything that isn't shaped that way (a network failure, a
 * response `openapi-fetch` couldn't parse as JSON at all).
 */
export function apiErrorMessage(error: unknown, fallback: string): string {
  if (
    error &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message?: unknown }).message === 'string' &&
    (error as { message: string }).message.length > 0
  ) {
    return (error as { message: string }).message
  }
  return fallback
}
