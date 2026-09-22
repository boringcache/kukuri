import { useSessionDisplay, type SessionDisplayContext } from './useSessionDisplay';
export type { SessionDisplayContext } from './useSessionDisplay';
import { type ReactNode, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { SessionCandidateView, TimelineScope } from '@/lib/api';
import { Button } from '@/components/ui/button';

/** DOMにあるだけでは取得しない。viewport/Columnのclipとdocument非表示を反映する。 */
export function SessionVisibility({ context, sessionId, kind, replicaId = '', allowRetry = false, children }: {
  context?: SessionDisplayContext; sessionId: string; kind: string; replicaId?: string; allowRetry?: boolean; children: ReactNode;
}) {
  const { element, failed, retry } = useSessionDisplay<HTMLLIElement>({ context, sessionId, kind, replicaId });
  const { t } = useTranslation('common');
  return <li ref={element}>
    {children}
    {failed || allowRetry ? <Button variant='secondary' onClick={() => retry.current()}>{t('actions.retry')}</Button> : null}
  </li>;
}

/** 不足候補は通常のsession操作を持たず、表示した時だけ取得する。 */
export function PendingSessionCards({ context, kind, refreshToken, knownIds, dome = false, onCountChange, targetId }: {
  context: SessionDisplayContext; kind: string; refreshToken: unknown; knownIds: string[]; dome?: boolean; onCountChange?: (count: number) => void; targetId?: string | null;
}) {
  const [candidates, setCandidates] = useState<SessionCandidateView[]>([]);
  const { api, topic } = context;
  const scopeJson = JSON.stringify(context.scope);
  const { t } = useTranslation('common');
  useEffect(() => {
    let cancelled = false;
    void api.listSessionCandidates(topic, JSON.parse(scopeJson) as TimelineScope)
      .then((items) => { if (!cancelled) setCandidates(items); })
      .catch(() => { if (!cancelled) setCandidates([]); });
    return () => { cancelled = true; };
  }, [api, topic, scopeJson, refreshToken]);
  const visibleCandidates = candidates.filter((item) => item.kind === kind && (kind !== 'game' || item.session_id.startsWith('dome-') === dome) && !knownIds.includes(item.session_id));
  // 明示的に開いた詳細は窓の候補に無くても対象1件を要求できる。
  if (targetId && !knownIds.includes(targetId) && !visibleCandidates.some((item) => item.session_id === targetId)) {
    visibleCandidates.unshift({ replica_id: '', session_id: targetId, kind });
  }
  const count = visibleCandidates.length;
  useEffect(() => { onCountChange?.(count); }, [count, onCountChange]);
  if (count === 0) return null;
  return <ul className='post-list'>
    {visibleCandidates.map((item) => <SessionVisibility key={`${item.replica_id}:${item.session_id}`}
      context={context} kind={kind} sessionId={item.session_id} replicaId={item.replica_id} allowRetry>
      <article className='post-card' tabIndex={targetId === item.session_id ? -1 : undefined}
        data-live-session-id={kind === 'live' ? item.session_id : undefined}
        data-game-room-id={kind === 'game' ? item.session_id : undefined}><p>{t('sessionManifestUnavailable')}</p>
      </article>
    </SessionVisibility>)}
  </ul>;
}
