import { useEffect, useMemo, type ReactNode } from 'react';
import { DisplayRetryContext, DisplayRetryScheduler } from '@/lib/displayRetryScheduler';

import { MediaDemandContext, MediaRetryContext, type MediaDemandHandler, type MediaRetryHandler } from './mediaRetryContext';
import { PostReloadContext, type PostReload } from './postReloadContext';

export function PostRecoveryProviders({
  children,
  mediaRetry,
  mediaDemand,
  postReload,
}: {
  children: ReactNode;
  mediaRetry: MediaRetryHandler;
  mediaDemand?: MediaDemandHandler;
  postReload: PostReload;
}) {
  // Backend handler replacement starts a new account-scoped retry history.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const retryScheduler = useMemo(() => new DisplayRetryScheduler(), [postReload, mediaRetry]);
  useEffect(() => () => retryScheduler.dispose(), [retryScheduler]);
  return (
    <DisplayRetryContext.Provider value={retryScheduler}>
      <MediaRetryContext.Provider value={mediaRetry}>
        <MediaDemandContext.Provider value={mediaDemand ?? null}>
          <PostReloadContext.Provider value={postReload}>{children}</PostReloadContext.Provider>
        </MediaDemandContext.Provider>
      </MediaRetryContext.Provider>
    </DisplayRetryContext.Provider>
  );
}
