// 把 tauri 打包产物收集到 out/。
// out/ 已在 .gitignore 中，交付产物只留在本地，不进版本库。
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const bundleDir = join(root, 'src-tauri/target/release/bundle')
const outDir = join(root, 'out')

const readJson = path => JSON.parse(readFileSync(join(root, path), 'utf8'))
const version = readJson('src-tauri/tauri.conf.json').version ?? readJson('package.json').version
const arch = process.arch === 'arm64' ? 'arm64' : 'x64'
const productName = readJson('src-tauri/tauri.conf.json').productName
const slug = 'WoWs-Mac-Patcher'

const newestDmg = () => {
  const dir = join(bundleDir, 'dmg')
  if (!existsSync(dir)) return null
  const files = readdirSync(dir)
    .filter(name => name.endsWith('.dmg'))
    .map(name => ({ path: join(dir, name), mtime: statSync(join(dir, name)).mtimeMs }))
    .sort((a, b) => b.mtime - a.mtime)
  return files[0] ?? null
}

const dmg = newestDmg()
if (!dmg) {
  console.error('没有找到 DMG 产物，请先执行 `npm run tauri -- build`。')
  process.exit(1)
}

mkdirSync(outDir, { recursive: true })

const dmgTarget = join(outDir, `${slug}-${version}-macos-${arch}.dmg`)
cpSync(dmg.path, dmgTarget)
console.log(`DMG  -> ${dmgTarget}`)

const appSource = join(bundleDir, 'macos', `${productName}.app`)
if (existsSync(appSource)) {
  const appTarget = join(outDir, `${productName}-${version}.app`)
  cpSync(appSource, appTarget, { recursive: true })
  console.log(`APP  -> ${appTarget}`)
}
