// Shared y/n confirmation-prompt buttons, rendered identically by the desktop
// overlay (`ui.ts`) and the touch prompt sheet (`touch/app.ts`). Each button
// carries the `PromptKey` character it answers with in `data-pk`; the prompt
// question itself is `SessionSnapshot.status`, rendered by the host.
import type { PromptView, Intent, SessionSnapshot } from "./types";

// Per-kind button rows: [label, key, primary?]. The affirmative action sits
// last (primary); Cancel always answers `n` (core treats any non-listed key
// as "no"/cancel for every prompt kind).
const PROMPT_BUTTONS: Record<PromptView, [string, string, boolean?][]> = {
  ConfirmQuit: [
    ["Cancel", "n"],
    ["Quit", "y", true],
  ],
  Collision: [
    ["Cancel", "n"],
    ["Rename", "r"],
    ["Overwrite", "o", true],
  ],
  TypeChange: [
    ["Cancel", "n"],
    ["Change type", "y", true],
  ],
  ArrayUpgrade: [
    ["Cancel", "n"],
    ["Convert & paste", "y", true],
  ],
  JsoncUpgrade: [
    ["Cancel", "n"],
    ["Upgrade to JSONC", "y", true],
  ],
};

// Short sheet/dialog title per prompt kind (the touch sheet header).
const PROMPT_TITLES: Record<PromptView, string> = {
  ConfirmQuit: "Quit?",
  Collision: "Key collision",
  TypeChange: "Change type?",
  ArrayUpgrade: "Convert to array?",
  JsoncUpgrade: "Enable comments?",
};

export function promptTitle(kind: PromptView): string {
  return PROMPT_TITLES[kind] ?? "Confirm";
}

// Fallback questions for prompts the core raises without a status line (the
// TUI renders these texts itself).
const PROMPT_QUESTIONS: Partial<Record<PromptView, string>> = {
  JsoncUpgrade: "Introduce a // comment? This makes the file JSONC.",
  ArrayUpgrade: "Reformat the array to multiline and insert?",
  ConfirmQuit: "Unsaved changes — quit?",
};

// The question line. `text` is `snap.status ?? snap.error` (collision reports
// via `error`), written for the keyboard TUI with a trailing key legend
// ("… y/n", "— o/r/c") that the buttons replace — strip it.
export function promptQuestion(kind: PromptView, text: string | undefined): string {
  const q = text?.replace(/\s*[—–-]?\s*\S+\/\S+\s*$/, "").trim();
  return q || PROMPT_QUESTIONS[kind] || "Confirm?";
}

export function promptButtonsHTML(kind: PromptView): string {
  const btns = PROMPT_BUTTONS[kind] ?? [
    ["No", "n"],
    ["Yes", "y", true],
  ];
  return (
    '<div class="row-btns prompt-btns">' +
    btns
      .map(
        ([label, pk, primary]) =>
          `<button class="btn${primary ? " primary" : ""}" data-pk="${pk}">${label}</button>`,
      )
      .join("") +
    "</div>"
  );
}

// Delegated click → PromptKey. Bind ONCE on a stable container (the host
// rewrites the container's innerHTML per render; the listener survives).
export function bindPromptClicks(
  container: HTMLElement,
  send: (intent: Intent) => SessionSnapshot | void,
): void {
  container.addEventListener("click", (ev) => {
    const btn = (ev.target as HTMLElement).closest<HTMLElement>("[data-pk]");
    if (btn?.dataset.pk) send({ PromptKey: btn.dataset.pk });
  });
}
