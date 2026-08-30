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
//    env: TAURI_SIGNING_PRIVATE_KEY (set => every updater bundle needs a .sig)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const [dir, platform] = process.argv.slice(2)
if (!dir || !platform) {
  throw new Error('Usage: verify-artifacts.mjs <artifacts-dir> <windows|macos|linux>')
}

// `updater`  = the bundle the Tauri updater downloads (must exist because
//              bundle.createUpdaterArtifacts is enabled).
// `installers` = anything a human can download and install by hand.
const SPEC = {
  windows: {
    updater: /(\.nsis\.zip|_x64-setup\.exe)$/,
    installers: /(_x64-setup\.exe|_x64\.msi|_x64_portable\.zip)$/,
  },
  macos: {
    updater: /\.app\.tar\.gz$/,
    installers: /\.dmg$/,
  },
  linux: {
    updater: /\.AppImage\.tar\.gz$/,
    installers: /\.(AppImage|deb|rpm)$/,
  },
}

const spec = SPEC[platform]
if (!spec) throw new Error(`Unknown platform "${platform}" (expected windows|macos|linux)`)

function fail(msg) {
  console.error(`verify-artifacts: ${msg}`)
  console.error(`  contents of ${dir}: ${fs.readdirSync(dir).join(', ') || '(empty)'}`)
  process.exit(1)
}

if (!fs.existsSync(dir)) fail(`artifacts dir does not exist: ${dir}`)
const files = fs.readdirSync(dir)

// 1. There must be something a user can actually install.
const installers = files.filter((f) => spec.installers.test(f))
if (installers.length === 0) {
  fail(`no installable artifact found for platform "${platform}"`)
}

// 2. createUpdaterArtifacts is enabled, so an updater bundle must exist.
const updater = files.filter((f) => spec.updater.test(f))
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
