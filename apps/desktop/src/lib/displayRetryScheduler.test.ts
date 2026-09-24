import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { DisplayRetryScheduler, DISPLAY_RETRY_LIMIT } from './displayRetryScheduler';

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(0); });
afterEach(() => { vi.useRealTimers(); });

test('one visible demand gets only the initial attempt and 5/30/120 second retries', async () => {
  const scheduler = new DisplayRetryScheduler();
  const run = vi.fn(async () => false);
  scheduler.subscribe('body:post', run, () => false);
  await vi.advanceTimersByTimeAsync(0);
  expect(run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(4_999);
  expect(run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(run).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(run).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(120_000);
  expect(run).toHaveBeenCalledTimes(4);
  await vi.advanceTimersByTimeAsync(600_000);
  expect(run).toHaveBeenCalledTimes(4);
  scheduler.dispose();
});

test('capacity deferral does not exhaust the Rust network retry budget', async () => {
  const scheduler = new DisplayRetryScheduler();
  let exhausted = false;
  const run = vi.fn(async () => ({ display_retry_next_at_ms: exhausted ? null : Date.now() + 5_000 }));
  scheduler.subscribe('body:deferred', run, () => false);
  await vi.advanceTimersByTimeAsync(0);
  for (let index = 0; index < 4; index += 1) {
    await vi.advanceTimersByTimeAsync(5_000);
  }
  expect(run).toHaveBeenCalledTimes(5);
  exhausted = true;
  await vi.advanceTimersByTimeAsync(5_000);
  expect(run).toHaveBeenCalledTimes(6);
  await vi.advanceTimersByTimeAsync(120_000);
  expect(run).toHaveBeenCalledTimes(6);
  scheduler.dispose();
});

test('leaving the window stops retries and returning does not reset the budget', async () => {
  const scheduler = new DisplayRetryScheduler();
  const run = vi.fn(async () => false);
  const leave = scheduler.subscribe('reply:post', run, () => false);
  await vi.advanceTimersByTimeAsync(0);
  leave();
  await vi.advanceTimersByTimeAsync(5_000);
  expect(run).toHaveBeenCalledTimes(1);
  scheduler.subscribe('reply:post', run, () => false);
  await vi.advanceTimersByTimeAsync(0);
  expect(run).toHaveBeenCalledTimes(2);
  scheduler.dispose();
});

test('the same demand is fetched once for two views and old history stays bounded', async () => {
  const scheduler = new DisplayRetryScheduler();
  const run = vi.fn(async () => 'ready');
  const first = vi.fn(() => false);
  const second = vi.fn(() => true);
  scheduler.subscribe('session:one', run, first);
  scheduler.subscribe('session:one', run, second);
  await vi.advanceTimersByTimeAsync(0);
  expect(run).toHaveBeenCalledTimes(1);
  expect(first).toHaveBeenCalledWith('ready');
  expect(second).toHaveBeenCalledWith('ready');
  expect(scheduler.size).toBe(0);
  for (let index = 0; index < DISPLAY_RETRY_LIMIT * 10; index += 1) {
    scheduler.subscribe(`old:${index}`, async () => null, () => false);
  }
  expect(scheduler.size).toBe(DISPLAY_RETRY_LIMIT);
  scheduler.dispose();
});

test('four stalled requests do not grow a waiting queue and a free slot advances another key', async () => {
  const scheduler = new DisplayRetryScheduler();
  const releases: Array<() => void> = [];
  for (let index = 0; index < 4; index += 1) {
    scheduler.subscribe(`stalled:${index}`, () => new Promise<boolean>((resolve) => {
      releases.push(() => resolve(false));
    }), () => false);
  }
  const next = vi.fn(async () => true);
  scheduler.subscribe('normal', next, (result) => result);
  await vi.advanceTimersByTimeAsync(0);
  expect(releases).toHaveLength(4);
  expect(next).not.toHaveBeenCalled();
  releases[0]();
  await vi.advanceTimersByTimeAsync(0);
  expect(next).toHaveBeenCalledTimes(1);
  scheduler.dispose();
});
