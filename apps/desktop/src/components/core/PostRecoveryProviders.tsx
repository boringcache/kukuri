import type { ReactNode } from 'react';

import { MediaRetryContext, type MediaRetryHandler } from './mediaRetryContext';
import { PostReloadContext, type PostReload } from './postReloadContext';

export function PostRecoveryProviders({
  children,
  mediaRetry,
  postReload,
}: {
  children: ReactNode;
  mediaRetry: MediaRetryHandler;
  postReload: PostReload;
}) {
  return (
    <MediaRetryContext.Provider value={mediaRetry}>
      <PostReloadContext.Provider value={postReload}>{children}</PostReloadContext.Provider>
    </MediaRetryContext.Provider>
  );
}
