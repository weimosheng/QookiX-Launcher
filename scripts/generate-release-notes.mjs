// ============================================================================
//  generate-release-notes.mjs - Generate the GitHub Release body:
//  CHANGELOG section (if given) + a friendly Markdown download table.
//
//  Usage:
//    GITHUB_REPOSITORY=owner/repo node scripts/generate-release-notes.mjs \
//      <release-dir> <tag> <output.md> [changelog.md]
//
//  Environment:
//    GITHUB_REPOSITORY e.g. "weimosheng/QookiX-Launcher" (used to build URLs)
//
//  更新日志来自 CHANGELOG.md 里版本号对得上的那一节（`## [0.10.0] - 日期`）。
//  对不上时依次退化：用 [Unreleased] 那一节 → 只用下载表格（只在日志里警告，
//  不让发版失败）。
// ============================================================================
import fs from 'node:fs'
import path from 'node:path'

const [releaseDir, tag, output, changelogPath] = process.argv.slice(2)
const repo = process.env.GITHUB_REPOSITORY
if (!releaseDir || !tag || !output || !repo) {
  throw new Error(
    'Usage: node scripts/generate-release-notes.mjs <release-dir> <tag> <output.md> [changelog.md] (with GITHUB_REPOSITORY set)',
  )
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

// ---------------------------------------------------------------- CHANGELOG ----

/** 按 `## ` 标题切成若干节（标题在文件里也常写成 `## [0.10.0] - 2026-09-27`） */
function splitSections(text) {
  const sections = []
  let current = null
  for (const line of text.replace(/\r\n/g, '\n').split('\n')) {
    const heading = /^##\s+(.*)$/.exec(line)
    if (heading) {
      current = { title: heading[1].trim(), body: [] }
      sections.push(current)
      continue
    }
    if (current) current.body.push(line)
  }
  return sections
}

/** 只留标题里的版本号：`[0.10.0] - 2026-09-27` / `v0.10.0` → `0.10.0` */
function sectionVersion(title) {
  const inner = /^\[(.*?)\]\s*(.*)$/.exec(title.trim())
  const rest = inner ? `${inner[1]}${inner[2] ? ` - ${inner[2]}` : ''}` : title
  return rest
    .replace(/^v/, '')
    .split(/\s+-\s+/)[0]
    .trim()
}

function pickChangelog(file, version) {
  if (!file) return null
  let text
  try {
    text = fs.readFileSync(file, 'utf8')
  } catch {
    console.warn(`! 读不到 ${file}，本次 Release 只放下载表格`)
    return null
  }

  const sections = splitSections(text)
  let hit = sections.find((s) => sectionVersion(s.title) === version)
  let fallback = false
  if (!hit) {
    hit = sections.find((s) => /unreleased/i.test(s.title))
    fallback = !!hit
  }
  if (!hit) {
    console.warn(`! ${file} 里没有 ${version} 的小节，也没找到 [Unreleased]，本次不放更新日志`)
    return null
  }
  if (fallback) {
    console.warn(`! ${file} 里没有 ${version} 的小节，暂用 [Unreleased] 那一节（发版前记得把标题改成该版本）`)
  }

  let body = hit.body.join('\n').trim()
  if (!body && !fallback) {
    // 版本号那一节只写了标题还没填内容时，退而看看 [Unreleased] 里有没有东西
    const unreleased = sections.find((s) => /unreleased/i.test(s.title))
    const unreleasedBody = unreleased?.body.join('\n').trim()
    if (unreleasedBody) {
      console.warn(`! ${file} 里「${hit.title}」一节是空的，改用 [Unreleased] 的内容`)
      return { title: unreleased.title, body: unreleasedBody, fallback: true }
    }
  }
  if (!body) {
    console.warn(`! ${file} 里「${hit.title}」一节是空的，本次不放更新日志`)
    return null
  }
  return { title: hit.title, body, fallback }
}

const changelog = pickChangelog(changelogPath, ver)

/**
 * 把一节渲染成 Release 里的样子：有序列表 + 加粗标签开头，例如
 *   0. **投影预览**：修复了……
 * 两种写法都支持：
 *   - `### 分类` + `- 条目`  → 分类变成条目的加粗标签
 *   - 自己写成 `0. **标签**：说明`  → 原样保留，不动你的编号和标签
 */
function renderEntries(body) {
  const items = []
  let category = ''
  let numbered = false
  for (const raw of body.split('\n')) {
    const line = raw.trim()
    if (!line) continue
    const heading = /^###\s+(.+?)\s*$/.exec(line)
    if (heading) {
      category = heading[1]
      continue
    }
    const bullet = /^[-*]\s+(.+)$/.exec(line)
    if (bullet) {
      items.push({ label: category, text: bullet[1].trim() })
      continue
    }
    const ordered = /^\d+[.)]\s+(.+)$/.exec(line)
    if (ordered) {
      numbered = true
      items.push({ label: category, text: ordered[1].trim() })
      continue
    }
    items.push({ label: category, text: line })
  }
  if (items.length === 0) return null
  // 自己编好号、也没用 `###` 分类的，就原样交给 GitHub 渲染
  if (numbered && !items.some((i) => i.label)) return body.trim()
  return items
    .map((item, i) => {
      const hasLabel = /^\*\*[^*]+\*\*[:：]/.test(item.text)
      const label = item.label && !hasLabel ? `**${item.label}**：` : ''
      // "**修复**：修复了 xxx" 这种重复就删掉正文里那个动词
      const text = item.label && !hasLabel ? stripRedundantVerb(item.text) : item.text
      return `${i}. ${label}${text}`
    })
    .join('\n')
}

/** 条目开头的动词和标签重复时（`修复` / `修复了`）去掉，重写后太短就不动 */
function stripRedundantVerb(text) {
  const stripped = text
    .replace(/^(修复|修正|解决|新增|添加|增加|优化|改进|提升|重构|调整|完善|更新)(了|的)?[:：]?\s*/, '')
    .trim()
  return stripped.length >= 4 ? stripped : text
}

const TIP = '> 应用内会自动检测新版本并提示更新。如自动更新未生效，可在下方按平台手动下载安装包。'

const parts = []
if (changelog) {
  // 日志里点明来源，方便核对拿到的是哪一节
  console.log(`Changelog: ${changelogPath} → 「${changelog.title}」${changelog.fallback ? '（回退到 Unreleased）' : ''}`)
  const entries = renderEntries(changelog.body)
  if (entries) {
    parts.push(`## 更新日志 v${ver}\n\n${entries}`)
    parts.push(TIP) // 提示挪到下载表格前面
  }
}

const winTable = table(windows)
if (winTable) parts.push(`## Windows\n\n${winTable}`)
const macTable = table(macos)
if (macTable) parts.push(`## macOS\n\n${macTable}`)
const linuxTable = table(linux)
if (linuxTable) parts.push(`## Linux\n\n${linuxTable}`)

if (!winTable && !macTable && !linuxTable) {
  throw new Error(`No downloadable artifacts found in ${absDir}`)
}
// 没有更新日志时，提示还是放最上面
if (!changelog) parts.unshift(TIP)

const full = `${parts.join('\n\n')}\n`
fs.writeFileSync(output, full)
console.log(`Wrote ${output}`)
console.log(`  Changelog: ${changelog ? 'yes' : 'no'}`)
console.log(`  Windows: ${windows.length}, macOS: ${macos.length}, Linux: ${linux.length}`)
