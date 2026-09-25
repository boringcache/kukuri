import { useContext, useEffect, useRef } from 'react';
import { MediaDemandContext } from './mediaRetryContext';

export function MediaDemandObserver({ hash }: { hash: string | null }) {
  const demand = useContext(MediaDemandContext);
  const marker = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (!hash || !demand || !marker.current) return;
    let active = false;
    const setVisible = (visible: boolean) => {
      if (active === visible) return;
      active = visible;
      demand(hash, visible);
    };
    if (typeof IntersectionObserver === 'undefined') {
      setVisible(true);
      return () => setVisible(false);
    }
    const observer = new IntersectionObserver(([entry]) => setVisible(Boolean(entry?.isIntersecting)));
    observer.observe(marker.current);
    return () => {
      observer.disconnect();
      setVisible(false);
    };
  }, [demand, hash]);

  return hash && demand ? <span ref={marker} className='media-demand-marker' aria-hidden='true' /> : null;
}
