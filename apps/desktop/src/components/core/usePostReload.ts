import { useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react';

import type { PostView } from '@/lib/api';
import { DisplayRetryContext } from '@/lib/displayRetryScheduler';

import { PostReloadContext } from './postReloadContext';

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
  const retryScheduler = useContext(DisplayRetryContext);
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
  const automaticPending = post.content_status === 'Missing' ||
    (recoverMissingReply && Boolean(post.reply_to) && post.reply_preview?.content_status !== 'Available');
  const sourceScopeKey = reloadScopeKey(sourcePost);

  useLayoutEffect(() => {
    reloadPostRef.current = reloadPost;
    sourcePostRef.current = sourcePost;
  }, [reloadPost, sourcePost]);

  useEffect(() => {
    const card = cardRef.current;
    if (
      !reloadAvailable ||
      !automaticPending ||
      !retryScheduler ||
      !card ||
      typeof IntersectionObserver === 'undefined'
    ) {
      return;
    }
    let intersecting = false;
    let release: (() => void) | null = null;
    const run = async () => {
      const reloadAtStart = reloadPostRef.current;
      if (!reloadAtStart || reloadInFlightRef.current) return null;
      reloadInFlightRef.current = true;
      setReloadPending(true);
      try {
        return await reloadAtStart(sourcePostRef.current, null, false);
      } catch {
        return null;
      } finally {
        reloadInFlightRef.current = false;
        setReloadPending(false);
      }
    };
    const receive = (updated: PostView | null) => {
      const currentSourcePost = sourcePostRef.current;
      if (!updated || reloadScopeKey(currentSourcePost) !== sourceScopeKey) return false;
      setReloadedPost({ source: currentSourcePost, value: updated });
      return updated.content_status === 'Available' &&
        (!recoverMissingReply || !updated.reply_to || updated.reply_preview?.content_status === 'Available');
    };
    const update = () => {
      if (intersecting && document.visibilityState !== 'hidden') {
        release ??= retryScheduler.subscribe(`${sourceScopeKey}\0elements`, run, receive);
      } else {
        release?.();
        release = null;
      }
    };
    const observer = new IntersectionObserver(
      (entries) => {
        intersecting = entries.some((entry) => entry.isIntersecting);
        update();
      },
      { rootMargin: '120px' }
    );
    observer.observe(card);
    document.addEventListener('visibilitychange', update);
    return () => {
      observer.disconnect();
      document.removeEventListener('visibilitychange', update);
      release?.();
    };
  }, [automaticPending, reloadAvailable, retryScheduler, recoverMissingReply, sourceScopeKey]);

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
          if (updated.content_status === 'Available' &&
              (!recoverMissingReply || !updated.reply_to || updated.reply_preview?.content_status === 'Available')) {
            retryScheduler?.forget(`${sourceScopeKey}\0elements`);
          }
        }
      } catch {
        setReloadFailed(true);
      } finally {
        reloadInFlightRef.current = false;
        setReloadPending(false);
      }
    },
    [post, reloadAvailable, reloadPending, reloadPost, sourcePost, recoverMissingReply,
      retryScheduler, sourceScopeKey]
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
