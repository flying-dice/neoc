---
name: ui-styleguide
description: Design or review vos UI (the shell, packages/ui, the kit, an app's page) so it looks and behaves as iPadOS 16–17 does. A binding rulebook of ID'd, checkable rules per pattern, with an index and a spec for the automatic checks.
---

# UI Styleguide

vos looks and behaves like iPadOS 16–17 (before Liquid Glass). The references below are its rulebook: every rule is
binding, has an ID (`TYPE-07`, `MODAL-12`…), says how it is checked (`auto`, `source`, `look`, `interact`), and shows a
wrong and a right example. A design or a review cites rules by ID; a finding that fits no rule is either a gap in the
rulebook (add a rule) or not a finding.

- Build with the kit's `ui-*` elements (an app) or `@vos/ui` (the shell). Where the kit already does something,
  the rule is "use the element"; restyling around the kit in one app is a violation. When an element is missing,
  add it to the kit rather than hand-roll it in one app.
- The tokens in `packages/ui/src/styles/tokens.css` are the design language. Where the repo departs from Apple on
  purpose, for accessibility (`--label-secondary` at 0.75 in light mode, the Increase Contrast red and blue, WCAG's
  contrast sizes), the repo wins and the rule says so.
- When neither a rule nor Apple's guidelines settle a question, do what Apple's own iPad apps do (Settings, Mail,
  Notes, Files, Reminders) in iPadOS 16–17.

Read the references that match the UI being changed; for a review, walk [the index](references/index.md).

| Pattern | Reference |
| --- | --- |
| Every rule, with its check type: the review checklist and answer-key template | [Index](references/index.md) |
| The exact criteria behind every `auto` check (computed styles, geometry, shadow roots) | [Auto checks](references/auto-checks.md) |
| Type ramp, roles, weights, tracking, Dynamic Type, truncation, alignment, case, text colour | [Typography](references/typography.md) |
| Capitalisation, tone, button verbs, errors, punctuation, numbers, dates, alerts and empty states | [Writing](references/writing.md) |
| Tokens, tint, labels, backgrounds, separators, fills, contrast, dark mode | [Color](references/color.md) |
| Margins, the 4-point grid, rows, safe areas, size classes, columns, radii, elevation, images | [Layout](references/layout.md) |
| Sidebars, split views, navigation bars, back button, bar items, search placement | [Navigation](references/navigation.md) |
| Buttons, toggles, checks, segmented controls, pickers, pull-downs, sliders, steppers, disclosure, glyphs | [Controls](references/controls.md) |
| Forms: fields, keyboards, secure entry, validation, failures | [Forms](references/forms.md) |
| Inset-grouped and plain lists, row anatomy, chevrons, selection, swipe actions, empty lists | [Lists](references/lists.md) |
| Modality, alerts, action sheets, sheets, popovers, full-window covers, dimming, focus | [Modals](references/modals.md) |
| Context, pull-down and edit menus: order, groups, destructive items, glyphs, submenus | [Menus](references/menus.md) |
| Status, progress, loading, errors, banners, toasts, notifications, undo offers | [Feedback](references/feedback.md) |
| Launch, first run, onboarding, tips, tooltips, settings | [Help](references/help.md) |
| Targets, names, focus, keyboard, Reduce Motion and Transparency, large text, hover, time | [Accessibility](references/accessibility.md) |
| Materials per surface, motion tokens, purposeful motion | [Motion and materials](references/motion-and-materials.md) |
| Pointer, keyboard shortcuts, Tab, Return, Escape, drag and drop, scrolling, windows, undo and redo | [Input](references/input.md) |
