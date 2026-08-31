// ============================================================================
//  fetch-release-json.mjs - Snapshot the GitHub Release API response (which
//  contains EVERY asset with its name, size and download URL) into a JSON file
//  so the mirror bucket can host a full release listing (e.g. `release.json`)
//  alongside the updater `latest.json`.
//
//  Why: `latest.json` only carries the signed updater bundles. The mirror site
//  (object storage bucket) may also want to list every installer/asset of a
//  release with its byte size — exactly what the GitHub API returns.
//
//  Usage:
//    GH_TOKEN=<token> GITHUB_REPOSITORY=owner/repo GITHUB_TAG=v0.3.1 \
//      node scripts/fetch-release-json.mjs release/release.json
//
//  Environment:
//    GH_TOKEN          GitHub token with read access to the repo
//    GITHUB_REPOSITORY e.g. "weimosheng/QookiX-Launcher"
//    GITHUB_TAG        e.g. "v0.3.1"
//
//  Arguments:
//    <out>             Output file path (e.g. release/release.json)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const token = process.env.GH_TOKEN || process.env.GITHUB_TOKEN
const repo = process.env.GITHUB_REPOSITORY
const tag = process.env.GITHUB_TAG
const out = process.argv[2]
if (!token || !repo || !tag || !out) {
  throw new Error('GH_TOKEN, GITHUB_REPOSITORY, GITHUB_TAG and an output path are all required')
}

const headers = {
  Authorization: `Bearer ${token}`,
  Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2022-11-28',
}

const res = await fetch(`https://api.github.com/repos/${repo}/releases/tags/${tag}`, { headers })
if (!res.ok) {
  throw new Error(`Failed to fetch release: ${res.status} ${await res.text()}`)
}

const release = await res.json()
fs.mkdirSync(path.dirname(out), { recursive: true })
fs.writeFileSync(out, `${JSON.stringify(release, null, 2)}\n`)
console.log(`Wrote ${out} with ${release.assets?.length ?? 0} asset(s)`)
