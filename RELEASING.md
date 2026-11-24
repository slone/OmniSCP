# Releasing

1. Set the new version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`, and commit.
2. Tag it and push the tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

3. The Release workflow (`.github/workflows/release.yml`) builds the installers on macOS, Linux and Windows and attaches them to a **draft** release. It stops at once if the tag does not match the version in `tauri.conf.json`.
4. Check the draft on GitHub, write the notes, and publish it.

To build the installers without a release, run the workflow by hand (Actions → Release → Run workflow). They are then attached to the run as artifacts.

| System | Files |
| --- | --- |
| macOS | `.dmg`, one universal build for Apple Silicon and Intel |
| Windows | `.msi` and `-setup.exe` |
| Linux | `.deb`, `.rpm` and `.AppImage`, built on Ubuntu 22.04 for older distributions |

## Signing

Signing turns on by itself as soon as its secrets exist (Settings → Secrets and variables → Actions). Without them the installers are still built, unsigned, and the run shows a warning.

### macOS

This needs a membership in the Apple Developer Program.

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | The "Developer ID Application" certificate exported from Keychain Access as a `.p12`, in base64: `base64 -i cert.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | The password chosen when exporting it |
| `APPLE_SIGNING_IDENTITY` | Its full name, such as `Developer ID Application: Guillaume Chantraine (TEAMID)` |
| `APPLE_ID` | The Apple account email, for notarization |
| `APPLE_PASSWORD` | An app-specific password for that account (appleid.apple.com → Sign-In and Security) |
| `APPLE_TEAM_ID` | The team ID, shown in the developer account |

Without the last three, the app is signed but not notarized, and Gatekeeper still warns. Without any of them, the app is only signed ad hoc: macOS refuses to open it at first, and the user has to allow it under System Settings → Privacy & Security.

### Windows

| Secret | Value |
| --- | --- |
| `WINDOWS_CERTIFICATE` | A code signing certificate as a `.pfx`, in base64: `[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Its password |

The certificate is imported for the run only and used with a DigiCert timestamp. An unsigned installer runs, but SmartScreen shows a warning first.

A certificate kept on a hardware key, or a cloud service such as Azure Trusted Signing, does not fit in a secret. For those, the setup step in the workflow gives way to Tauri's `bundle.windows.signCommand`.

### Linux

The packages are not signed. Distributions check signatures on their own repositories, not on downloaded `.deb` or `.rpm` files.

## Icon

The icon is drawn in `src-tauri/icons/app-icon.svg`. After changing it, regenerate every size:

```bash
npm run tauri icon src-tauri/icons/app-icon.svg
```
