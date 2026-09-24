import { createContext } from 'react';
import { DISPLAY_RETRY_ATTEMPTS, DISPLAY_RETRY_DELAYS_MS, DISPLAY_RETRY_LIMIT } from './displayRetryPolicy';

export { DISPLAY_RETRY_ATTEMPTS, DISPLAY_RETRY_DELAYS_MS, DISPLAY_RETRY_LIMIT } from './displayRetryPolicy';

type Subscriber = { run: () => Promise<unknown>; receive: (value: unknown) => boolean };
type Entry = {
  subscribers: Map<symbol, Subscriber>;
  attempts: number;
  nextAt: number;
  inFlight: boolean;
};

/** One timer and four running requests for the visible display demand. */
export class DisplayRetryScheduler {
  private readonly entries = new Map<string, Entry>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private running = 0;
  private disposed = false;

  subscribe<T>(key: string, run: () => Promise<T>, receive: (value: T) => boolean): () => void {
    if (this.disposed || new TextEncoder().encode(key).length > 256) return () => {};
    let entry = this.entries.get(key);
    if (!entry) {
      if (this.entries.size === DISPLAY_RETRY_LIMIT) {
        const victim = [...this.entries].find(([, item]) => !item.inFlight && item.subscribers.size === 0)
          ?? [...this.entries].find(([, item]) => !item.inFlight);
        if (!victim) return () => {};
        this.entries.delete(victim[0]);
      }
      entry = { subscribers: new Map(), attempts: 0, nextAt: Date.now(), inFlight: false };
      this.entries.set(key, entry);
    }
    const token = Symbol(key);
    entry.subscribers.set(token, { run, receive: (value) => receive(value as T) });
    this.arm();
    return () => {
      entry.subscribers.delete(token);
      this.arm();
    };
  }

  forget(key: string): void {
    this.entries.delete(key);
    this.arm();
  }

  dispose(): void {
    this.disposed = true;
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    this.entries.clear();
  }

  get size(): number { return this.entries.size; }

  private arm(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = null;
    if (this.disposed || this.running === 4) return;
    let nextAt = Infinity;
    for (const entry of this.entries.values()) {
      if (entry.subscribers.size && !entry.inFlight && entry.attempts < DISPLAY_RETRY_ATTEMPTS) {
        nextAt = Math.min(nextAt, entry.nextAt);
      }
    }
    if (Number.isFinite(nextAt)) {
      this.timer = setTimeout(() => this.tick(), Math.max(0, nextAt - Date.now()));
    }
  }

  private tick(): void {
    this.timer = null;
    for (const [key, entry] of this.entries) {
      if (this.running === 4) break;
      const subscriber = entry.subscribers.values().next().value;
      if (!subscriber || entry.inFlight || entry.attempts === DISPLAY_RETRY_ATTEMPTS || entry.nextAt > Date.now()) continue;
      entry.inFlight = true;
      entry.attempts += 1;
      this.running += 1;
      let serverRetryAt: number | null | undefined;
      void Promise.resolve().then(() => subscriber.run()).then((result) => {
        if (result && typeof result === 'object' && 'display_retry_next_at_ms' in result) {
          const due = result.display_retry_next_at_ms;
          if (due === null || typeof due === 'number') serverRetryAt = due;
        }
        let recovered = false;
        for (const listener of entry.subscribers.values()) {
          try { recovered = listener.receive(result) || recovered; } catch { /* detached view */ }
        }
        if (recovered) this.entries.delete(key);
      }).catch(() => {
        // A failed request follows the same finite schedule.
      }).finally(() => {
        entry.inFlight = false;
        this.running -= 1;
        if (this.entries.get(key) === entry) {
          if (serverRetryAt !== undefined) {
            entry.attempts = serverRetryAt === null ? DISPLAY_RETRY_ATTEMPTS : 0;
            if (serverRetryAt !== null) entry.nextAt = Math.max(Date.now() + 1_000, serverRetryAt);
          } else {
            const delay = DISPLAY_RETRY_DELAYS_MS[entry.attempts - 1];
            if (delay !== undefined) entry.nextAt = Date.now() + delay;
          }
        }
        this.arm();
      });
    }
    this.arm();
  }
}

export const DisplayRetryContext = createContext<DisplayRetryScheduler | null>(null);
