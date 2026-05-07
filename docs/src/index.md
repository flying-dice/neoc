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
    details: Curated helpers and gap-fillers we wrote. Encoding, JSON document handles, the test runner, the recording layer.
    link: /lib/
  - title: "vnd:*"
    details: Handpicked third-party libraries, named after their upstream. HTTP, SQL, parsers, schema validation.
    link: /vnd/
---

## About

This is the reference for **neoc** — every module, function, and behaviour exposed to Luau scripts running inside the engine, organised by namespace.

The site follows three tiers from the [documentation styleguide](https://github.com/flying-dice/neoc/blob/main/docs/styleguide/documentation-styleguide.md):

- **Landing pages** orient and link out, one per namespace.
- **Guides** explain how the engine works and how to use it.
- **Reference pages** document every module's public Lua surface.

Design proposals for cross-cutting changes live under [RFCs](/rfcs/). The development workflow is the agent-backlog Kanban, defined in the in-repo [`agent-backlog-kanban` skill](https://github.com/flying-dice/neoc/blob/main/.claude/skills/agent-backlog-kanban/SKILL.md).

## Where to start

[Getting started](/guides/getting-started)
:   Install neoc and run a first script.

[The module system](/guides/module-system)
:   How `require("ns:name")` resolves, what the three namespaces are for, and the rules each follows.

[Error conventions](/guides/error-conventions)
:   When functions return `(value, err)` tuples and when they raise; how error prefixes are formatted.

## The Project

The motivation was personal. I run a small agent team on a VM, each one a Linux user with its own daemon. The tool sprawl got worrying: different every prompt, no real visibility. I wanted a toolkit with observability built in and a security model I could evolve.

The agents reach for it first. When something's missing, they file a feature request in Slack, which becomes the next feature. The surface grows from real use.

## The Name

> *Neocybex (n.) — the modern, working tongue of the machines, descended in use from Old Cybertronian.*

Neocybex comes from Transformers lore, which my agents were based on — the modern, everyday language of Cybertron, the one most bots actually speak. It emerged as a common tongue for trade and diplomacy and gradually displaced Old Cybertronian, the older, denser, more sacred language of the Primes' era. The purists hated it. Everyone uses it anyway.

Neoc is a play on the word "neocybex".

## The Seekers

The agents who ship, design, review, and maintain neoc — the runtime that speaks it.

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