import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';

import type { PostView } from '@/lib/api';
import { DisplayRetryContext, DisplayRetryScheduler } from '@/lib/displayRetryScheduler';

import { PostCard } from './PostCard';
import { createView } from './PostCard.testHelpers';
import { PostReloadContext } from './postReloadContext';
import type { PostCardView } from './types';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function replyView(contentStatus: 'Available' | 'Missing' = 'Available'): PostCardView {
  const base = createView();
  const replyPreview = {
    object_id: 'parent-1',
    topic: 'kukuri:topic:source',
    author: {
      pubkey: 'b'.repeat(64),
      name: 'parent-author',
      display_name: 'Parent Author',
      picture_asset: null,
    },
    content: contentStatus === 'Missing' ? '[blob pending]' : 'parent body',
    content_status: contentStatus,
    attachments: [],
    content_labels: [],
    root_id: 'parent-1',
    reply_to: null,
  } satisfies NonNullable<PostView['reply_preview']>;
  return {
    ...base,
    post: { ...base.post, reply_to: replyPreview.object_id, reply_preview: replyPreview },
    replyParentAuthor: {
      pubkey: replyPreview.author.pubkey,
      label: replyPreview.author.display_name,
      picture: null,
    },
  };
}

function card(view: PostCardView) {
  return (
    <PostCard
      view={view}
      onOpenAuthor={() => undefined}
      onOpenThread={() => undefined}
      onReply={() => undefined}
    />
  );
}

function stubVisibleIntersectionObserver() {
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      constructor(private readonly callback: IntersectionObserverCallback) {}
      observe(target: Element) {
        this.callback([{ isIntersecting: true, target } as IntersectionObserverEntry], this as never);
      }
      disconnect() {}
      unobserve() {}
      takeRecords() { return []; }
      readonly root = null;
      readonly rootMargin = '120px';
      readonly thresholds = [0];
    }
  );
}

test('missing post body reloads only that body without opening the thread', async () => {
  const user = userEvent.setup();
  const base = createView();
  const missing = { ...base.post, content: '[blob pending]', content_status: 'Missing' as const };
  const onOpenThread = vi.fn();
  const reload = vi.fn(async () => ({
    ...missing,
    content: 'recovered body',
    content_status: 'Available' as const,
  }));
  render(
    <PostReloadContext.Provider value={reload}>
      <PostCard
        view={createView({ post: missing })}
        onOpenAuthor={() => undefined}
        onOpenThread={onOpenThread}
        onReply={() => undefined}
      />
    </PostReloadContext.Provider>
  );

  await user.click(screen.getByRole('button', { name: 'Retry loading' }));
  expect(reload).toHaveBeenCalledWith(missing, missing.object_id, true);
  expect(onOpenThread).not.toHaveBeenCalled();
  expect(await screen.findByText('recovered body')).toBeInTheDocument();
}, 10_000);

test('missing reply preview body retries only the referenced body', async () => {
  const user = userEvent.setup();
  const view = replyView('Missing');
  const replyPreview = view.post.reply_preview!;
  const reload = vi.fn(async () => ({
    ...view.post,
    reply_preview: {
      ...replyPreview,
      content: 'recovered reply parent',
      content_status: 'Available' as const,
    },
  }));
  render(
    <PostReloadContext.Provider value={reload}>
      {card(view)}
    </PostReloadContext.Provider>
  );

  await user.click(screen.getByRole('button', { name: 'Retry loading' }));
  expect(reload).toHaveBeenCalledWith(view.post, replyPreview.object_id, true);
  expect(await screen.findByText('recovered reply parent')).toBeInTheDocument();
}, 10_000);

test('visible missing reply preview follows the bounded automatic recovery schedule', async () => {
  vi.useFakeTimers();
  stubVisibleIntersectionObserver();
  const view = replyView('Missing');
  const recovered = {
    ...view.post,
    reply_preview: {
      ...view.post.reply_preview!,
      content: 'automatically recovered parent',
      content_status: 'Available' as const,
    },
  };
  const reload = vi.fn()
    .mockResolvedValueOnce(view.post)
    .mockResolvedValueOnce(recovered);
  const scheduler = new DisplayRetryScheduler();
  const { unmount } = render(
    <DisplayRetryContext.Provider value={scheduler}>
      <PostReloadContext.Provider value={reload}>{card(view)}</PostReloadContext.Provider>
    </DisplayRetryContext.Provider>
  );

  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(reload).toHaveBeenCalledWith(view.post, null, false);
  expect(reload).toHaveBeenCalledTimes(1);
  await act(async () => vi.advanceTimersByTimeAsync(4_999));
  expect(reload).toHaveBeenCalledTimes(1);
  await act(async () => vi.advanceTimersByTimeAsync(1));
  expect(reload).toHaveBeenCalledTimes(2);
  expect(screen.getByText('automatically recovered parent')).toBeInTheDocument();

  unmount();
  scheduler.dispose();
  await act(async () => vi.advanceTimersByTimeAsync(600_000));
  expect(reload).toHaveBeenCalledTimes(2);
});

test('unrecovered reply preview stops after four automatic attempts', async () => {
  vi.useFakeTimers();
  stubVisibleIntersectionObserver();
  const view = replyView('Missing');
  const reload = vi.fn(async () => view.post);
  const scheduler = new DisplayRetryScheduler();
  const { unmount } = render(
    <DisplayRetryContext.Provider value={scheduler}>
      <PostReloadContext.Provider value={reload}>{card(view)}</PostReloadContext.Provider>
    </DisplayRetryContext.Provider>
  );

  await act(async () => vi.advanceTimersByTimeAsync(0));
  await act(async () => vi.advanceTimersByTimeAsync(3_000_000));
  expect(reload).toHaveBeenCalledTimes(4);
  await act(async () => vi.advanceTimersByTimeAsync(3_000_000));
  expect(reload).toHaveBeenCalledTimes(4);
  unmount();
  scheduler.dispose();
});

test('visible missing post body retries through the same scheduler', async () => {
  vi.useFakeTimers();
  stubVisibleIntersectionObserver();
  const view = createView();
  const missing = { ...view.post, content: '[blob pending]', content_status: 'Missing' as const };
  const reload = vi.fn(async () => ({ ...missing, content: 'recovered body', content_status: 'Available' as const }));
  const scheduler = new DisplayRetryScheduler();
  const { unmount } = render(
    <DisplayRetryContext.Provider value={scheduler}>
      <PostReloadContext.Provider value={reload}>{card(createView({ post: missing }))}</PostReloadContext.Provider>
    </DisplayRetryContext.Provider>
  );
  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(reload).toHaveBeenCalledWith(missing, null, false);
  expect(screen.getByText('recovered body')).toBeInTheDocument();
  unmount();
  scheduler.dispose();
});

test('two visible cards for one missing parent share a request and both update', async () => {
  vi.useFakeTimers();
  stubVisibleIntersectionObserver();
  const view = replyView('Missing');
  const recovered = { ...view.post, reply_preview: {
    ...view.post.reply_preview!, content: 'shared parent', content_status: 'Available' as const,
  } };
  const reload = vi.fn(async () => recovered);
  const scheduler = new DisplayRetryScheduler();
  const { unmount } = render(
    <DisplayRetryContext.Provider value={scheduler}>
      <PostReloadContext.Provider value={reload}>{card(view)}{card(view)}</PostReloadContext.Provider>
    </DisplayRetryContext.Provider>
  );
  await act(async () => vi.advanceTimersByTimeAsync(0));
  expect(reload).toHaveBeenCalledTimes(1);
  expect(screen.getAllByText('shared parent')).toHaveLength(2);
  unmount();
  scheduler.dispose();
});

test('post reload is the rightmost action and reloads the whole card', async () => {
  const user = userEvent.setup();
  const view = createView();
  const reload = vi.fn(async () => view.post);
  render(
    <PostReloadContext.Provider value={reload}>
      <PostCard
        view={view}
        onOpenAuthor={() => undefined}
        onOpenThread={() => undefined}
        onReply={() => undefined}
        showBookmarkAction
        onToggleBookmark={() => undefined}
      />
    </PostReloadContext.Provider>
  );

  const reloadButton = screen.getByRole('button', { name: 'Reload post' });
  const actions = reloadButton.closest('.post-actions');
  expect(actions).not.toBeNull();
  expect(within(actions as HTMLElement).getAllByRole('button').at(-1)).toBe(reloadButton);
  await user.click(reloadButton);
  expect(reload).toHaveBeenCalledWith(view.post, undefined, true);
}, 10_000);

test('post reload is unavailable while content or media is gated', () => {
  const reload = vi.fn(async () => createView().post);
  const { rerender } = render(
    <PostReloadContext.Provider value={reload}>
      {card(createView({ adultContentGated: true }))}
    </PostReloadContext.Provider>
  );
  expect(screen.queryByRole('button', { name: 'Reload post' })).not.toBeInTheDocument();

  rerender(
    <PostReloadContext.Provider value={reload}>
      {card(createView({
        media: { ...createView().media, state: 'gated', gatedBy: 'shared_media' },
      }))}
    </PostReloadContext.Provider>
  );
  expect(screen.queryByRole('button', { name: 'Reload post' })).not.toBeInTheDocument();
  expect(reload).not.toHaveBeenCalled();
});

test('late reload result does not cross a backend or scope change', async () => {
  const user = userEvent.setup();
  const initialView = createView();
  let resolveReload!: (post: PostCardView['post']) => void;
  const oldReload = vi.fn(
    () => new Promise<PostCardView['post']>((resolve) => { resolveReload = resolve; })
  );
  const nextReload = vi.fn(async () => initialView.post);
  const nextView = createView({ post: { ...initialView.post, content: 'current scope content' } });
  const { rerender } = render(
    <PostReloadContext.Provider value={oldReload}>{card(initialView)}</PostReloadContext.Provider>
  );

  await user.click(screen.getByRole('button', { name: 'Reload post' }));
  rerender(
    <PostReloadContext.Provider value={nextReload}>{card(nextView)}</PostReloadContext.Provider>
  );
  act(() => resolveReload({ ...initialView.post, content: 'stale backend content' }));

  expect(await screen.findByText('current scope content')).toBeInTheDocument();
  expect(screen.queryByText('stale backend content')).not.toBeInTheDocument();
}, 10_000);
