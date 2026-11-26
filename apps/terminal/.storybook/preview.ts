/**
 * What the Terminal app's stories run in. The host's frame gives a real app three things (`HOST_TAGS` in @vos/vite-plugin-v2):
 * the kit's tokens, the kit's elements and the bridge. Here:
 *  - the tokens come with the shared preview (`@vos/ui/styles.css`, the sheet `/kit/tokens.css` is generated from);
 *  - the `ui-*` elements are defined from the kit's source, before any story is imported (the frame's `/kit/kit.js` is
 *    this same file, bundled);
 *  - there is no bridge and no `RPC` or `vos`: only the composition root (`src/App.svelte`) uses them, and no story
 *    renders that. Stories build the state with a fake backend (`src/lib/fakeApi.ts`).
 */
import '../../../packages/kit/src/index'

export { default } from '@vos/storybook/preview'
