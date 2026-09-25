import { act, render } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { MediaDemandObserver } from './MediaDemandObserver';
import { MediaDemandContext } from './mediaRetryContext';

afterEach(() => vi.unstubAllGlobals());

test('video demand exists only while its marker is visible', () => {
  let notify: IntersectionObserverCallback | null = null;
  const disconnect = vi.fn();
  vi.stubGlobal('IntersectionObserver', class {
    constructor(callback: IntersectionObserverCallback) { notify = callback; }
    observe() {}
    disconnect() { disconnect(); }
  });
  const demand = vi.fn();
  const view = render(
    <MediaDemandContext.Provider value={demand}>
      <MediaDemandObserver hash='video-hash' />
    </MediaDemandContext.Provider>
  );
  act(() => notify?.([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(demand).toHaveBeenCalledWith('video-hash', true);
  act(() => notify?.([{ isIntersecting: false } as IntersectionObserverEntry], {} as IntersectionObserver));
  expect(demand).toHaveBeenCalledWith('video-hash', false);
  view.unmount();
  expect(disconnect).toHaveBeenCalledOnce();
});
