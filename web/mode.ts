// Shared mode-tag helper (desktop `ui.ts` and touch `touch/app.ts` had
// byte-identical copies - see docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md).
import type { ModeView } from "./types.js";

export function modeTag(m: ModeView): string {
  return typeof m === "string" ? m : Object.keys(m)[0];
}

// Shared batching flag/try-finally shape (desktop `ui.ts` and touch `touch/app.ts`
// had the same structure with a different post-render hook per host - see
// docs/superpowers/plans/2026-08-11-web-code-audit-remediation-plan.md).
export function createBatcher(render: () => void, afterRender?: () => void) {
  let batching = false;
  return {
    isBatching: () => batching,
    batch(fn: () => void) {
      if (batching) return fn(); // nested batches render at the outermost level
      batching = true;
      try {
        fn();
      } finally {
        batching = false;
        render();
        afterRender?.();
      }
    },
  };
}
