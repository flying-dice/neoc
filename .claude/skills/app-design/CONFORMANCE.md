# Styleguide conformance score

How well an app conforms to `docs/design/DESIGN_LANGUAGE.md`, as one number out of 100 plus nine category
scores. The conformance auditor in the `ux-research` workflow fills this in each round. A person can do it by hand
too.

**Pass:** total ≥ 90 **and** no category below 70.

## Evidence (gather all of it before scoring)

| Source | Command / place |
|---|---|
| Static audit | `bun scripts/design-audit.ts <page>` for a built-in shell page: raw colours, type, radii, shadows, gradients, materials, spacing, emoji. A Luau app has no CSS (it uses semantic tokens only), so read its props, keys, labels and copy in `apps/<app>/app.luau` instead; categories 1 to 5 are then mostly the kit's, and a defect there is a kit or token fix |
| Automated UI checks | `bunx playwright test --project=stories --project=desktop --grep <app>`: axe WCAG 2.1 AA, overflow, snapshots (starts the real host) |
| Visual baselines | `e2e/__screenshots__/stories.spec.ts/<storybook>/…` and `e2e/__screenshots__/desktop.spec.ts/<app>-…` |
| Live use | `node e2e/tools/session.mjs start audit --url "http://127.0.0.1:<port>/"`, paired as in `docs/TESTING.md`, then open the app from the Dock |

## Categories

Score each category 0 to 100. The total is the weighted sum. Every score must cite evidence (file:line, a
screenshot and region, or a test name).

| # | Category | Weight | 100 means | Deduct for |
|---|---|---|---|---|
| 1 | **Colour and tokens** (§2) | 15 | Every colour is a token and works in light and dark; one accent; destructive is red | each raw colour without a documented exception (−5); a colour that breaks in one theme (−15); a second accent (−10) |
| 2 | **Flat surfaces and elevation** (§1, §5) | 10 | Solid surfaces; only elevation-scale shadows, on the right plane | gloss, bevel, inner shadow or texture (−15 each); a non-token shadow (−5); a gradient outside wallpaper or icon tiles (−10) |
| 3 | **Materials** (§3) | 5 | Translucency only on floating chrome, always blurred; Reduce Transparency works | a translucent fill without blur (−20); a material on a content surface (−15); a hard-coded rgba on chrome (−10) |
| 4 | **Typography** (§4) | 15 | Type scale only; hierarchy is clear; survives text at 85% to 125% | each raw font size (−5); a weight below regular under 20 px (−10); clipping or overlap at 125% text (−20) |
| 5 | **Layout, spacing and radii** (§5) | 10 | 4 pt grid; 16 px list insets; radii from the scale; nested corners shrink | each off-grid spacing value (−2, max −30); each raw radius (−5); misaligned edges in a screenshot (−10) |
| 6 | **Resize and multitasking** (§7, §8) | 15 | Works at its minimum size, preferred size, full screen, split and Slide Over; never scrolls sideways | an overflow or cut-off failure (−20 each); unusable at the minimum size (−30); the minimum size isn't declared when the app needs more than 320x240 (−10) |
| 7 | **Accessibility** (§1.7, §9) | 15 | axe passes in both themes; 44 pt targets; visible focus; full keyboard use; every control labelled | each axe violation (−10); a control you can't reach by keyboard (−15); an invisible focus ring (−15); a target under 44 pt (−5) |
| 8 | **Controls and patterns** (§8, §9) | 10 | The right control for each job (table in §9); one primary action; destructive actions confirm; iPadOS conventions | the wrong control (−10); more than one filled primary button (−10); a destructive action with no confirmation (−20); a non-iPadOS pattern such as a FAB or hamburger (−15) |
| 9 | **Content and states** (§8) | 5 | Clear titles; empty, loading and error states exist and follow §8; plain language | a missing empty or error state (−20); jargon or unclear copy (−5 each) |

**Weights total 100.** Category scores are floored at 0.

## Output

```
Conformance: 87 / 100   (pass needs ≥ 90, every category ≥ 70)
 1 Colour & tokens          80  × 15
 2 Flat & elevation        100  × 10
 …
Violations (fix list):
 - [Colour] §2 raw #4b3bd6 in DiscoverPage.svelte:231 → add a --brand token, or document it as an exception
```

Record the score in the round report (`docs/ux/<app>/round-<n>.md`) so it can be compared across rounds. A score
must never go down between rounds without the report explaining why.
