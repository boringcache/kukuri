import type { DesktopApi, PostView } from '../types';
import type { RetryPostElementsRequest } from '../types.generated';
import { invokeDesktop } from '../invoke/desktop';
import { command } from '../invoke/dispatch';

export const postReloadApi: Pick<DesktopApi, 'retryPostElements'> = {
  retryPostElements: command('retryPostElements', async (objectId, bodyObjectId = null, manual = true) =>
    invokeDesktop<PostView | null>('retry_post_elements', {
      request: { object_id: objectId, body_object_id: bodyObjectId, manual } satisfies RetryPostElementsRequest,
    })
  ),
};
