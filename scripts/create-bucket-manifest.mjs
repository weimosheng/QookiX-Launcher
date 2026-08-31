// ============================================================================
//  create-bucket-manifest.mjs - Generate the Tauri updater `latest.json` for the
//  OBJECT STORAGE (bucket) update source.
//
//  The GitHub-updater flow points `latest.json` URLs at the GitHub Release; the
//  bucket source (「设置 → 更新源 → 对象存储」) instead serves the exact same
//  installers from your object storage bucket. This script rebuilds the manifest
//  so every platform URL points at the artifact on the bucket (`<base>/<name>`),
//  and CI uploads it (as `latest.json`) together with the installers.
//
//  Usage:
//    node scripts/create-bucket-manifest.mjs <artifacts-dir> <version-tag> <output.json> [base-url]
//    env: QOOKIX_BUCKET_UPDATE_URL 更新目录的公开根 URL（缺省时用第 4 个参数）
//    exit code 0 = wrote manifest, 2 = nothing to write (caller may skip)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'
import { UPDATER_TARGETS } from './shared/updater-targets.mjs'

const [dir, tag, outputPath, baseUrlArg] = process.argv.slice(2)
if (!dir || !tag || !outputPath) {
  throw new Error(
    'Usage: node create-bucket-manifest.mjs <artifacts-dir> <version-tag> <output.json> [base-url]',
  )
}
// 与 Rust 端 `option_env!("QOOKIX_BUCKET_UPDATE_URL")` 保持一致
const base = (baseUrlArg || process.env.QOOKIX_BUCKET_UPDATE_URL || '')
  .trim()
  .replace(/\/+$/, '')
if (!base) {
  throw new Error('QOOKIX_BUCKET_UPDATE_URL (or the base-url argument) is required')
}

const targets = UPDATER_TARGETS

function filesEndingWith(dirname, suffix) {
  try {
    return fs.readdirSync(dirname).filter((name) => name.endsWith(suffix))
  } catch {
    return []
  }
}

const platforms = {}
for (const t of targets) {
  let matches = []
  for (const suffix of t.suffixes) {
    matches = filesEndingWith(dir, suffix)
    if (matches.length >= 1) break
  }
  if (matches.length !== 1) continue // platform not built (or named differently)
  const name = matches[0]
  const sigPath = path.join(dir, `${name}.sig`)
  if (!fs.existsSync(sigPath)) continue // unsigned bundle -> skip
  const signature = fs.readFileSync(sigPath, 'utf8')
  const url = `${base}/${encodeURIComponent(name)}`
  for (const p of t.platforms) {
    platforms[p] = { signature, url }
  }
}

if (Object.keys(platforms).length === 0) {
  console.warn(
    'create-bucket-manifest: no signed updater bundles found (TAURI_SIGNING_PRIVATE_KEY not configured?), skipping latest.json',
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
  `create-bucket-manifest: wrote ${outputPath} for ${Object.keys(platforms).length} platform(s) (base=${base})`,
)
