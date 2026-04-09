# macOS Release Pipeline

This repo now ships a first-pass macOS release pipeline for `con` that:

- builds a native `.app` bundle without depending on `cargo-bundle`
- signs with a Developer ID Application certificate
- notarizes with `notarytool`
- staples the `.app`
- packages and signs a `.dmg`
- notarizes and staples the `.dmg`
- publishes per-architecture artifacts to GitHub Releases

## What Changed

Scripts live in [`scripts/macos`](../../scripts/macos):

- `build-app.sh` builds `con` and assembles `con.app`
- `import-certificate.sh` imports the Developer ID cert into a temporary keychain for CI
- `release.sh` runs build, sign, notarize, staple, package, and checksum generation
- `verify.sh` runs `codesign`, `spctl`, and stapler validation checks

GitHub Actions release workflow:

- [`.github/workflows/release-macos.yml`](../../.github/workflows/release-macos.yml)

## Identity Model

The website host and the bundle identifier should not use the same order.

- website: `con.nowledge.co`
- macOS bundle id base: `co.nowledge.con`

That follows reverse-DNS convention and is the long-term correct identifier layout.

Channel behavior:

- `stable`: bundle id `co.nowledge.con`, app name `con`
- `beta`: bundle id `co.nowledge.con.beta`, app name `con Beta`

That split is deliberate. If beta and stable ever need to coexist on one machine, they cannot share a bundle identifier.

## GitHub Scope Split

Use GitHub scopes this way:

- organization secrets: Apple team credentials reused by multiple apps
- repository variables: app identity and packaging defaults

This is the right long-term split because your Apple trust chain belongs to the organization, while bundle ids and app names belong to individual products.

GitHub documents organization-level secret sharing and selected-repository access here:

- https://docs.github.com/en/actions/administering-github-actions/sharing-workflows-secrets-and-runners-with-your-organization
- https://docs.github.com/en/actions/reference/security/secrets

## Required GitHub Secrets

Signing:

- `APPLE_CERTIFICATE_P12_BASE64`
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_KEYCHAIN_PASSWORD`
- `APPLE_SIGNING_IDENTITY`

Notarization, preferred:

- `APPLE_NOTARY_KEY_ID`
- `APPLE_NOTARY_ISSUER_ID`
- `APPLE_NOTARY_API_KEY_BASE64`

These are team-level credentials and can be organization secrets, restricted to the repositories that build signed apps.

Important:

- `APPLE_NOTARY_API_KEY_BASE64` must be the base64-encoded contents of the `.p8` file
- for local testing, you can skip base64 entirely and use `APPLE_NOTARY_KEY_PATH=/absolute/path/to/AuthKey_XXXXXX.p8`
- the workflow uses `APPLE_NOTARY_API_KEY_BASE64` because GitHub secrets cannot safely point to local filesystem paths

Local fallback is also supported in the scripts, but should not be your primary CI path:

- `APPLE_ID`
- `APPLE_APP_SPECIFIC_PASSWORD`
- `APPLE_TEAM_ID`

Apple's App Store Connect API key docs are here:

- https://developer.apple.com/help/app-store-connect/get-started/app-store-connect-api
- https://developer.apple.com/documentation/appstoreconnectapi/creating-api-keys-for-app-store-connect-api

Apple explicitly notes that individual App Store Connect API keys cannot be used with `notaryTool`; use a team key.

## Repository Variables

This repo does not need any GitHub repository variables for macOS releases.

The workflow builds correctly from the defaults baked into the scripts:

- `MACOS_APP_NAME=con`
- `MACOS_BUNDLE_ID_BASE=co.nowledge.con`
- `MACOS_ICON_SOURCE=assets/Con-macOS-Dark-1024x1024@1x.png`
- `MACOS_MINIMUM_SYSTEM_VERSION=10.15.7`

If you reuse these scripts in another app repo, override those values there instead of editing the workflow.

## Release Tags

Stable release:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Beta release:

```bash
git tag v0.2.0-beta.1
git push origin v0.2.0-beta.1
```

The workflow maps `*-beta.*` tags to the beta channel and marks the GitHub Release as a prerelease.

## Reusing Existing Apple Credentials

If your existing `nowledge-identities.p12` contains the same Apple team's `Developer ID Application` identity, it can be reused for `con`.

If your existing App Store Connect team API key is:

- Key ID: `KG4RA8F5A6`
- Issuer ID: `df090cec-9b81-4642-a0f0-5063ae39fb87`

then it can also be reused for notarizing `con`, provided it is a team key and still active.

What is reusable across apps:

- Developer ID Application certificate
- App Store Connect team API key
- temporary CI keychain password convention

What is app-specific and should stay per-repo:

- bundle identifier
- app name
- icon path
- updater feed URL
- release-channel defaults

## Local Dry Run

Ad-hoc signed local build:

```bash
CON_ALLOW_ADHOC_SIGNING=1 \
CON_SKIP_NOTARIZATION=1 \
./scripts/macos/release.sh
```

Signed local build with notarization:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Company (TEAMID)"
export APPLE_NOTARY_KEY_ID="XXXXXXXXXX"
export APPLE_NOTARY_ISSUER_ID="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
export APPLE_NOTARY_KEY_PATH="/absolute/path/to/AuthKey_XXXXXXXXXX.p8"

./scripts/macos/release.sh
```

## CI Output

The workflow currently builds native artifacts on:

- `macos-15` for Apple Silicon / `arm64`
- `macos-15-intel` for Intel / `x86_64`

That matches the current `con-ghostty` build behavior: it builds Ghostty for the host architecture in `build.rs`. This pipeline avoids pretending we have a universal build when we do not.

## Updater Recommendation

### What GPUI Gives Us

GPUI itself is not an updater framework.

The relevant 3pp references are:

- Zed's [`release_channel`](../../3pp/zed/crates/release_channel/src/lib.rs): useful as a pattern for app-side channel state
- Zed's [`auto_update`](../../3pp/zed/crates/auto_update/src/auto_update.rs): custom infrastructure, not a GPUI primitive
- Ghostty's Sparkle-based update implementation in [`3pp/ghostty/macos/Sources/Features/Update`](../../3pp/ghostty/macos/Sources/Features/Update)

### Recommended Direction

For `con` on macOS, use Sparkle instead of inventing a custom DMG updater inside the Rust app.

Recommended model:

1. Use GitHub Releases as the binary host.
2. Serve channel-specific Sparkle appcasts from `con.nowledge.co`.
3. Keep separate feeds:
   - `https://con.nowledge.co/appcast/stable.xml`
   - `https://con.nowledge.co/appcast/beta.xml`
4. Point each appcast entry at the corresponding GitHub Release asset URL.
5. Keep stable and beta as separate bundle identifiers.

This follows Ghostty's direction closely: separate feed URLs per channel is simpler than overloading one shared feed.

### Why Not “GitHub Releases Only” For Updating

GitHub Releases is a fine file host. It is not, by itself, a full macOS update protocol.

For a real updater you still need:

- signed update metadata
- channel selection
- version comparison policy
- install/relaunch flow
- rollback-safe behavior

Sparkle already solves those pieces correctly on macOS.

## Next Step For Auto-Update

The release pipeline is now ready for Sparkle hosting, but the app does not embed Sparkle yet.

The next implementation step should be:

1. add a small Swift/AppKit wrapper target that owns Sparkle
2. inject `SUFeedURL` and `SUPublicEDKey` into the bundled `Info.plist`
3. expose channel selection in Settings
4. publish appcasts from `con.nowledge.co`
