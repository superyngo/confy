# Microsoft Store submission notes

The release workflow produces an **unsigned** `confy-desktop-windows-x86_64.msix`
built by `pack-msix.ps1` from `AppxManifest.xml`. Unsigned is intentional: the
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

5. Create a GitHub **Environment** named `msstore-publish` with a required
   reviewer (Settings → Environments). The `msstore` job in `release.yml` runs
   under this environment, so every tagged release pauses for manual approval
   before it reaches the Store — CI builds and stages the submission, a human
   clicks "Approve" to actually publish.

## Per-release submission

Automatic: the `msstore` job in `.github/workflows/release.yml` runs after
`desktop` + `release` on every `v*.*.*` tag, gated behind the `msstore-publish`
environment approval. Once approved it downloads the `x86_64-pc-windows-msvc`
`.msix`, configures the Microsoft Store Developer CLI (`msstore reconfigure`)
with the secrets above, and runs `msstore publish` — which creates a new
submission, uploads the package (`x.y.z.0` derived from the git tag, same as
the identity manifest), and commits it. The Store then validates and
publishes it same as any Partner Center submission (review time varies).

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
