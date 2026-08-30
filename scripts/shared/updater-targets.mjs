// ============================================================================
//  updater-targets.mjs - SINGLE source of truth for "which artifact is the
//  Tauri updater bundle on this platform".
//
//  Consumed by:
//    - create-update-manifest-local.mjs (generates latest.json from files)
//    - fix-update-manifest.mjs          (regenerates it from published assets)
//    - verify-artifacts.mjs             (fails the build if none was produced)
//
//  Keeping this in ONE place prevents the three from drifting apart, which
//  previously cost a failed release: the verifier demanded a
//  `*.AppImage.tar.gz` on Linux that Tauri never emits.
//
//  `suffixes` is ordered by PREFERENCE: the first suffix that matches wins.
// ============================================================================

// NOTE on Linux: `bundle.createUpdaterArtifacts` does NOT produce an
// `*.AppImage.tar.gz` — Tauri 2 signs and ships the AppImage binary ITSELF as
// the updater payload. Verified against the published assets of v0.3.8, v0.4.2
// and v0.4.3: each contains `QookiX.Launcher_<ver>_amd64.AppImage` + its
// `.sig`, and no `.AppImage.tar.gz`. The `.tar.gz` form is kept as a fallback
// only, in case a future Tauri release changes this.
//
// NOTE on Windows: likewise no `.nsis.zip` is produced; the updater payload is
// the `_x64-setup.exe` installer itself. Kept as a fallback for the same reason.
export const UPDATER_TARGETS = [
  { platforms: ['windows-x86_64'], suffixes: ['_x64-setup.exe', '.nsis.zip'] },
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], suffixes: ['.app.tar.gz'] },
  { platforms: ['linux-x86_64'], suffixes: ['_amd64.AppImage', '_amd64.AppImage.tar.gz'] },
  { platforms: ['linux-aarch64'], suffixes: ['_aarch64.AppImage', '_aarch64.AppImage.tar.gz'] },
]
