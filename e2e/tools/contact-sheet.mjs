#!/usr/bin/env node
/**
 * Lay several screenshots side by side in one PNG, labelled, so they can be
 * reviewed (by a person or an agent) in a single look.
 *
 *   node e2e/tools/contact-sheet.mjs out.png a.png b.png …   [--columns 4] [--width 1800]
 */
import { readFileSync } from 'node:fs'
import { basename } from 'node:path'
import { chromium } from '@playwright/test'

const args = process.argv.slice(2)
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`)
  return i === -1 ? fallback : Number(args.splice(i, 2)[1])
}
const columns = opt('columns', 4)
const width = opt('width', 1800)
const [out, ...files] = args
if (!out || !files.length) {
  console.error('Usage: contact-sheet.mjs out.png img1.png img2.png … [--columns 4] [--width 1800]')
  process.exit(1)
}

const cells = files
  .map((f) => {
    const src = `data:image/png;base64,${readFileSync(f).toString('base64')}`
    return `<figure><img src="${src}"><figcaption>${basename(f).replace(/-(darwin|linux|win32)\.png$/, '')}</figcaption></figure>`
  })
  .join('')

const browser = await chromium.launch()
const page = await browser.newPage({ viewport: { width, height: 400 } })
await page.setContent(`<style>
  body { margin: 0; padding: 12px; background: #fff; font: 12px -apple-system, system-ui, sans-serif; }
  main { display: grid; grid-template-columns: repeat(${columns}, 1fr); gap: 12px; align-items: start; }
  figure { margin: 0; border: 1px solid #ddd; }
  img { display: block; width: 100%; }
  figcaption { padding: 4px 6px; background: #f4f4f4; overflow-wrap: anywhere; }
</style><main>${cells}</main>`)
await page.screenshot({ path: out, fullPage: true })
await browser.close()
console.log(out)
