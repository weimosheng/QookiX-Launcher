// ============================================================================
//  fix-update-manifest.mjs - Regenerate a CORRECT `latest.json` straight from
//  the assets already published on a GitHub Release, then upload it back to
//  overwrite the broken manifest.
//
//  Why: the previous `latest.json` referenced a Windows asset name that does
//  not exist on the release (e.g. `QookiX Launcher_...` vs the real
//  `QookiX.Launcher_...`), so the updater hit a 404 and every auto-update
//  failed. This script rebuilds the manifest from the release's actual
//  `browser_download_url` + its matching `.sig`, guaranteeing URL/signature
//  consistency.
//
//  Usage:
//    GH_TOKEN=<token> GITHUB_REPOSITORY=owner/repo GITHUB_TAG=v0.3.1 \
//      node scripts/fix-update-manifest.mjs
//
//  Environment:
//    GH_TOKEN          GitHub token with write access to the repo's releases
//    GITHUB_REPOSITORY e.g. "weimosheng/QookiX-Launcher"
//    GITHUB_TAG        e.g. "v0.3.1"
// ============================================================================
import fs from 'node:fs'

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN
const repo = process.env.GITHUB_REPOSITORY
const tag = process.env.GITHUB_TAG
if (!token || !repo || !tag) {
  throw new Error('GH_TOKEN, GITHUB_REPOSITORY and GITHUB_TAG are all required')
}

const api = 'https://api.github.com'
const headers = {
  Authorization: `Bearer ${token}`,
  Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2022-11-28',
}

const relRes = await fetch(`${api}/repos/${repo}/releases/tags/${tag}`, { headers })
if (!relRes.ok) throw new Error(`Failed to fetch release: ${relRes.status} ${await relRes.text()}`)
const release = await relRes.json()
const assets = release.assets
console.log(`Loaded release ${tag} with ${assets.length} asset(s)`)

// Updater bundle matchers. Order matters; first match per platform wins.
const targets = [
  { platforms: ['windows-x86_64'], re: /_x64-setup\.exe$/ },
  { platforms: ['darwin-aarch64', 'darwin-x86_64'], re: /\.app\.tar\.gz$/ },
  { platforms: ['linux-x86_64'], re: /_amd64\.AppImage$/ },
  { platforms: ['linux-aarch64'], re: /_aarch64\.AppImage$/ },
]

const platforms = {}
for (const t of targets) {
  const asset = assets.find((a) => t.re.test(a.name))
  if (!asset) {
    console.log(`  - skip ${t.platforms.join('/')}: no asset matching ${t.re}`)
    continue
  }
  const sigAsset = assets.find((a) => a.name === `${asset.name}.sig`)
  if (!sigAsset) {
    console.log(`  - skip ${t.platforms.join('/')}: no .sig for ${asset.name}`)
    continue
  }
  const sigRes = await fetch(sigAsset.browser_download_url)
  if (!sigRes.ok) {
    console.log(`  - skip ${t.platforms.join('/')}: failed to download ${sigAsset.name}`)
    continue
  }
  const signature = (await sigRes.text()).trim()
  for (const p of t.platforms) {
    platforms[p] = { signature, url: asset.browser_download_url }
  }
  console.log(`  - ok ${t.platforms.join('/')}: ${asset.name}`)
}

if (Object.keys(platforms).length === 0) {
  throw new Error('No signed updater bundles found on the release; nothing to publish')
}

const manifest = {
  version: tag.replace(/^v/, ''),
  notes: release.body ?? '',
  pub_date: new Date().toISOString(),
  platforms,
}
fs.writeFileSync('latest.json', `${JSON.stringify(manifest, null, 2)}\n`)
console.log('Wrote latest.json with platforms:', Object.keys(platforms).join(', '))

// Overwrite the old manifest on the release (delete then upload).
const old = assets.find((a) => a.name === 'latest.json')
if (old) {
  const del = await fetch(`${api}/repos/${repo}/releases/assets/${old.id}`, { method: 'DELETE', headers })
  if (!del.ok && del.status !== 404) {
    throw new Error(`Failed to delete old latest.json: ${del.status} ${await del.text()}`)
  }
  console.log('Deleted old latest.json asset')
}
// IMPORTANT: release asset uploads MUST go to uploads.github.com, not
// api.github.com (which returns 404 for asset uploads).
const uploadsApi = 'https://uploads.github.com'
const up = await fetch(
  `${uploadsApi}/repos/${repo}/releases/${release.id}/assets?name=latest.json`,
  {
    method: 'POST',
    headers: { ...headers, 'Content-Type': 'application/octet-stream' },
    body: fs.readFileSync('latest.json'),
  },
)
if (!up.ok) throw new Error(`Failed to upload latest.json: ${up.status} ${await up.text()}`)
console.log('Uploaded corrected latest.json — updater should now work.')
