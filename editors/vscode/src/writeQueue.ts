/** Serializes the operations a webview message can start against the shared
 * `TextDocument`.
 *
 * `applyWebviewEdit` awaits `workspace.applyEdit`, so an `edit` message is
 * still in flight when the next message arrives. The Raw pane's ⌘S posts
 * `edit` and `request-save` in the *same tick* (apply-then-save is one
 * keystroke), and unserialized the save command can reach the workbench first
 * and write the pre-Apply text. Everything that consumes document content
 * therefore queues behind the edits already started.
 *
 * A rejected step must not wedge the queue: the chain continues either way,
 * and each caller keeps its own error handling. */
export interface WriteQueue {
  /** Queue `op` after everything already queued; resolves when `op` settles. */
  run(op: () => void | PromiseLike<unknown>): Promise<void>;
}

export function createWriteQueue(): WriteQueue {
  let tail: Promise<void> = Promise.resolve();
  return {
    run(op) {
      const next = tail.then(
        () => op(),
        () => op(),
      );
      // Swallow here only to keep the chain alive; the returned promise still
      // carries the rejection to whoever awaits this particular step.
      tail = next.then(
        () => undefined,
        () => undefined,
      );
      return next.then(() => undefined);
    },
  };
}
