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

## Per-release submission

1. Download `confy-desktop-windows-x86_64.msix` from the GitHub release.
2. Partner Center → the app → new submission → upload the `.msix`.
3. The Store validates the manifest (identity must match step 3 above exactly;
   version must be strictly greater than the previous submission — the workflow
   derives `x.y.z.0` from the git tag automatically).

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
