// ============================================================================
//  create-update-manifest-local.mjs - Generate the Tauri updater `latest.json`
//  from LOCAL build artifacts only.
//
//  Unlike the release-API-based variant, asset URLs are derived directly from
//  the tag, so this works even before the GitHub Release exists — which makes
//  the publish step robust against release-state churn (deleted/untagged
//  releases, parallel runs, etc.).
//
//  Usage:
//    node scripts/create-update-manifest-local.mjs <artifacts-dir> <version-tag> <output.json>
//    env: GITHUB_REPOSITORY (e.g. "owner/repo")
//    exit code 0 = wrote manifest, 2 = nothing to write (caller may skip)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const [dir, tag, outputPath] = process.argv.slice(2)
if (!dir || !tag || !outputPath) {
  throw new Error(
    'Usage: node create-update-manifest-local.mjs <artifacts-dir> <version-tag> <output.json>',
  )
}
const repo = process.env.GITHUB_REPOSITORY
if (!repo) throw new Error('GITHUB_REPOSITORY is not set')

const targets = [
  { platforms: ['windows-x86_64'], suffix: '.nsis.zip' },
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], suffix: '.app.tar.gz' },
  { platforms: ['linux-x86_64'], suffix: '_amd64.AppImage.tar.gz' },
  { platforms: ['linux-aarch64'], suffix: '_aarch64.AppImage.tar.gz' },
]

function filesEndingWith(dir, suffix) {
  try {
    return fs.readdirSync(dir).filter((name) => name.endsWith(suffix))
  } catch {
    return []
  }
}

const platforms = {}
for (const t of targets) {
  const matches = filesEndingWith(dir, t.suffix)
  if (matches.length !== 1) continue // platform not built (or named differently)
  const name = matches[0]
  const sigPath = path.join(dir, `${name}.sig`)
  if (!fs.existsSync(sigPath)) continue // unsigned bundle -> skip
  const signature = fs.readFileSync(sigPath, 'utf8')
  const url = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`
  for (const p of t.platforms) {
    platforms[p] = { signature, url }
  }
}

if (Object.keys(platforms).length === 0) {
  console.warn(
    'create-update-manifest-local: no signed updater bundles found (TAURI_SIGNING_PRIVATE_KEY not configured?), skipping latest.json',
  )
  process.exit(2)
}

const manifest = {
  version: tag.replace(/^v/, ''),
  notes: '',
  pub_date: new Date().toISOString(),
  platforms,
}

fs.writeFileSync(outputPath, `${JSON.stringify(manifest, null, 2)}\n`)
console.log(
  `create-update-manifest-local: wrote ${outputPath} for ${Object.keys(platforms).length} platform(s)`,
)
