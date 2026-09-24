import { type RefObject, useContext, useEffect, useId, useRef, useState } from 'react';
import type { DesktopApi, TimelineScope } from '@/lib/api';
import { DisplayRetryContext } from '@/lib/displayRetryScheduler';
export type SessionDisplayContext = { api: DesktopApi; topic: string; scope: TimelineScope };

export function useSessionDisplay<T extends HTMLElement>({ context, sessionId, kind, replicaId = '', target }: {
  context?: SessionDisplayContext; sessionId: string; kind: string; replicaId?: string; target?: RefObject<T | null>;
}) {
  const ownElement = useRef<T>(null);
  const element = target ?? ownElement;
  const observerId = useId();
  const retryScheduler = useContext(DisplayRetryContext);
  const generation = useRef(0);
  const [failed, setFailed] = useState(false);
  const retry = useRef<() => void>(() => {});
  const api = context?.api;
  const topic = context?.topic;
  const scopeJson = JSON.stringify(context?.scope);
  useEffect(() => {
    if (!api || !topic || !element.current || typeof IntersectionObserver === 'undefined') return;
    const scope = JSON.parse(scopeJson) as TimelineScope;
    const observerKey = `${observerId}:${++generation.current}`;
    let intersecting = false;
    let visible = false;
    let disposed = false;
    // 同一observerの登録と解除は順序を保つ。遅い登録がunmount後に残ることを防ぐ。
    let running = false;
    let queued: { visible: boolean; retry: boolean; done: () => void } | null = null;
    let release: (() => void) | null = null;
    const retryKey = replicaId
      ? `session:${replicaId}:${kind}:${sessionId}`
      : `session:${topic}:${scopeJson}:${kind}:${sessionId}`;
    const send = (next: boolean, manualRetry = false): Promise<void> => new Promise((done) => {
      queued?.done();
      queued = { visible: next, retry: manualRetry, done };
      if (running) return;
      running = true;
      void (async () => {
        while (queued) {
          const request = queued;
          queued = null;
          try {
            await api.setSessionDisplay({ topic, scope, replica_id: replicaId, session_id: sessionId,
              kind, observer: observerKey, visible: request.visible && !disposed, retry: request.retry });
          } catch { if (!disposed) setFailed(true); }
          request.done();
        }
        running = false;
      })();
    });
    const update = () => {
      const next = intersecting && document.visibilityState !== 'hidden';
      if (next !== visible) {
        visible = next;
        void send(next);
      }
      if (next) {
        release ??= retryScheduler?.subscribe(retryKey, async () => {
          await send(true);
          return false;
        }, () => false, true) ?? null;
      } else {
        release?.();
        release = null;
      }
    };
    retry.current = () => { if (visible) { setFailed(false); void send(true, true); } };
    const observer = new IntersectionObserver(([entry]) => {
      intersecting = entry.isIntersecting && entry.intersectionRatio > 0;
      update();
    }, { threshold: 0 });
    observer.observe(element.current);
    document.addEventListener('visibilitychange', update);
    return () => {
      disposed = true;
      observer.disconnect();
      document.removeEventListener('visibilitychange', update);
      release?.();
      void send(false);
    };
  }, [api, topic, scopeJson, sessionId, kind, replicaId, observerId, element, retryScheduler]);
  return { element, failed, retry };
}
