# MSIX: making the bundled CLI node headless
Status: Resolved (2026-09-18)

**Outcome — the no-waiver route of the addendum below is what shipped** (`5b4bf5f`): one
`<Application>` node, `windows.appExecutionAlias` carrying its own
`Executable="confy.exe" EntryPoint="Windows.FullTrustApplication"`, the `Application
Id="confycli"` node gone. So P1's `HeadlessAppBypass` waiver email was **never sent and is no
longer needed**, and P2's hidden-node branch is abandoned. `makeappx` accepts the attribute in
that position (the open question of the addendum's last line), verified on Windows 2026-09-18
together with `%LOCALAPPDATA%\Microsoft\WindowsApps\confy.exe --help` printing the TUI usage.
Of the competing hypotheses below, **none** was right: the alias did launch the GUI all along,
and the owner's "it used to work" memory was H3 in a sharper form — a winget-installed
`confy.exe` on PATH ahead of the `WindowsApps` stub. `crates/confy-tauri/msix/STORE.md`
§Known caveats carries the durable version. Everything from here down is the 2026-09-17
diagnosis as written.

Evidence settled 2026-09-17.

## Goal

Today the Store `.msix` installs **two** visible Start-menu entries: `confy` (GUI,
`confy-desktop.exe`) and `confy (CLI)` (TUI, `confy.exe`, which carries the `confy.exe` App
Execution Alias). Desired: one visible entry (the GUI), the CLI node hidden via
`uap:VisualElements AppListEntry="none"`, alias still on PATH.

## Why the visible CLI entry is not merely cosmetic — it is broken

`confy-tui` requires a file argument: `crates/confy-tui/src/cli.rs:244-246` bails with
`cli.no-file` when `args.file` is `None`. A Start-menu click passes no argv, so the entry
opens a console that prints an error and exits. The entry can never start the TUI. So the
hidden-node change is a **bug fix**, not polish — which is also the justification text for
the waiver request.

## Evidence (settled 2026-09-17, corrects this file's first revision)

The rejection is **measured**, not inferred. A real `publish-msstore` run failed with:

```
InvalidParameterValue | Package acceptance validation error: The package file
confy-desktop-windows-x86_64.msix specifies a headless app. You don't have permission to
create a headless app. Please update AppListEntry="none" in the AppxManifest file and also
ensure you have the waiver "HeadlessAppBypass" associated to this app.
```

Run: <https://github.com/superyngo/confy/actions/runs/35043429642/job/104629133832>

So the validator is **per-Application-node**: a package with one visible GUI node and one
hidden CLI node still trips the check. (`85dc72b`'s commit message did not cite this run,
which is why the claim read as unverified; the run is the evidence.) The conclusion in
`AppxManifest.xml:79-81`, `STORE.md:133-136` and `CHANGELOG.md:129-131` is correct as
written — only their provenance was missing.

## Waiver request (P1) — what to do

Primary channel, the one that is documented to have worked (NanaZip, Store-version CPython):
email **storeops@microsoft.com**. The waiver must be associated with the product **before**
the package is uploaded.

Facts to quote (from `gh variable list` + `AppxManifest.xml`):

| Field | Value |
|---|---|
| Store ID | `9PLCJGQ3C654` |
| Package/Identity/Name | `WENAN.ConfyTOMLJSONYAMLEditor` |
| Package/Identity/Publisher | `CN=2B98008C-550F-4198-A37B-C393A29FE133` |
| PublisherDisplayName | `WenanLin Studio` |
| Properties/DisplayName | `Confy — TOML/JSON/YAML Editor` |

Fallback if no reply in ~2 weeks: a **Business support** ticket at
<https://support.serviceshub.microsoft.com/supportforbusiness/create?sapId=bc9d4067-7218-61b9-1d2c-68ae591acf9d>
with category **Developer, Student and Startup Programs → Dev Center → Account Management**.
The Windows Developer Support page does *not* expose the right category (documented dead
loop, MS Q&A 2115779). `partnerops@microsoft.com` is the historically unanswered address —
do not rely on it.

## After the waiver is granted (P2–P4, not started)

- **P2** Branch, unmerged: add `AppListEntry="none"` to the `confycli` node's
  `uap:VisualElements` only. Must not regress: the `uap3` + `desktop:ExecutionAlias`
  namespace shape (the `uap5` form in Microsoft's packaging-CLI guide fails `makeappx`
  here — see the 2026-08-31 changelog entry) and the hyphen-free `Application/@Id`
  (`209fa3f`, `MakeAppx C00CE169`).
- Verification needs a **Windows machine** (unavailable on this workstation): `pack-msix.ps1`,
  self-signed sideload per `STORE.md:92-104`, then confirm (i) only one Start-menu entry,
  (ii) `confy path.toml` in a fresh terminal launches the *TUI* (the `a7d81c6` bug class),
  (iii) "Open with confy" on a `.toml` still opens the GUI.
- **P3** Merge with the next release commit; watch `publish-msstore.yml`'s validation; keep a
  revert commit ready.
- **P4** Same commit as P3: manifest comment, `STORE.md` caveat (rewrite from "must stay
  visible" to "hidden, waiver granted <date>"), `CHANGELOG.md` `[Unreleased]`, and a row in
  `docs/plan/2026-09-09-open-follow-ups.md`. Cite the failed-run URL above so the next
  reader does not re-litigate the evidence.

## 2026-09-17 addendum — the `confy` alias regression, and a route with no waiver

Owner's report: typing `confy` in a terminal used to start the **TUI**; only after the last
two Store updates does it start the **desktop app**. That contradicts the premise of
`a7d81c6`, which is worth stating plainly: that commit's message asserts "An App Execution
Alias always launches its parent Application node's Executable" and cites **no measurement**
(no error text, no observed behavior) — same provenance problem as `85dc72b`. Its sibling
`209fa3f` quotes a real `MakeAppx` code, so the distinction is visible in the history.

### Primary-source mechanism (settled)

`uap3:Extension` (Learn, `element-uap3-extension`, ms.date 2026-06-05) has an **optional
`Executable` attribute** — "The default launch executable" — plus `EntryPoint` and
`uap11:Subsystem` (`console`|`windows`). So the alias target is declarable **on the extension
itself**, independent of the `<Application>` node it sits under. The second `<Application>`
node `a7d81c6` introduced was therefore never required. (Conveyor emits exactly this shape:
`<uap3:Extension Category="windows.appExecutionAlias" Executable="bin\app.exe"
EntryPoint="Windows.FullTrustApplication">`, hydraulic-software/conveyor#190.)

### The route that needs no waiver

Keep **one** visible `<Application>` (the GUI) and point the alias at the TUI explicitly:

```xml
<uap3:Extension Category="windows.appExecutionAlias"
                Executable="confy.exe" EntryPoint="Windows.FullTrustApplication">
  <uap3:AppExecutionAlias>
    <desktop:ExecutionAlias Alias="confy.exe" />
  </uap3:AppExecutionAlias>
</uap3:Extension>
```

Result: one Start-menu entry, `confy` on PATH runs the TUI, **no hidden node, so no
`AppListEntry="none"`, so no `HeadlessAppBypass` waiver**. Optionally add
`uap11:Subsystem="console"` (+ the `uap11` namespace). Unverified: whether `makeappx`'s
schema accepts the attribute in this position — the `380b97d` lesson is that this manifest's
namespace shapes must be tested against the real SDK, on Windows.

### Competing hypotheses for "it used to work"

- **H1 alias-name resolution.** Pre-`a7d81c6` the extension had no `Executable`, and Windows
  resolved the alias by *filename* to the package's `confy.exe` → TUI, as remembered. Then
  `a7d81c6` is a fix for a non-bug, and the regression came from something else.
- **H2 stale alias registration** (fits the timing best). The alias moved from
  `Application Id="confy"` to `Id="confycli"` across an in-place Store upgrade, and the
  `%LOCALAPPDATA%\Microsoft\WindowsApps\confy.exe` stub stayed bound to the old AUMID
  (`…!confy` = the GUI). Nothing in the package is wrong; the machine's alias registration is.
  Test: uninstall and reinstall, then re-run `confy`.
- **H3 memory is of a different binary** — a `cargo build`/portable `confy.exe` on PATH rather
  than the Store alias. Cheap to rule out with `where.exe confy`.

H2 and H3 are distinguishable only on a Windows machine; evidence to collect is in the reply
of 2026-09-17 (`where.exe confy`, the WindowsApps reparse target, `Get-AppxPackage`
version, then a clean reinstall).

### Consequence for the waiver email

If the one-node route works, the waiver is unnecessary — the email can wait until the
one-node variant has been sideload-tested on Windows.
