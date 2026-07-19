import { useState } from 'react'
import {
  useInfiniteQuery,
  useMutation,
  useQueryClient,
} from '@tanstack/react-query'
import { MessageCircle, Pencil, Trash2 } from 'lucide-react'
import { fetchClient } from '@/lib/api-client'
import type { components } from '@/types/api'
import { cn } from '@/lib/utils'
import { relativeTime } from '@/lib/datetime'
import { Avatar } from '@/components/agon/Avatar'
import { Button } from '@/components/ui/button'

type Comment = components['schemas']['Comment']
type CommentPage = components['schemas']['CommentPage']

/** Page size for the comment list. The API caps at 50; 20 matches its default. */
const PAGE_SIZE = 20

/**
 * The match's comment thread: a composer, the top-level comments (newest first),
 * each with its replies, and per-comment edit/delete for the author. Wired to
 * `GET/POST/PATCH/DELETE /matches/{id}/comments` and the replies sub-resource.
 * Reflects the server's tombstone model — a deleted comment that still has
 * replies is kept as "[deleted]" so its thread stays intact.
 */
export function MatchComments({
  matchId,
  currentUserId,
}: {
  matchId: string
  currentUserId?: string
}) {
  const query = useInfiniteQuery({
    queryKey: ['comments', matchId],
    initialPageParam: undefined as string | undefined,
    queryFn: async ({ pageParam }): Promise<CommentPage> => {
      const { data, error } = await fetchClient.GET(
        '/matches/{match_id}/comments',
        {
          params: {
            path: { match_id: matchId },
            query: { cursor: pageParam, limit: PAGE_SIZE },
          },
        },
      )
      if (error || !data) throw new Error('Failed to load comments')
      return data
    },
    getNextPageParam: (last) => last.next_cursor,
  })

  const comments = (query.data?.pages ?? []).flatMap((p) => p.items)

  return (
    <div className="rounded-xl border bg-card p-4">
      <div className="mb-3 flex items-center gap-1.5 text-sm font-medium">
        <MessageCircle className="size-4" /> Comments
      </div>

      <CommentComposer matchId={matchId} />

      {query.isLoading ? (
        <p className="mt-4 text-sm text-muted-foreground">Loading comments…</p>
      ) : query.isError ? (
        <div className="mt-4 text-sm text-muted-foreground">
          <p className="mb-2">Couldn't load comments.</p>
          <Button variant="outline" size="sm" onClick={() => query.refetch()}>
            Retry
          </Button>
        </div>
      ) : comments.length === 0 ? (
        <p className="mt-4 text-sm text-muted-foreground">
          No comments yet. Be the first to say something.
        </p>
      ) : (
        <div className="mt-4 flex flex-col gap-4">
          {comments.map((comment) => (
            <CommentThread
              key={comment.id}
              matchId={matchId}
              comment={comment}
              currentUserId={currentUserId}
            />
          ))}
        </div>
      )}

      {query.hasNextPage && (
        <Button
          variant="ghost"
          size="sm"
          className="mt-3"
          disabled={query.isFetchingNextPage}
          onClick={() => query.fetchNextPage()}
        >
          {query.isFetchingNextPage ? 'Loading…' : 'Load more comments'}
        </Button>
      )}
    </div>
  )
}

/** A top-level comment plus its replies and an inline reply composer. */
function CommentThread({
  matchId,
  comment,
  currentUserId,
}: {
  matchId: string
  comment: Comment
  currentUserId?: string
}) {
  const [replying, setReplying] = useState(false)

  const repliesQuery = useInfiniteQuery({
    queryKey: ['comment-replies', matchId, comment.id],
    initialPageParam: undefined as string | undefined,
    // Only fetch replies when there are any — avoids a request per comment.
    enabled: comment.reply_count > 0,
    queryFn: async ({ pageParam }): Promise<CommentPage> => {
      const { data, error } = await fetchClient.GET(
        '/matches/{match_id}/comments/{comment_id}/replies',
        {
          params: {
            path: { match_id: matchId, comment_id: comment.id },
            query: { cursor: pageParam, limit: PAGE_SIZE },
          },
        },
      )
      if (error || !data) throw new Error('Failed to load replies')
      return data
    },
    getNextPageParam: (last) => last.next_cursor,
  })

  const replies = (repliesQuery.data?.pages ?? []).flatMap((p) => p.items)

  return (
    <div>
      <CommentRow
        matchId={matchId}
        comment={comment}
        currentUserId={currentUserId}
        onReply={() => setReplying((v) => !v)}
      />

      {(replies.length > 0 || replying) && (
        <div className="mt-2 flex flex-col gap-3 border-l pl-3 ml-3.5">
          {replies.map((reply) => (
            <CommentRow
              key={reply.id}
              matchId={matchId}
              comment={reply}
              currentUserId={currentUserId}
            />
          ))}
          {repliesQuery.hasNextPage && (
            <Button
              variant="ghost"
              size="sm"
              className="self-start"
              disabled={repliesQuery.isFetchingNextPage}
              onClick={() => repliesQuery.fetchNextPage()}
            >
              {repliesQuery.isFetchingNextPage ? 'Loading…' : 'More replies'}
            </Button>
          )}
          {replying && (
            <CommentComposer
              matchId={matchId}
              parentId={comment.id}
              placeholder="Write a reply…"
              onDone={() => setReplying(false)}
            />
          )}
        </div>
      )}
    </div>
  )
}

/** A single comment (top-level or reply): author, body/tombstone, actions. */
function CommentRow({
  matchId,
  comment,
  currentUserId,
  onReply,
}: {
  matchId: string
  comment: Comment
  currentUserId?: string
  /** When present (top-level only), renders a Reply toggle. */
  onReply?: () => void
}) {
  const [editing, setEditing] = useState(false)
  const deleted = !!comment.deleted_at
  const isAuthor =
    !!currentUserId && !!comment.author && comment.author.id === currentUserId
  const name = comment.author?.name ?? 'Someone'

  if (editing && !deleted) {
    return (
      <CommentComposer
        matchId={matchId}
        editing={comment}
        onDone={() => setEditing(false)}
      />
    )
  }

  return (
    <div className="flex gap-2.5">
      <Avatar
        name={name}
        imageUrl={comment.author?.profile_image?.image_url}
        size="md"
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-xs">
          <span className="font-medium">{deleted ? 'Deleted' : name}</span>
          <span className="text-muted-foreground">
            {relativeTime(comment.created_at)}
          </span>
          {comment.edited_at && !deleted && (
            <span className="text-muted-foreground">· edited</span>
          )}
        </div>
        <p
          className={cn(
            'mt-0.5 whitespace-pre-wrap break-words text-sm',
            deleted && 'italic text-muted-foreground',
          )}
        >
          {deleted ? '[deleted]' : comment.text}
        </p>

        {!deleted && (onReply || isAuthor) && (
          <div className="mt-1 flex items-center gap-1 text-muted-foreground">
            {onReply && (
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-1.5 text-xs"
                onClick={onReply}
              >
                Reply
              </Button>
            )}
            {isAuthor && (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 gap-1 px-1.5 text-xs"
                  onClick={() => setEditing(true)}
                >
                  <Pencil className="size-3" /> Edit
                </Button>
                <DeleteComment matchId={matchId} comment={comment} />
              </>
            )}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * The composer used for a new top-level comment, a reply (`parentId` set), or an
 * edit (`editing` set — pre-fills the text and PATCHes instead of POSTing).
 * Invalidates the comment thread (and, for a top-level create/delete, the match
 * so `comment_count` refreshes) on success.
 */
function CommentComposer({
  matchId,
  parentId,
  editing,
  placeholder = 'Add a comment…',
  onDone,
}: {
  matchId: string
  parentId?: string
  editing?: Comment
  placeholder?: string
  onDone?: () => void
}) {
  const queryClient = useQueryClient()
  const [text, setText] = useState(editing?.text ?? '')

  const submit = useMutation({
    mutationFn: async () => {
      const body = text.trim()
      if (!body) return
      if (editing) {
        const { error } = await fetchClient.PATCH(
          '/matches/{match_id}/comments/{comment_id}',
          {
            params: {
              path: { match_id: matchId, comment_id: editing.id },
            },
            body: { text: body },
          },
        )
        if (error) throw new Error('Failed to edit comment')
      } else {
        const { error } = await fetchClient.POST(
          '/matches/{match_id}/comments',
          {
            params: { path: { match_id: matchId } },
            body: { text: body, parent_id: parentId },
          },
        )
        if (error) throw new Error('Failed to post comment')
      }
    },
    onSuccess: () => {
      // Refresh the thread these belong to.
      queryClient.invalidateQueries({ queryKey: ['comments', matchId] })
      if (parentId) {
        queryClient.invalidateQueries({
          queryKey: ['comment-replies', matchId, parentId],
        })
      }
      // A new top-level comment / reply changes the match's comment_count.
      if (!editing) {
        queryClient.invalidateQueries({ queryKey: ['match', matchId] })
      }
      if (!editing) setText('')
      onDone?.()
    },
  })

  const disabled = submit.isPending || text.trim().length === 0

  return (
    <div className={cn(parentId || editing ? '' : 'mb-1')}>
      <textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={placeholder}
        rows={parentId || editing ? 2 : 3}
        className="w-full resize-y rounded-lg border bg-background px-3 py-2 text-sm outline-none ring-primary/30 focus:ring-2"
      />
      {submit.isError && (
        <p className="mt-1 text-xs text-red-600">
          Something went wrong. Please try again.
        </p>
      )}
      <div className="mt-2 flex items-center gap-2">
        <Button
          size="sm"
          disabled={disabled}
          onClick={() => submit.mutate()}
        >
          {submit.isPending
            ? 'Saving…'
            : editing
              ? 'Save'
              : parentId
                ? 'Reply'
                : 'Comment'}
        </Button>
        {onDone && (
          <Button
            variant="ghost"
            size="sm"
            disabled={submit.isPending}
            onClick={onDone}
          >
            Cancel
          </Button>
        )}
      </div>
    </div>
  )
}

/** Author-only delete, with a lightweight inline confirm. */
function DeleteComment({
  matchId,
  comment,
}: {
  matchId: string
  comment: Comment
}) {
  const queryClient = useQueryClient()
  const [confirming, setConfirming] = useState(false)

  const del = useMutation({
    mutationFn: async () => {
      const { error } = await fetchClient.DELETE(
        '/matches/{match_id}/comments/{comment_id}',
        {
          params: { path: { match_id: matchId, comment_id: comment.id } },
        },
      )
      if (error) throw new Error('Failed to delete comment')
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['comments', matchId] })
      if (comment.parent_id) {
        queryClient.invalidateQueries({
          queryKey: ['comment-replies', matchId, comment.parent_id],
        })
      }
      queryClient.invalidateQueries({ queryKey: ['match', matchId] })
    },
  })

  if (!confirming) {
    return (
      <Button
        variant="ghost"
        size="sm"
        className="h-6 gap-1 px-1.5 text-xs text-destructive hover:text-destructive"
        onClick={() => setConfirming(true)}
      >
        <Trash2 className="size-3" /> Delete
      </Button>
    )
  }

  return (
    <span className="flex items-center gap-1 text-xs">
      <Button
        variant="ghost"
        size="sm"
        className="h-6 px-1.5 text-xs text-destructive hover:text-destructive"
        disabled={del.isPending}
        onClick={() => del.mutate()}
      >
        {del.isPending ? 'Deleting…' : 'Confirm'}
      </Button>
      <Button
        variant="ghost"
        size="sm"
        className="h-6 px-1.5 text-xs"
        disabled={del.isPending}
        onClick={() => setConfirming(false)}
      >
        Cancel
      </Button>
    </span>
  )
}
