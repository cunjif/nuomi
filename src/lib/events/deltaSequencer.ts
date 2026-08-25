/**
 * Live-stream delta sequencer (one instance per session).
 *
 * SEMANTICS — two unrelated "seq" numbers share the wire:
 *  - `session.delta` payload `seq`: NON-PERSISTENT ordinal injected by the
 *    kernel (in-memory counter, starts at 1 per session, reset on session
 *    switch). It exists only to dedupe replays and restore order inside one
 *    live stream. It is never persisted and must not be used for gap
 *    recovery against durable history.
 *  - every other event's `seq` (e.g. `session.message`, whiteboard notes):
 *    the AUTHORITATIVE per-aggregate seq of the row in the SQLite `events`
 *    table (or its domain equivalent), recoverable via listEvents(afterSeq).
 */
import type { DomainEvent } from "./types";

export class DeltaSequencer {
  private lastAppliedSeq = 0;
  private highWaterSeq = 0;
  private readonly pending = new Map<number, DomainEvent>();

  /**
   * Hands over one incoming delta; returns the deltas that may be applied
   * now, in strictly increasing seq order. Replayed/duplicated deltas
   * (seq <= last applied) are dropped; gaps are buffered until filled.
   * Deltas without a seq (legacy producers) pass through untouched.
   */
  accept(event: DomainEvent): DomainEvent[] {
    const seq = event.seq;
    if (typeof seq !== "number") return [event];
    if (seq <= this.lastAppliedSeq) return [];
    this.highWaterSeq = Math.max(this.highWaterSeq, seq);
    this.pending.set(seq, event);
    return this.releaseReady();
  }

  /**
   * A persisted message replaced the live buffer and covers every delta up
   * to `uptoSeq` (its payload `deltaTo`): retires that range — including
   * deltas buffered but not yet delivered — and releases any surviving
   * buffered deltas beyond it.
   */
  markAppliedUpTo(uptoSeq: number): DomainEvent[] {
    if (uptoSeq > this.lastAppliedSeq) this.lastAppliedSeq = uptoSeq;
    for (const key of [...this.pending.keys()]) {
      if (key <= uptoSeq) this.pending.delete(key);
    }
    return this.releaseReady();
  }

  /**
   * A persisted message without coverage info replaced the buffer: the full
   * text supersedes everything seen so far, so retire all of it (stragglers
   * already observed are dropped; only newer seqs apply).
   */
  dropSeen(): void {
    this.lastAppliedSeq = this.highWaterSeq;
    this.pending.clear();
  }

  /** Releases consecutive pending deltas starting right after lastAppliedSeq. */
  private releaseReady(): DomainEvent[] {
    const ready: DomainEvent[] = [];
    let next = this.lastAppliedSeq + 1;
    for (;;) {
      const event = this.pending.get(next);
      if (event === undefined) break;
      this.pending.delete(next);
      ready.push(event);
      this.lastAppliedSeq = next;
      next += 1;
    }
    return ready;
  }
}
