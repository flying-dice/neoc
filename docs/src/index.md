---
layout: home

hero:
  name: "neoc"
  text: "A fast, lightweight Luau runtime"
  tagline: Rust's standard library, blessed gap-fillers, and vendored crates. One trusted supply chain.
  actions:
    - theme: brand
      text: Get started
      link: /guides/getting-started
    - theme: alt
      text: Browse modules
      link: /std/

features:
  - title: "std:*"
    details: Rust's standard library, 1:1 in Luau. Collections, files, I/O, networking, threads, workers.
    link: /std/
  - title: "lib:*"
    details: Curated helpers and gap-fillers. Encoding, JSON handles, test runner, recording layer.
    link: /lib/
  - title: "vnd:*"
    details: Handpicked third-party libraries, named after their upstream. HTTP, SQL, parsers, schema validation.
    link: /vnd/
---

## About

Reference for **neoc** — every module, function, and behaviour exposed to Luau scripts, organised by namespace.

Site structure follows the [documentation styleguide](https://github.com/flying-dice/neoc/blob/main/docs/styleguide/documentation-styleguide.md):

- **Landing pages** — orient and link out, one per namespace.
- **Guides** — how the engine works.
- **Reference pages** — every module's public Lua surface.

Design proposals: [RFCs](/rfcs/). Workflow: [agent-backlog Kanban skill](https://github.com/flying-dice/neoc/blob/main/.claude/skills/agent-backlog-kanban/SKILL.md).

## Where to start

[Getting started](/guides/getting-started)
:   Install neoc, run a first script.

[The module system](/guides/module-system)
:   How `require("ns:name")` resolves. Three namespaces, their rules.

[Error conventions](/guides/error-conventions)
:   `(value, err)` tuples vs raises. Error prefix format.

## The project

Personal motivation. Small agent team on a VM, each a Linux user with its own daemon. Tool sprawl got concerning — different every prompt, no visibility. Wanted a toolkit with observability and an evolvable security model.

Agents reach for it first. Missing something → feature request in Slack → next feature. Surface grows from real use.

## The name

> *Neocybex (n.) — the modern, working tongue of the machines, descended from Old Cybertronian.*

From Transformers lore. Neocybex: common tongue for trade and diplomacy, displaced Old Cybertronian. Purists hated it. Everyone uses it anyway.

Neoc is a play on "neocybex".

## The Seekers

The agents who ship, design, review, and maintain neoc.

<table>
<tr>
<td align="center" width="25%">
<img src="/seekers/starscream_real.png" width="140" alt="Starscream" /><br/>
<b>Starscream</b><br/>
<sub>Implementation lead</sub>
</td>
<td align="center" width="25%">
<img src="/seekers/thundercracker_real.png" width="140" alt="Thundercracker" /><br/>
<b>Thundercracker</b><br/>
<sub>Design &amp; documentation</sub>
</td>
<td align="center" width="25%">
<img src="/seekers/skywarp_real.png" width="140" alt="Skywarp" /><br/>
<b>Skywarp</b><br/>
<sub>Refactors &amp; precision fixes</sub>
</td>
<td align="center" width="25%">
<img src="/seekers/shockwave_real.png" width="140" alt="Shockwave" /><br/>
<b>Shockwave</b><br/>
<sub>Code review</sub>
</td>
</tr>
</table>
