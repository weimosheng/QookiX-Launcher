#!/usr/bin/env node
// Wrapper around the tauri CLI.
//
// On Windows it:
//   1. builds the `installer-ui` crate (the WebView2-based modern installer
//      frontend) when needed,
//   2. sets the `QOOKIX_INSTALLER_UI` environment variable to the built
//      `QookiXInstallerUI.exe` so the NSIS bundler embeds it into the
//      installer (see `src-tauri/nsis/installer.nsi`),
//   3. forwards every argument to the real `tauri` CLI.
//
// When the variable is unset the NSIS template falls back to the classic
// (dark-styled) wizard, so building still works.
import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const projectRoot = path.resolve(__dirname, '..')
const crateDir = path.join(projectRoot, 'src-tauri', 'installer-ui')
const uiExe = path.join(crateDir, 'target', 'release', 'QookiXInstallerUI.exe')
const args = process.argv.slice(2)
const isBuild = args.includes('build')
const signScript = path.join(projectRoot, 'scripts', 'sign-windows.ps1')

// Code signing is enabled only when a certificate is configured. Without it
// every check below is skipped, so the normal dev/build flow is unaffected.
function shouldSign() {
  return process.platform === 'win32' && Boolean(process.env.QOOKIX_CERT_PFX)
}

// Sign a list of files via scripts/sign-windows.ps1 (no-op unless a cert is set).
function signFiles(files) {
  if (!shouldSign()) return
  const real = files.filter((f) => f && existsSync(f))
  if (real.length === 0) return
  const r = spawnSync(
    'powershell',
    ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', signScript, '-Files', ...real],
    { stdio: 'inherit', env: process.env },
  )
  if (r.status !== 0) process.exit(r.status ?? 1)
}

if (process.platform === 'win32' && (isBuild || !existsSync(uiExe))) {
  const build = spawnSync(
    'cargo',
    ['build', '--release', '--manifest-path', path.join(crateDir, 'Cargo.toml')],
    { stdio: 'inherit' },
  )
  if (build.status !== 0) process.exit(build.status ?? 1)
  // Sign the installer UI BEFORE it is embedded into the NSIS installer, so the
  // copy extracted to $PLUGINSDIR at install time is already signed (unsigned
  // extracted-and-executed exes are a common antivirus false-positive trigger).
  signFiles([uiExe])
}

const env = { ...process.env }
if (process.platform === 'win32' && existsSync(uiExe)) {
  env.QOOKIX_INSTALLER_UI = uiExe
} else {
  delete env.QOOKIX_INSTALLER_UI
}

const result = spawnSync('tauri', args, {
  stdio: 'inherit',
  env,
  shell: process.platform === 'win32',
})

// After a successful `tauri build`, sign the main binary and the NSIS installer.
if (result.status === 0 && isBuild && process.platform === 'win32') {
  const releaseDir = path.join(projectRoot, 'src-tauri', 'target', 'release')
  const mainExe = path.join(releaseDir, 'qookix-launcher.exe')
  const nsisDir = path.join(releaseDir, 'bundle', 'nsis')
  const installers = existsSync(nsisDir)
    ? readdirSync(nsisDir).filter((f) => f.toLowerCase().endsWith('.exe')).map((f) => path.join(nsisDir, f))
    : []
  signFiles([mainExe, ...installers])
}

process.exit(result.status ?? 1)
