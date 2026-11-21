# @agentos/ui

AgentOS UI kit — the iOS / iPadOS 16–17 design language (before Liquid Glass) for Svelte 5.

## Strict atomic design

| Layer | May import components from | Examples |
| --- | --- | --- |
| **atoms** | nothing | `Button`, `Icon`, `Text`, `Toggle`, `IconTile`, `Material` |
| **molecules** | atoms | `ListRow`, `AppIcon`, `SearchField`, `IconButton`, `Alert`, `Sheet` |
| **organisms** | atoms, molecules | `NavigationBar`, `TabBar`, `ListSection`, `StatusBar`, `Dock`, `Window` |
| **templates** | atoms → organisms (layout only, regions are snippets) | `DesktopTemplate`, `SplitViewTemplate`, `ScreenTemplate` |
| **pages** | anything below (live in the client, not here) | `client/src/pages/*` |

Rules, enforced by `bun run lint:atomic` (`scripts/check-atomic.ts`):

1. Every `.svelte` file lives in exactly one layer folder (the client has one composition root, `client/src/App.svelte`).
2. Imports go **strictly downward** — never sideways within a layer, never up.
3. Every component starts with a `<!-- @<layer> Name — … -->` header matching its folder.
4. Consumers import through layer entry points, never deep paths:

```ts
import { Button, Icon } from '@agentos/ui/atoms'
import { ListRow } from '@agentos/ui/molecules'
import { Window } from '@agentos/ui/organisms'
import { DesktopTemplate } from '@agentos/ui/templates'
import type { IconName } from '@agentos/ui'
import '@agentos/ui/styles.css' // tokens + base, once at app entry
```

Content flows *into* a component through snippets, which doesn't count as an import. That's how an organism like `ListSection` holds `ListRow`s without importing them.

## Tokens

`src/styles/tokens.css` holds the system colours, labels, fills, grouped backgrounds, materials, Dynamic Type, radii, shadows and motion. Every colour is declared once with `light-dark()`. The theme follows the OS, or you can force it with `data-theme="light" | "dark"` on any subtree. Re-tint controls with `--tint`.
