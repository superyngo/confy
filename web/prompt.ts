// Shared y/n confirmation-prompt buttons, rendered identically by the desktop
// overlay (`ui.ts`) and the touch prompt sheet (`touch/app.ts`). Each button
// carries the `PromptKey` character it answers with in `data-pk`; the prompt
// question itself is `ModeView.Prompt.question`, rendered directly by the host.
import type { PromptView, Intent, SessionSnapshot } from "./types";
import { t } from "./i18n.js";

// Per-kind button rows: [labelKey, key, primary?]. The affirmative action
// sits last (primary); Cancel always answers `n` (core treats any
// non-listed key as "no"/cancel for every prompt kind).
const PROMPT_BUTTONS: Record<PromptView, [string, string, boolean?][]> = {
  ConfirmQuit: [
    ["web.prompt.btn.cancel", "n"],
    ["web.prompt.btn.quit", "y", true],
  ],
  Collision: [
    ["web.prompt.btn.cancel", "n"],
    ["web.prompt.btn.rename", "r"],
    ["web.prompt.btn.overwrite", "o", true],
  ],
  TypeChange: [
    ["web.prompt.btn.cancel", "n"],
    ["web.prompt.btn.changeType", "y", true],
  ],
  ArrayUpgrade: [
    ["web.prompt.btn.cancel", "n"],
    ["web.prompt.btn.convertAndPaste", "y", true],
  ],
  JsoncUpgrade: [
    ["web.prompt.btn.cancel", "n"],
    ["web.prompt.btn.upgradeJsonc", "y", true],
  ],
};

// Short sheet/dialog title per prompt kind (the touch sheet header).
const PROMPT_TITLES: Record<PromptView, string> = {
  ConfirmQuit: "web.prompt.title.confirmQuit",
  Collision: "web.prompt.title.collision",
  TypeChange: "web.prompt.title.typeChange",
  ArrayUpgrade: "web.prompt.title.arrayUpgrade",
  JsoncUpgrade: "web.prompt.title.jsoncUpgrade",
};

export function promptTitle(kind: PromptView): string {
  const key = PROMPT_TITLES[kind];
  return key ? t(key) : t("web.prompt.titleFallback");
}


export function promptButtonsHTML(kind: PromptView): string {
  const btns = PROMPT_BUTTONS[kind] ?? [
    ["web.prompt.btn.no", "n"],
    ["web.prompt.btn.yes", "y", true],
  ];
  return (
    '<div class="row-btns prompt-btns">' +
    btns
      .map(
        ([labelKey, pk, primary]) =>
          `<button class="btn${primary ? " primary" : ""}" data-pk="${pk}">${t(labelKey)}</button>`,
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
