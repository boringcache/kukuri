import { useEffect, type MutableRefObject } from 'react';
import { primarySectionForColumn } from '@/shell/slices/workspace';
import type { useDesktopShellStoreApi } from '@/shell/store';

type SectionLoader = (topic: string, channel: string | null) => Promise<void>;

/** projectionの変更通知に追従する表示中session一覧。取得そのものをtimerで繰り返さない。 */
export function useSessionProjectionRefresh(
  storeApi: ReturnType<typeof useDesktopShellStoreApi>,
  loadLiveSection: SectionLoader,
  loadGameSection: SectionLoader,
  visibleColumnIdsRef?: MutableRefObject<string[]>,
) {
  useEffect(() => {
    let disposed = false;
    let running = false;
    let requested = false;
    const refreshSessions = async () => {
      requested = true;
      if (running) return;
      running = true;
      while (requested && !disposed) {
        requested = false;
        const state = storeApi.getState();
        const visible = new Set(visibleColumnIdsRef?.current ?? [state.workspaceState.activeColumnId]);
        const scopes = new Set<string>();
        const tasks: Promise<void>[] = [];
        for (const column of state.workspaceState.columns) {
          if (!visible.has(column.id) || !column.scope) continue;
          const section = primarySectionForColumn(column);
          if (section !== 'live' && section !== 'game') continue;
          const key = JSON.stringify([section, column.scope]);
          if (scopes.has(key)) continue;
          scopes.add(key);
          tasks.push(section === 'live'
            ? loadLiveSection(column.scope.topicId, column.scope.channelId ?? null)
            : loadGameSection(column.scope.topicId, column.scope.channelId ?? null));
        }
        await Promise.allSettled(tasks);
      }
      running = false;
    };
    const unsubscribe = storeApi.subscribe((state, previous) => {
      // projection/candidateの変更でだけ再取得する。manifest取得のtimer retryではない。
      if (state.syncStatus.last_sync_ts !== previous.syncStatus.last_sync_ts) void refreshSessions();
    });
    return () => { disposed = true; unsubscribe(); };
  }, [loadGameSection, loadLiveSection, storeApi, visibleColumnIdsRef]);
}
