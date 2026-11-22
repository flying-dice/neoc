/**
 * The Custom Elements Manifest of the kit. `bun run build:kit` writes it here, committed (`packages/kit/custom-elements.json`:
 * the vos Vite plugin reads it to give every app its editor types, so an app's type check needs no kit build first), and
 * copies it beside kit.js as /kit/custom-elements.json. `scripts/kit-types.test.ts` fails when the committed copy is stale.
 */
export default {
  globs: ['src/components/ui-*.ts', 'src/boolean-control.ts', 'src/form-control.ts'],
  exclude: ['src/**/*.stories.ts', 'src/**/*.test.ts'],
  outdir: '.',
  litelement: true,
  // do not write a `customElements` field into package.json: the manifest is a build output
  packagejson: false
}
