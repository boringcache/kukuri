import { useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react';

import type { PostView } from '@/lib/api';

import { PostReloadContext } from './postReloadContext';

const AUTOMATIC_REPLY_RETRY_DELAYS_MS = [
  0,
  5_000,
  30_000,
  120_000,
  600_000,
  600_000,
  600_000,
  600_000,
];

function reloadScopeKey(post: PostView) {
  const topicId = post.published_topic_id ?? post.origin_topic_id ?? '';
  return `${topicId}\u0000${post.channel_id ?? ''}\u0000${post.object_id}`;
}

export function usePostReload({
  sourcePost,
  adultContentGated,
  mediaState,
  recoverMissingReply,
}: {
  sourcePost: PostView;
  adultContentGated: boolean;
  mediaState: 'loading' | 'ready' | 'unavailable' | 'gated' | 'pending';
  recoverMissingReply: boolean;
}) {
  const reloadPost = useContext(PostReloadContext);
  const reloadPostRef = useRef(reloadPost);
  const sourcePostRef = useRef(sourcePost);
  const cardRef = useRef<HTMLElement | null>(null);
  const reloadInFlightRef = useRef(false);
  const [reloadedPost, setReloadedPost] = useState<{
    source: PostView;
    value: PostView;
  } | null>(null);
  const [reloadPending, setReloadPending] = useState(false);
  const [reloadFailed, setReloadFailed] = useState(false);
  const post = reloadedPost?.source === sourcePost ? reloadedPost.value : sourcePost;
  const reloadAvailable =
    reloadPost !== null && !adultContentGated && mediaState !== 'gated';
  const automaticReplyObjectId =
    recoverMissingReply && post.reply_preview?.content_status === 'Missing'
      ? post.reply_preview.object_id
      : null;
  const sourceScopeKey = reloadScopeKey(sourcePost);

  useLayoutEffect(() => {
    reloadPostRef.current = reloadPost;
    sourcePostRef.current = sourcePost;
  }, [reloadPost, sourcePost]);

  useEffect(() => {
    const card = cardRef.current;
    if (
      !reloadAvailable ||
      !automaticReplyObjectId ||
      !card ||
      typeof IntersectionObserver === 'undefined'
    ) {
      return;
    }

    let cancelled = false;
    let started = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const wait = (delayMs: number) =>
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, delayMs);
      });
    const retryVisibleReply = async () => {
      for (const delayMs of AUTOMATIC_REPLY_RETRY_DELAYS_MS) {
        if (delayMs > 0) await wait(delayMs);
        if (cancelled) return;
        const reloadAtStart = reloadPostRef.current;
        const postAtStart = sourcePostRef.current;
        if (!reloadAtStart || reloadInFlightRef.current) continue;
        reloadInFlightRef.current = true;
        setReloadPending(true);
        try {
          const updated = await reloadAtStart(postAtStart, automaticReplyObjectId, false);
          const currentSourcePost = sourcePostRef.current;
          if (
            cancelled ||
            reloadPostRef.current !== reloadAtStart ||
            reloadScopeKey(currentSourcePost) !== sourceScopeKey
          ) {
            return;
          }
          if (updated) {
            setReloadedPost({ source: currentSourcePost, value: updated });
            if (updated.reply_preview?.content_status === 'Available') return;
          }
        } catch {
          // Automatic recovery remains quiet and follows the finite backoff schedule.
        } finally {
          reloadInFlightRef.current = false;
          if (!cancelled) setReloadPending(false);
        }
      }
    };
    const observer = new IntersectionObserver(
      (entries) => {
        if (!started && entries.some((entry) => entry.isIntersecting)) {
          started = true;
          observer.disconnect();
          void retryVisibleReply();
        }
      },
      { rootMargin: '120px' }
    );
    observer.observe(card);
    return () => {
      cancelled = true;
      observer.disconnect();
      if (timer !== null) clearTimeout(timer);
      setReloadPending(false);
    };
  }, [automaticReplyObjectId, reloadAvailable, reloadPost, sourceScopeKey]);

  const runReload = useCallback(
    async (bodyObjectId?: string | null) => {
      if (!reloadAvailable || reloadPending || reloadInFlightRef.current || !reloadPost) return;
      const reloadAtStart = reloadPost;
      const sourcePostAtStart = sourcePost;
      reloadInFlightRef.current = true;
      setReloadPending(true);
      setReloadFailed(false);
      try {
        const updated = await reloadAtStart(post, bodyObjectId, true);
        if (
          updated &&
          reloadPostRef.current === reloadAtStart &&
          sourcePostRef.current === sourcePostAtStart
        ) {
          setReloadedPost({ source: sourcePostAtStart, value: updated });
        }
      } catch {
        setReloadFailed(true);
      } finally {
        reloadInFlightRef.current = false;
        setReloadPending(false);
      }
    },
    [post, reloadAvailable, reloadPending, reloadPost, sourcePost]
  );

  return {
    cardRef,
    post,
    reloadAvailable,
    reloadFailed,
    reloadPending,
    runReload,
  };
}
