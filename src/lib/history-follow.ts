/** Live history only. Saved sessions never opt into this controller. */
export class HistoryFollow {
  private deadline = 0;
  readonly idleMilliseconds = 60_000;

  get paused() { return this.deadline !== 0; }
  pause(now: number) { this.deadline = now + this.idleMilliseconds; }
  resume() { this.deadline = 0; }
  resumeIfIdle(now: number, interacting: boolean): boolean {
    if (!this.paused) return false;
    if (interacting) { this.pause(now); return false; }
    if (now < this.deadline) return false;
    this.resume();
    return true;
  }
}
