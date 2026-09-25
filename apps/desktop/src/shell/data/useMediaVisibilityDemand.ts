import { useCallback, useRef, useState } from 'react';

export function useMediaVisibilityDemand() {
  const [demandedMediaHashes, setDemandedMediaHashes] = useState<ReadonlySet<string>>(() => new Set());
  const countsRef = useRef(new Map<string, number>());
  const setMediaDemand = useCallback((hash: string, visible: boolean) => {
    const counts = countsRef.current;
    const previous = counts.get(hash) ?? 0;
    const next = Math.max(0, previous + (visible ? 1 : -1));
    if (next === 0) counts.delete(hash);
    else counts.set(hash, next);
    if (previous === 0 && next > 0) {
      setDemandedMediaHashes((current) => new Set([...current, hash]));
    } else if (previous > 0 && next === 0) {
      setDemandedMediaHashes((current) => {
        const hashes = new Set(current);
        hashes.delete(hash);
        return hashes;
      });
    }
  }, []);
  return { demandedMediaHashes, setMediaDemand };
}
