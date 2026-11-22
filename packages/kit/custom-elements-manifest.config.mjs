/** The Custom Elements Manifest of the kit (`bun run build:kit` writes it beside kit.js as /kit/custom-elements.json). */
export default {
  globs: ['src/components/ui-*.ts', 'src/boolean-control.ts', 'src/form-control.ts'],
  exclude: ['src/**/*.stories.ts', 'src/**/*.test.ts'],
  outdir: '../../client/public/kit',
  litelement: true,
  // do not write a `customElements` field into package.json: the manifest is a build output
  packagejson: false
}
