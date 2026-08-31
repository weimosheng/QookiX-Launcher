// ============================================================================
//  verify-artifacts.mjs - Fail the build EARLY if the collected release
//  artifacts are incomplete, instead of shipping a broken release.
//
//  This guards the bundle-target optimisations: whenever `bundle.targets` is
//  narrowed (e.g. dropping the MSI on Windows) there is a risk of also losing
//  the updater bundle, which would silently break in-app auto-updates.
//
//  Usage:
//    node scripts/verify-artifacts.mjs <artifacts-dir> <windows|macos|linux>
//    env: TAURI_SIGNING_PRIVATE_KEY (set => the updater bundle needs a .sig)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'
import { UPDATER_TARGETS } from './shared/updater-targets.mjs'

const [dir, platform] = process.argv.slice(2)
if (!dir || !platform) {
  throw new Error('Usage: verify-artifacts.mjs <artifacts-dir> <windows|macos|linux>')
}

// CI matrix value -> Tauri target-triple prefix used by UPDATER_TARGETS.
const TRIPLE_PREFIX = { windows: 'windows', macos: 'darwin', linux: 'linux' }
const prefix = TRIPLE_PREFIX[platform]
if (!prefix) throw new Error(`Unknown platform "${platform}" (expected windows|macos|linux)`)

// Updater bundles are derived from the SAME source of truth the manifest
// generators use, so this check can never disagree with what the updater
// actually needs.
const updaterSuffixes = UPDATER_TARGETS.filter((t) =>
  t.platforms.some((p) => p.startsWith(prefix)),
).flatMap((t) => t.suffixes)

// What a human can download and install by hand.
// NOTE: Tauri inserts the NSIS language code into MSI names
// (`QookiX.Launcher_0.4.3_x64_en-US.msi`), hence the wildcard before `.msi`.
const INSTALLERS = {
  windows: /(_x64-setup\.exe|_x64.*\.msi|_x64_portable\.zip)$/,
  macos: /\.dmg$/,
  linux: /\.(AppImage|deb|rpm)$/,
}

function fail(msg) {
  console.error(`verify-artifacts: ${msg}`)
  console.error(`  contents of ${dir}: ${fs.readdirSync(dir).join(', ') || '(empty)'}`)
  console.error(`  accepted updater suffixes: ${updaterSuffixes.join(', ')}`)
  process.exit(1)
}

if (!fs.existsSync(dir)) fail(`artifacts dir does not exist: ${dir}`)
const files = fs.readdirSync(dir)

// 1. There must be something a user can actually install.
const installers = files.filter((f) => INSTALLERS[platform].test(f))
if (installers.length === 0) {
  fail(`no installable artifact found for platform "${platform}"`)
}

// 2. createUpdaterArtifacts is enabled, so an updater bundle must exist.
const updater = files.filter((f) => updaterSuffixes.some((s) => f.endsWith(s)))
if (updater.length === 0) {
  fail(
    `no updater bundle found for platform "${platform}" — ` +
      'in-app auto-updates would break. Check bundle.targets in tauri.conf.json.',
  )
}

// 3. When signing is configured, at least one updater bundle must carry a .sig,
//    otherwise create-update-manifest-local.mjs silently skips the platform.
if (process.env.TAURI_SIGNING_PRIVATE_KEY) {
  const signed = updater.filter((f) => fs.existsSync(path.join(dir, `${f}.sig`)))
  if (signed.length === 0) {
    fail(`signing is enabled but no updater bundle has a .sig: ${updater.join(', ')}`)
  }
}

console.log(`verify-artifacts: ${platform} OK`)
console.log(`  installers: ${installers.join(', ')}`)
console.log(`  updater   : ${updater.join(', ')}`)
