# Microsoft Store submission notes

The release workflow produces an **unsigned** `confy-desktop-windows-x86_64.msix`
built by `pack-msix.ps1` from `AppxManifest.xml`. The package also contains the
TUI binary as `confy.exe`, exposed on PATH after install via the
`windows.appExecutionAlias` in the manifest (resolves through
`%LOCALAPPDATA%\Microsoft\WindowsApps`). Unsigned is intentional: the
Store re-signs every submission with its own certificate, and a package signed
with a non-Store cert is rejected.

## One-time setup (Partner Center)

1. Register a developer account at <https://partner.microsoft.com/dashboard>
   (individual, one-time ~USD $19).
2. Create the app / reserve the name **confy** (or a fallback if taken).
3. Under *Product management → Product identity*, copy the three values and set
   them as GitHub **repository variables** (Settings → Secrets and variables →
   Actions → Variables) so CI bakes them into the manifest:

   | Partner Center field                | GitHub variable          |
   |-------------------------------------|--------------------------|
   | `Package/Identity/Name`             | `MSIX_IDENTITY_NAME`     |
   | `Package/Identity/Publisher`        | `MSIX_PUBLISHER`         |
   | `Package/Properties/PublisherDisplayName` | `MSIX_PUBLISHER_DISPLAY` |

   Until these are set, CI uses placeholders — fine for sideload testing, but a
   Store upload will fail identity validation.

4. Register a Microsoft Entra ID application and, from Partner Center's
   *Account settings → User management → Microsoft Entra applications*, assign
   it the **Manager** role. Collect the Submission API credentials and set them
   as GitHub **repository secrets**:

   | Value                              | GitHub secret                    |
   |-------------------------------------|-----------------------------------|
   | Tenant ID                          | `MSIX_SUBMISSION_TENANT_ID`       |
   | Client (application) ID            | `MSIX_SUBMISSION_CLIENT_ID`       |
   | Client secret                      | `MSIX_SUBMISSION_CLIENT_SECRET`   |
   | Seller ID (Account settings → Developer settings) | `MSIX_SUBMISSION_SELLER_ID` |
   | Store product ID (App identity → "Store ID") | `MSIX_SUBMISSION_APP_ID`   |

5. Create GitHub **Environments** named `publish-gate-msstore` and
   `publish-gate-vscode`, each with a required reviewer (Settings →
   Environments). `publish-gate.yml` runs one job per store, each under its
   own environment: it fires once `release.yml`'s Release job succeeds on a
   `v*.*.*` tag and pauses both jobs for manual approval — reviewable
   independently in the same "Review pending deployments" screen — then
   dispatches the corresponding `publish-*.yml` workflow.
6. In Partner Center's *Store listings* page, set **Privacy policy URL** to
   <https://confy.turkeyang.net/privacy> (`web/privacy.html` / `PRIVACY.md`,
   kept in sync manually). Not automatable via the Submission API used
   above — a one-time manual edit in the dashboard. If the listing already
   has a different URL from an earlier submission, update it there too.
7. Store listing text (Description/ReleaseNotes, all locales) and screenshot
   references are edited by hand in Partner Center per submission — not
   CI-managed. After editing, export the submission's *Listings* page
   (Partner Center's "Export listings" button) and archive the CSV under
   `crates/confy-tauri/msix/listings/`, named `<tag>-listingData-<app
   id>-<submission id>.csv` (the Partner-Center-generated filename already
   carries the app/submission IDs — just prefix the version tag). This is a
   point-in-time record for diffing future edits and recovering copy if a
   submission is ever discarded; it is never read by CI or `pack-msix.ps1`.

## Per-release submission

Automatic, in two stages:

1. `release.yml` builds the `.msix` (via `pack-msix.ps1`) on every `v*.*.*`
   tag and publishes the GitHub Release.
2. Once that succeeds, `publish-gate.yml` (`workflow_run` on `release.yml`
   completing) pauses its `msstore` job for approval in the
   `publish-gate-msstore` environment, then dispatches `publish-msstore.yml`
   with the tag + source run ID.
   `publish-msstore.yml` downloads the `x86_64-pc-windows-msvc` `.msix` from
   that run, configures the Microsoft Store Developer CLI (`msstore
   reconfigure`) with the secrets above, and runs `msstore publish` — which
   creates a new submission, uploads the package (`x.y.z.0` derived from the
   git tag, same as the identity manifest), and commits it. The Store then
   validates and publishes it same as any Partner Center submission (review
   time varies).

Runs on `windows-latest`, not `ubuntu-latest`: the msstore CLI's Linux
credential store needs `libsecret` + a D-Bus Secret Service daemon that
headless Ubuntu runners don't have; Windows DPAPI works headless with no
extra setup.

Note the CLI's "Only needed if the project has not been initialized before
with the `init` command" caveat on `--appId` doesn't apply here: this repo
never ran `msstore init` (no local `msstore.json` project file), so `-id` is
passed explicitly every run.

## Sideload testing (before Store identity exists)

On a Windows machine, sign with a self-signed cert whose subject equals the
manifest `Publisher` placeholder, then trust it:

```powershell
New-SelfSignedCertificate -Type Custom -Subject "CN=00000000-0000-0000-0000-000000000000" `
  -KeyUsage DigitalSignature -FriendlyName confy-dev -CertStoreLocation Cert:\CurrentUser\My `
  -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
# export it, import into LocalMachine\TrustedPeople, then:
signtool sign /fd SHA256 /a confy-desktop-windows-x86_64.msix
Add-AppxPackage confy-desktop-windows-x86_64.msix
```

## Known caveats

- **WebView2 runtime**: the MSIX cannot bundle the WebView2 bootstrapper.
  Windows 11 ships it inbox and Windows 10 receives it via Edge updates, so in
  practice it is nearly always present; on a machine without it the app shows a
  WebView2 error at launch.
- x64 only for now; add an arm64 manifest/`ProcessorArchitecture` + build leg
  (and an `.msixbundle`) if Windows-on-ARM demand appears.
- **Never add `--noCommit`.** Tried in v0.31.0/v0.31.1 and reverted: the
  package zip is only uploaded to an Azure blob by the upload step, and the
  Store ingests it *only* when the submission is committed. Without the
  commit the submission sits at `PendingCommit` and Partner Center still
  shows the packages cloned from the last published submission (v0.30.1.0,
  "Unchanged") — and submitting that draft by hand in Partner Center
  re-publishes the *old* package. Related: the Submission API docs warn that
  a submission created via the API must only be changed via the API; editing
  it in Partner Center can leave it uncommittable, requiring a discard.
  <https://learn.microsoft.com/windows/uwp/monetize/manage-app-submissions>
