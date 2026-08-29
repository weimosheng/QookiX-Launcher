// ============================================================================
//  generate-release-notes.mjs - Generate a friendly Markdown download table for
//  the GitHub Release body from the locally-collected build artifacts.
//
//  Usage:
//    GITHUB_REPOSITORY=owner/repo node scripts/generate-release-notes.mjs \
//      <release-dir> <tag> <output.md>
//
//  Environment:
//    GITHUB_REPOSITORY e.g. "weimosheng/QookiX-Launcher" (used to build URLs)
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const [releaseDir, tag, output] = process.argv.slice(2)
const repo = process.env.GITHUB_REPOSITORY
if (!releaseDir || !tag || !output || !repo) {
  throw new Error('Usage: node scripts/generate-release-notes.mjs <release-dir> <tag> <output.md> (with GITHUB_REPOSITORY set)')
}

const ver = tag.replace(/^v/, '')
const absDir = path.resolve(releaseDir)
const files = fs
  .readdirSync(absDir)
  .filter((f) => !f.endsWith('.sig') && f !== 'latest.json')
  .sort()

const assetUrl = (name) =>
  `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`

// Classify artifacts by platform. Order matters; first match wins.
const windows = []
const macos = []
const linux = []
for (const f of files) {
  if (/_x64-setup\.exe$/.test(f)) windows.push(['安装包 (x64)', f])
  else if (/_x64\.msi$/.test(f)) windows.push(['MSI 安装包 (x64)', f])
  else if (/_x64_portable\.zip$/.test(f)) windows.push(['免安装便携版 (x64)', f])
  else if (/\.app\.tar\.gz$/.test(f)) continue // updater bundle, not for manual download
  else if (/_aarch64\.dmg$/.test(f)) macos.push(['macOS 安装包 (Apple Silicon)', f])
  else if (/_x64\.dmg$/.test(f)) macos.push(['macOS 安装包 (Intel)', f])
  else if (/\.dmg$/.test(f)) macos.push(['macOS 安装包', f])
  else if (/_aarch64\.AppImage$/.test(f)) linux.push(['AppImage (arm64)', f])
  else if (/_amd64\.AppImage$/.test(f)) linux.push(['AppImage (x64)', f])
  else if (/\.AppImage$/.test(f)) linux.push(['AppImage', f])
  else if (/\.deb$/.test(f)) linux.push(['Debian/Ubuntu (.deb)', f])
  else if (/\.rpm$/.test(f)) linux.push(['Fedora/RHEL (.rpm)', f])
}

const table = (rows) => {
  if (rows.length === 0) return null
  const lines = ['| 类型 | 文件 | 下载 |', '| --- | --- | --- |']
  for (const [label, name] of rows) {
    lines.push(`| ${label} | \`${name}\` | [下载](${assetUrl(name)}) |`)
  }
  return lines.join('\n')
}

const body = [
  `# QookiX Launcher v${ver}`,
  '',
  '> 应用内会自动检测新版本并提示更新。如自动更新未生效，可在下方按平台手动下载安装包。',
  '',
].join('\n')

const parts = []
const winTable = table(windows)
if (winTable) parts.push(`## Windows\n\n${winTable}`)
const macTable = table(macos)
if (macTable) parts.push(`## macOS\n\n${macTable}`)
const linuxTable = table(linux)
if (linuxTable) parts.push(`## Linux\n\n${linuxTable}`)

if (parts.length === 0) {
  throw new Error(`No downloadable artifacts found in ${absDir}`)
}

const full = `${body}${parts.join('\n\n---\n\n')}\n`
fs.writeFileSync(output, full)
console.log(`Wrote ${output}`)
console.log(`  Windows: ${windows.length}, macOS: ${macos.length}, Linux: ${linux.length}`)
