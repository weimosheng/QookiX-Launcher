// ============================================================================
//  create-update-manifest.mjs - Generate the Tauri updater `latest.json` from
//  a GitHub release's assets + the minisign `.sig` signatures produced by
//  `tauri build` (when TAURI_SIGNING_PRIVATE_KEY is set).
//
//  Mirrors the format used by the Tauri updater plugin so the launcher can
//  self-update from GitHub Releases. Only platforms that actually built a
//  signed updater bundle are included; platforms that were not built or are
//  unsigned are skipped (no hard failure).
//
//  Usage:
//    node scripts/create-update-manifest.mjs <release.json> <signatures-dir> <version-tag> <output.json>
//    exit code 0 = wrote manifest, 2 = nothing to write (caller may skip upload)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const [releasePath, signaturesPath, tag, outputPath] = process.argv.slice(2)
if (!releasePath || !signaturesPath || !tag || !outputPath) {
  throw new Error(
    'Usage: node create-update-manifest.mjs <release.json> <signatures-dir> <version-tag> <output.json>',
  )
}

const release = JSON.parse(fs.readFileSync(releasePath, 'utf8'))
const assets = release.assets
if (!Array.isArray(assets)) {
  throw new Error('Release metadata does not contain an assets array')
}

// Updater bundle suffixes produced by `tauri build` with createUpdaterArtifacts.
const targets = [
  { platforms: ['windows-x86_64'], suffix: '.nsis.zip' },
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], suffix: '.app.tar.gz' },
  { platforms: ['linux-x86_64'], suffix: '_amd64.AppImage.tar.gz' },
  { platforms: ['linux-aarch64'], suffix: '_aarch64.AppImage.tar.gz' },
]

const platforms = {}
for (const t of targets) {
  const matches = assets.filter((a) => a.name?.endsWith(t.suffix))
  if (matches.length !== 1) continue // platform not built (or named differently)
  const asset = matches[0]
  const signaturePath = path.join(signaturesPath, `${asset.name}.sig`)
  if (!fs.existsSync(signaturePath)) continue // unsigned bundle -> skip
  const signature = fs.readFileSync(signaturePath, 'utf8')
  const url = asset.browser_download_url ?? asset.url
  if (!url) continue
  for (const p of t.platforms) {
    platforms[p] = { signature, url }
  }
}

if (Object.keys(platforms).length === 0) {
  console.warn(
    'create-update-manifest: no signed updater bundles found (TAURI_SIGNING_PRIVATE_KEY not configured?), skipping latest.json',
  )
  process.exit(2)
}

const manifest = {
  version: tag.replace(/^v/, ''),
  notes: release.body ?? '',
  pub_date: new Date().toISOString(),
  platforms,
}

fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`)
console.log(
  `create-update-manifest: wrote ${outputPath} for ${Object.keys(platforms).length} platform(s)`,
)
