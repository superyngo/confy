// Shared Help/About/KIND-legend content for the Help overlay (desktop
// `web/ui.ts`) and the touch edit UI. Row descriptions are sourced from the
// `help.row.*`/`help.section.*` i18n keys shared with the TUI's
// `keys::help_text` (crates/confy-tui/src/tui/keys.rs) so the same binding
// reads identically on both surfaces; `web.help.*` keys are Web-only
// (Pointer section, VS Code variant rows/note). See
// docs/reference/KEYMAP.md §Help overlay parity.
import { escapeHtml } from "./escape.js";
import { t, tArgs, getLang } from "./i18n.js";

interface HelpRow {
  keys: string;
  descKey: string;
}
interface HelpSection {
  titleKey: string;
  rows: HelpRow[];
}

const NAV_SECTION: HelpSection = {
  titleKey: "help.section.nav",
  rows: [
    { keys: "j/k/↑/↓", descKey: "help.row.move_cursor" },
    { keys: "Home/End/g/G", descKey: "help.row.first_last_row" },
    { keys: "PgUp/PgDn", descKey: "help.row.page" },
    { keys: "1/2", descKey: "help.row.expand_collapse_level" },
    { keys: "0/9", descKey: "help.row.collapse_expand_all" },
    { keys: "Space", descKey: "help.row.space_toggle" },
    { keys: "Enter/i", descKey: "help.row.detail" },
  ],
};

const SELECT_SECTION: HelpSection = {
  titleKey: "help.section.select",
  rows: [
    { keys: "s", descKey: "help.row.toggle_select" },
    { keys: "Shift+↑/↓", descKey: "help.row.range_select" },
    { keys: "/", descKey: "help.row.fuzzy_filter" },
    { keys: "f", descKey: "help.row.type_filter" },
    { keys: "Esc", descKey: "help.row.clear_esc" },
  ],
};

function editSection(docFormat: string, variant: "web" | "vscode"): HelpSection {
  const fmt = docFormat.toLowerCase();
  return {
    titleKey: "help.section.edit",
    rows: [
      { keys: "e", descKey: "help.row.edit" },
      { keys: "E", descKey: "help.row.force_editor" },
      { keys: "F2", descKey: "help.row.rename" },
      { keys: "a", descKey: "help.row.add_node" },
      { keys: "d/Delete", descKey: "help.row.delete" },
      { keys: "c/x/v", descKey: "help.row.copy_cut_paste" },
      { keys: "←/→/+/-", descKey: "help.row.nudge" },
      { keys: "r", descKey: `help.row.remark.${fmt}` },
      { keys: "K", descKey: `help.row.kind_switch.${fmt}` },
      {
        keys: "z/y",
        descKey: variant === "vscode" ? "web.help.row.undo_redo_vscode" : "help.row.undo_redo",
      },
      { keys: "C", descKey: "help.row.convert" },
    ],
  };
}

function fileSection(variant: "web" | "vscode"): HelpSection {
  const rows: HelpRow[] = [
    { keys: "Ctrl+s", descKey: variant === "vscode" ? "web.help.row.save_vscode" : "help.row.save" },
  ];
  if (variant === "web") rows.push({ keys: "Ctrl+o", descKey: "help.row.open" });
  if (variant === "vscode") rows.push({ keys: "⇧⌘S / Ctrl+⇧S", descKey: "web.help.row.save_as_convert" });
  rows.push({ keys: "m", descKey: "help.row.action_menu" });
  rows.push({ keys: "?", descKey: "help.row.help" });
  if (variant === "web") rows.push({ keys: "q", descKey: "help.row.quit" });
  return { titleKey: "help.section.file", rows };
}

const POINTER_SECTION: HelpSection = {
  titleKey: "web.help.section.pointer",
  rows: [
    { keys: "click", descKey: "web.help.row.pointer_select" },
    { keys: "Shift+click", descKey: "web.help.row.pointer_range" },
    { keys: "⌘click", descKey: "web.help.row.pointer_multi" },
    { keys: "drag", descKey: "web.help.row.pointer_drag" },
    { keys: "right-click", descKey: "help.row.action_menu" },
  ],
};

function sectionHTML(s: HelpSection): string {
  const title = `<div class="help-sect-title">${escapeHtml(t(s.titleKey))}</div>`;
  const rows = s.rows
    .map(
      (r) =>
        `<div class="help-key">${escapeHtml(r.keys)}</div>` +
        `<div class="help-desc">${escapeHtml(t(r.descKey))}</div>`,
    )
    .join("");
  return title + rows;
}

function sectionsHTML(sections: HelpSection[]): string {
  return `<div class="help-grid">${sections.map(sectionHTML).join("")}</div>`;
}

// One Help line → HTML: the Kind-legend glossary alternates key/description
// (some lines carry two pairs), so wrap every even content segment in a
// .help-key span. Splitting on runs of 2+ spaces with a capture keeps the
// separators, so the alignment survives untouched. Lines without a 2+-space
// split (headings, prose) stay plain; `──` rules get their own .help-sect
// span. Kept distinct from the row-based keymap grid above because the Web
// Kind legend uses its own `label·notation` vocabulary, not the TUI's
// bracket-tag notation (see docs/reference/KEYMAP.md §Help overlay parity).
function helpLineHTML(line: string): string {
  if (line.startsWith("──"))
    return `<span class="help-sect">${escapeHtml(line)}</span>`;
  const parts = line.split(/(\s{2,})/);
  const contentCount = parts.filter((p, i) => i % 2 === 0 && p !== "").length;
  if (contentCount < 2) return escapeHtml(line);
  let content = 0;
  return parts
    .map((p, i) => {
      if (i % 2 === 1 || p === "") return escapeHtml(p);
      return content++ % 2 === 0
        ? `<span class="help-key">${escapeHtml(p)}</span>`
        : escapeHtml(p);
    })
    .join("");
}

function legendHTML(docFormat: string): string {
  const legend = t(`web.help.legend.${docFormat.toLowerCase()}`);
  return `<div class="help-legend">${legend.split("\n").map(helpLineHTML).join("\n")}</div>`;
}

function vscodeNoteHTML(): string {
  return `<div class="help-note">${escapeHtml(t("web.help.note.vscode"))}</div>`;
}

// Shared Help/About body composition, used by both the desktop overlay
// (`web/ui.ts`) and the touch sheet (`web/touch/app.ts`). Returns HTML ready
// to drop inside the `.help-body` container — the caller must NOT escape it
// again.
//
// `aboutText` is the core-catalog body (`ConfySession.about_text()`, mirrors
// `crates/confy-core/src/session/state.rs::about_text`). One host-owned line
// is appended, mirroring the TUI's `tui.about.language` disclosure: the
// active language code.
export function helpBodyHTML(
  tab: "Help" | "About",
  docFormat: string,
  aboutText: string,
  variant: "web" | "vscode" = "web",
): string {
  if (tab === "About") {
    const body =
      aboutText.replace(/\n+$/, "") + "\n\n" + tArgs("web.about.language", [getLang()]);
    const escaped = escapeHtml(body).replace(
      /(https:\/\/\S+)/g,
      '<a href="$1" target="_blank" rel="noopener noreferrer">$1</a>',
    );
    return `<div class="help-about">${escaped}</div>`;
  }
  let out = sectionsHTML([NAV_SECTION, SELECT_SECTION, editSection(docFormat, variant), fileSection(variant)]);
  if (variant === "vscode") out += vscodeNoteHTML();
  out += sectionsHTML([POINTER_SECTION]);
  out += legendHTML(docFormat);
  return out;
}
