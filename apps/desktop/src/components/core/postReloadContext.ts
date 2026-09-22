import { createContext } from 'react';

import type { PostView } from '@/lib/api';

export type PostReload = (
  post: PostView,
  bodyObjectId?: string | null,
  manual?: boolean
) => Promise<PostView | null>;

export const PostReloadContext = createContext<PostReload | null>(null);
