// Desktop (Tauri) native system menu bar — File/Edit/View/Help mapped onto
// existing app functions. A no-op on the pure web build (`isTauri()` guard).
// Menu logic lives entirely in JS: i18n, lang preference, recent files, and
// every `Intent` are already here, so there is no Rust-side menu code.
//
// Accelerator policy (see the plan): node ops (`c`/`x`/`v`/`z`/`y`) get NO
// menu accelerator — binding CmdOrCtrl+C/X/V/Z/Y would steal those keys from
// every text input (inline edit, panel fields, search box). The plain-key
// hint is appended to the label instead; actual handling stays in ui.ts's
// `onKey`. Zoom items get no accelerator either — `zoomHotkeysEnabled` in
// tauri.conf.json already owns Cmd+/−/0.
import { isTauri } from "./fs.js";
import { availableLangs, getLang, LANG_DISPLAY_NAMES, t, type Lang } from "./i18n.js";
import type { Intent } from "./types.js";

// ---- ambient types for window.__TAURI__.menu / .webview ----
// `withGlobalTauri: true` (tauri.conf.json) puts the full JS API on
// `window.__TAURI__`. No `@tauri-apps/api` dependency — minimal ambient
// types, following the `fs.ts` TauriCore pattern.
interface MenuElement {
  id?: string;
}
interface MenuItemOptions {
  id?: string;
  text: string;
  accelerator?: string;
  enabled?: boolean;
  action?: () => void;
}
interface CheckMenuItemOptions {
  id?: string;
  text: string;
  checked: boolean;
  action?: () => void;
}
type PredefinedItemKind =
  | "Separator"
  | "Copy"
  | "Cut"
  | "Paste"
  | "SelectAll"
  | "Undo"
  | "Redo"
  | "Quit"
  | "Hide"
  | "HideOthers"
  | "ShowAll"
  | "CloseWindow";
interface PredefinedMenuItemOptions {
  // Every other kind is a plain unit variant on the Rust side (bare string);
  // `About` is a newtype variant carrying `Option<AboutMetadata>` and MUST be
  // sent as `{ About: null }` — a bare `"About"` string fails to deserialize
  // ("invalid type: unit variant, expected newtype variant").
  item: PredefinedItemKind | { About: null };
  text?: string;
}
interface SubmenuOptions {
  id?: string;
  text: string;
  items: MenuElement[];
}
interface MenuHandle extends MenuElement {
  setAsAppMenu(): Promise<void>;
}
interface TauriMenuNs {
  Menu: { new: (opts: { id?: string; items: MenuElement[] }) => Promise<MenuHandle> };
  Submenu: { new: (opts: SubmenuOptions) => Promise<MenuElement> };
  MenuItem: { new: (opts: MenuItemOptions) => Promise<MenuElement> };
  CheckMenuItem: { new: (opts: CheckMenuItemOptions) => Promise<MenuElement> };
  PredefinedMenuItem: { new: (opts: PredefinedMenuItemOptions) => Promise<MenuElement> };
}
function tauriMenuNs(): TauriMenuNs | null {
  const w = window as unknown as { __TAURI__?: { menu?: TauriMenuNs } };
  return w.__TAURI__?.menu ?? null;
}

interface CurrentWebview {
  setZoom(factor: number): Promise<void>;
}
function currentWebview(): CurrentWebview | null {
  const w = window as unknown as {
    __TAURI__?: { webview?: { getCurrentWebview: () => CurrentWebview } };
  };
  return w.__TAURI__?.webview?.getCurrentWebview() ?? null;
}

function isMac(): boolean {
  const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
  const platform = nav.userAgentData?.platform ?? navigator.platform ?? "";
  return /mac/i.test(platform);
}

// ---- recent files (Tauri only — paths are only meaningful there) ----
export interface RecentEntry {
  path: string;
  name: string;
}
const RECENT_KEY = "confy-recent";
const RECENT_CAP = 8;

function readRecent(): RecentEntry[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? (parsed as RecentEntry[]) : [];
  } catch {
    return [];
  }
}
function writeRecent(list: RecentEntry[]): void {
  localStorage.setItem(RECENT_KEY, JSON.stringify(list));
}
export function recentList(): RecentEntry[] {
  return readRecent();
}
export function recentAdd(path: string, name: string): void {
  const list = readRecent().filter((e) => e.path !== path);
  list.unshift({ path, name });
  writeRecent(list.slice(0, RECENT_CAP));
}
export function recentRemove(path: string): void {
  writeRecent(readRecent().filter((e) => e.path !== path));
}
export function recentClear(): void {
  writeRecent([]);
}

// ---- zoom (module-local; Reset/In/Out only — `zoomHotkeysEnabled` owns the
// Cmd+/−/0 hotkey path, deliberately not synced, see the plan) ----
let zoom = 1;
async function applyZoom(next: number): Promise<void> {
  zoom = Math.min(3, Math.max(0.3, next));
  await currentWebview()?.setZoom(zoom);
}
function zoomIn(): Promise<void> {
  return applyZoom(zoom + 0.1);
}
function zoomOut(): Promise<void> {
  return applyZoom(zoom - 0.1);
}
function zoomReset(): Promise<void> {
  return applyZoom(1);
}

// ---- host wiring ----
export interface MenuDeps {
  doNew: () => void | Promise<void>;
  doOpen: () => void | Promise<void>;
  doSave: () => void | Promise<void>;
  send: (intent: Intent) => void;
  toggleTheme: () => void;
  chooseLang: (lang: Lang) => void;
  openRecentPath: (path: string) => void | Promise<void>;
  err: (msg: string) => void;
}

let currentDeps: MenuDeps | null = null;
let building = false;
// The constructed Menu is otherwise a local variable inside buildAndSet() —
// once that function returns, nothing in JS references it, so V8 is free to
// garbage-collect it at any later point (a big allocation spike, e.g. opening
// a file, is a classic GC trigger). Its action handlers ride on Tauri
// resources tied to the JS wrapper's lifetime; GC'ing it tears down those
// resources (and their click channels) even though the native OS menu bar
// keeps showing the — now unresponsive — items. Keeping the root referenced
// here for the module's lifetime prevents that (children stay alive via the
// Rust-side tree the root owns).
let installedMenu: MenuHandle | null = null;

// Every menu handler is routed through this: menu-triggered events are
// unhandled-rejection territory otherwise (Tauri swallows handler errors).
function menuAction(fn: () => void | Promise<void>): () => void {
  return () => {
    Promise.resolve()
      .then(fn)
      .catch((e: unknown) => {
        console.error("[confy menu]", e);
        currentDeps?.err(e instanceof Error ? e.message : String(e));
      });
  };
}

/** Build the native menu and install it. No-op on the pure web build. */
export async function setupAppMenu(deps: MenuDeps): Promise<void> {
  currentDeps = deps;
  if (!isTauri()) return;
  try {
    await buildAndSet();
  } catch (e) {
    console.error("[confy menu]", e);
    deps.err(e instanceof Error ? e.message : String(e));
  }
}

/** Rebuild + reinstall the menu (language switch, recent-files mutation). */
export async function rebuildMenu(): Promise<void> {
  if (!currentDeps || !isTauri()) return;
  try {
    await buildAndSet();
  } catch (e) {
    console.error("[confy menu]", e);
    currentDeps.err(e instanceof Error ? e.message : String(e));
  }
}

async function buildAndSet(): Promise<void> {
  if (building) return;
  const deps = currentDeps;
  if (!deps) return; // setupAppMenu was never called — nothing to build against
  const menuNs = tauriMenuNs();
  if (!menuNs) {
    // isTauri() (window.__TAURI__.core) was true but window.__TAURI__.menu is
    // missing — surface this instead of failing silently (see WEBUI.md
    // §Desktop menu (Tauri) for the diagnosis that led here).
    throw new Error("window.__TAURI__.menu is unavailable — native menu bar disabled");
  }
  building = true;
  try {
    const { Menu, Submenu, MenuItem, CheckMenuItem, PredefinedMenuItem } = menuNs;
    const mac = isMac();
    const submenus: MenuElement[] = [];

    if (mac) {
      submenus.push(
        await Submenu.new({
          text: "confy",
          items: [
            // Custom (not Predefined) so "About confy" opens our in-app About
            // overlay instead of macOS's native About panel — same handler as
            // the Help menu's About item below.
            await MenuItem.new({
              text: `${t("web.help.tab.about")} confy`,
              action: menuAction(() => {
                deps.send("EnterHelp");
                deps.send("ToggleHelpTab");
              }),
            }),
            await PredefinedMenuItem.new({ item: "Separator" }),
            await PredefinedMenuItem.new({ item: "Hide" }),
            await PredefinedMenuItem.new({ item: "HideOthers" }),
            await PredefinedMenuItem.new({ item: "ShowAll" }),
            await PredefinedMenuItem.new({ item: "Separator" }),
            await PredefinedMenuItem.new({ item: "Quit" }),
          ],
        }),
      );
    }

    // File > Open Recent
    const recents = recentList();
    const recentItems: MenuElement[] = [];
    for (const r of recents) {
      recentItems.push(
        await MenuItem.new({ text: r.name, action: menuAction(() => deps.openRecentPath(r.path)) }),
      );
    }
    if (recents.length > 0) {
      recentItems.push(await PredefinedMenuItem.new({ item: "Separator" }));
    }
    recentItems.push(
      await MenuItem.new({
        text: t("web.menu.clearRecent"),
        enabled: recents.length > 0,
        action: menuAction(() => {
          recentClear();
          return rebuildMenu();
        }),
      }),
    );
    const openRecentMenu = await Submenu.new({ text: t("web.menu.openRecent"), items: recentItems });

    const fileItems: MenuElement[] = [
      await MenuItem.new({ text: t("web.menu.new"), accelerator: "CmdOrCtrl+N", action: menuAction(deps.doNew) }),
      await MenuItem.new({ text: t("web.menu.open"), accelerator: "CmdOrCtrl+O", action: menuAction(deps.doOpen) }),
      openRecentMenu,
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({ text: t("web.menu.save"), accelerator: "CmdOrCtrl+S", action: menuAction(deps.doSave) }),
    ];
    if (!mac) {
      // No app submenu on Windows — Quit lives at the bottom of File instead.
      fileItems.push(await PredefinedMenuItem.new({ item: "Separator" }));
      fileItems.push(await PredefinedMenuItem.new({ item: "Quit" }));
    }
    submenus.push(await Submenu.new({ text: t("web.menu.file"), items: fileItems }));

    // Edit: node ops only, routed through Session Intents — no native
    // Predefined items. Real text-field editing (panel inputs, search box)
    // already gets native OS copy/cut/paste/undo/redo/select-all directly
    // from the browser/webview regardless of menu contents (those inputs
    // stop propagation to the tree key handler); a Predefined item here
    // would just be a second, differently-behaved route to the same keys
    // (on macOS it targets the focused-responder text action, not the tree,
    // and on Windows it happened to coincide with the plain-key case below
    // only because the unmodified keystroke leaked through — see the plan).
    // Node ops below get NO accelerator so CmdOrCtrl+C/X/V/Z/Y never steal
    // those keys from a focused text input.
    submenus.push(
      await Submenu.new({
        text: t("web.menu.edit"),
        items: [
          await MenuItem.new({ text: `${t("web.menu.undo")} (z)`, action: menuAction(() => deps.send("Undo")) }),
          await MenuItem.new({ text: `${t("web.menu.redo")} (y)`, action: menuAction(() => deps.send("Redo")) }),
          await MenuItem.new({
            text: `${t("web.menu.copyNode")} (c)`,
            action: menuAction(() => deps.send("CopySelected")),
          }),
          await MenuItem.new({
            text: `${t("web.menu.cutNode")} (x)`,
            action: menuAction(() => deps.send("CutSelected")),
          }),
          await MenuItem.new({
            text: `${t("web.menu.pasteNode")} (v)`,
            action: menuAction(() => deps.send("Paste")),
          }),
        ],
      }),
    );

    // View > Language
    const langItems: MenuElement[] = [];
    for (const lang of availableLangs()) {
      langItems.push(
        await CheckMenuItem.new({
          text: LANG_DISPLAY_NAMES[lang],
          checked: getLang() === lang,
          action: menuAction(() => {
            deps.chooseLang(lang);
          }),
        }),
      );
    }
    const langMenu = await Submenu.new({ text: t("web.menu.language"), items: langItems });

    submenus.push(
      await Submenu.new({
        text: t("web.menu.view"),
        items: [
          await MenuItem.new({ text: t("web.menu.toggleTheme"), action: menuAction(deps.toggleTheme) }),
          await PredefinedMenuItem.new({ item: "Separator" }),
          await MenuItem.new({ text: t("web.menu.zoomIn"), action: menuAction(zoomIn) }),
          await MenuItem.new({ text: t("web.menu.zoomOut"), action: menuAction(zoomOut) }),
          await MenuItem.new({ text: t("web.menu.zoomReset"), action: menuAction(zoomReset) }),
          await PredefinedMenuItem.new({ item: "Separator" }),
          langMenu,
        ],
      }),
    );

    // Help: both items open the in-app overlay (EnterHelp resets to the Help
    // tab; About additionally flips it — see session.rs enter_help/toggle_help_tab).
    submenus.push(
      await Submenu.new({
        text: t("web.menu.help"),
        items: [
          await MenuItem.new({ text: t("web.help.tab.help"), action: menuAction(() => deps.send("EnterHelp")) }),
          await MenuItem.new({
            text: t("web.help.tab.about"),
            action: menuAction(() => {
              deps.send("EnterHelp");
              deps.send("ToggleHelpTab");
            }),
          }),
        ],
      }),
    );

    const menu = await Menu.new({ items: submenus });
    await menu.setAsAppMenu();
    installedMenu = menu; // keep alive — see the comment on the module-level declaration
  } finally {
    building = false;
  }
}
