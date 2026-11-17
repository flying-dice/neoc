export const meta = {
  name: 'ux-research',
  description: 'One UX research round for an AgentOS app: Haiku, Sonnet and Opus participants use it for real, a styleguide conformance audit scores it, and a synthesis writes the round report and change list',
  whenToUse: 'Every design round of an AgentOS app (see .claude/skills/app-design). Args: { app, round, previous? }. Web mode must be serving on http://localhost:6200.',
  phases: [
    { title: 'Panel', detail: 'three participants (Haiku, Sonnet, Opus) walk every key journey in the real app' },
    { title: 'Audit', detail: 'styleguide conformance score against docs/design/DESIGN_LANGUAGE.md' },
    { title: 'Synthesis', detail: 'merge findings, score the round, write docs/ux/<app>/round-<n>.md' }
  ]
}

// ── Inputs ───────────────────────────────────────────────────────────────
const app = args && args.app
if (!app) throw new Error('ux-research needs args.app, e.g. { app: "calculator", round: 1 }')
const round = (args && args.round) || 1
const previous = args && args.previous // path to the previous round's report, if any
const base = (args && args.baseUrl) || 'http://localhost:6200/web.html'

const journeysPath = `apps/${app}/JOURNEYS.md (module apps) or apps/desktop/journeys/${app}.md (built-in apps and the desktop shell)`
const reportPath = `docs/ux/${app}/round-${round}.md`

// ── Participants ────────────────────────────────────────────────────────
// Different models AND different people: the spread is the point.
const PARTICIPANTS = [
  {
    id: 'haiku',
    model: 'haiku',
    persona:
      'Sam, 54, a small-business owner. Uses an iPad daily for email and invoices but is not technical. Impatient: if something is not obvious within a few seconds, says so. Reads labels literally. Uses the mouse/touch, never keyboard shortcuts.'
  },
  {
    id: 'sonnet',
    model: 'sonnet',
    persona:
      'Priya, 31, a product manager and iPad Pro power user (Stage Manager, Split View, Slide Over every day). Expects iPadOS conventions to just work and notices when something behaves differently from Apple apps. Works fast, tries the obvious gesture first.'
  },
  {
    id: 'opus',
    model: 'opus',
    persona:
      'Alex, 40, a software engineer with low vision who works mostly from the keyboard, uses dark mode, larger text and small windows. Tabs through everything, checks that focus is visible, that every control has a sensible name, and that nothing breaks when the window is narrow.'
  }
]

const PARTICIPANT_SCHEMA = {
  type: 'object',
  properties: {
    participant: { type: 'string' },
    journeys: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'string' },
          outcome: { type: 'string', enum: ['success', 'struggled', 'failed'] },
          ease: { type: 'integer', minimum: 1, maximum: 7, description: 'Single Ease Question: 1 very difficult … 7 very easy' },
          actions: { type: 'integer', description: 'number of session commands it took' },
          narrative: { type: 'string', description: 'think-aloud: what you expected, what you did, what happened' }
        },
        required: ['id', 'outcome', 'ease', 'actions', 'narrative']
      }
    },
    findings: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          severity: { type: 'string', enum: ['blocker', 'major', 'minor', 'polish'] },
          journey: { type: 'string' },
          observation: { type: 'string', description: 'what happened, concretely' },
          evidence: { type: 'string', description: 'screenshot path(s) from test-results/sessions/…' },
          suggestion: { type: 'string', description: 'what would have helped' }
        },
        required: ['severity', 'journey', 'observation', 'evidence', 'suggestion']
      }
    },
    quote: { type: 'string', description: 'one sentence, in character, about the app overall' }
  },
  required: ['participant', 'journeys', 'findings', 'quote']
}

const participantPrompt = (p) => `You are a usability-study participant. Stay in character the whole time:

${p.persona}

You are using AgentOS, an iPad-style desktop, in a real browser. You have never seen this app before. Your job is
to try to achieve each goal below the way this person would, and say honestly how it went. You are not a tester:
do not read the source code, do not use data-testid values, do not look at JOURNEYS.md beyond the goal and start
URL of each journey (the "Done when" column is for the moderator; read it only after you finish a journey, to
judge your outcome).

The goals are in ${journeysPath}, for the app "${app}".

How you use the app: a browser session driven from the shell (run \`node e2e/tools/session.mjs\` with no args for
help). Your session name is "${p.id}-r${round}". For each journey:
  1. node e2e/tools/session.mjs start ${p.id}-r${round} --url "${base}<start params from the journey>"
     (add --theme dark / --width / --height if your persona would; Alex uses dark mode and a 1024x768 screen)
  2. Loop: \`look\` (then open the screenshot with the Read tool — LOOK at it, as a person would), decide, act
     (click "<label you can see>", type, press, drag, resize…). Think aloud in your notes.
  3. Give up when this person would (Sam gives up quickly). That is a valid, useful result: outcome "failed".
  4. Stop the session (\`stop\`) before the next journey.

Report every point of friction as a finding with a screenshot path as evidence: anything confusing, slow,
unlabelled, cut off, hidden behind something, inconsistent with iPadOS, low-contrast, or broken. Severity:
blocker = could not finish; major = finished only with real difficulty or a wrong turn; minor = noticeable
friction; polish = cosmetic. Do not invent problems you did not experience, and do not soften ones you did.`

// ── Conformance audit ───────────────────────────────────────────────────
const AUDIT_SCHEMA = {
  type: 'object',
  properties: {
    score: { type: 'number', description: 'weighted total, 0-100' },
    categories: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          id: { type: 'string' },
          weight: { type: 'number' },
          score: { type: 'number', description: '0-100 for this category' },
          evidence: { type: 'array', items: { type: 'string' } }
        },
        required: ['id', 'weight', 'score', 'evidence']
      }
    },
    violations: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          category: { type: 'string' },
          rule: { type: 'string', description: 'the DESIGN_LANGUAGE.md section / checklist item' },
          where: { type: 'string', description: 'file:line or screenshot + region' },
          fix: { type: 'string' }
        },
        required: ['category', 'rule', 'where', 'fix']
      }
    }
  },
  required: ['score', 'categories', 'violations']
}

const auditPrompt = `You are the design-systems reviewer for AgentOS. Score how well the app "${app}" conforms to the
styleguide in docs/design/DESIGN_LANGUAGE.md, using the rubric in .claude/skills/app-design/CONFORMANCE.md
(categories, weights, and what earns each score are defined there — follow it exactly, do not invent categories).

Evidence to gather (all of it, before scoring):
  • \`bun scripts/design-audit.ts ${app}\` — static token/flat/spacing findings in the app's components.
  • The visual baselines: e2e/__screenshots__/stories.spec.ts/** and e2e/__screenshots__/desktop.spec.ts/** files
    for this app (light, dark, min, large, fullscreen, split, slideover, screen sizes). Open them with Read and look.
  • A live look in web mode: node e2e/tools/session.mjs start audit-r${round} --url "${base}?open=${app}"
    (then look, resize the screen to 1024 768, try dark with --theme dark in a new session, Tab through controls).
  • The automated results: \`npx playwright test --project=stories --project=desktop --grep "${app}" --reporter=line\`
    (axe, overflow and snapshot checks). Do NOT pass --update-snapshots.

Every category score must cite evidence (file:line, screenshot path + region, or test name). A deliberate,
documented exception in the design language (e.g. an instrument palette, §2) is not a violation.`

// ── Run ──────────────────────────────────────────────────────────────────
log(`UX research round ${round} for "${app}"${previous ? ` (previous: ${previous})` : ''}`)

const [panel, audit] = await parallel([
  () =>
    parallel(
      PARTICIPANTS.map((p) => () =>
        agent(participantPrompt(p), { label: `participant:${p.id}`, phase: 'Panel', model: p.model, schema: PARTICIPANT_SCHEMA })
      )
    ),
  () => agent(auditPrompt, { label: 'conformance-audit', phase: 'Audit', model: 'opus', schema: AUDIT_SCHEMA })
])

const sessions = (panel || []).filter(Boolean)
const dropped = PARTICIPANTS.length - sessions.length
if (dropped) log(`${dropped} participant(s) returned nothing; the round is incomplete`)
if (!audit) log('conformance audit returned nothing; the round has no score')

phase('Synthesis')
const SYNTHESIS_SCHEMA = {
  type: 'object',
  properties: {
    report: { type: 'string', description: 'path of the report written' },
    journeySuccessRate: { type: 'number', description: 'successes / (journeys x participants), 0-1' },
    meanEase: { type: 'number' },
    conformance: { type: 'number' },
    exitCriteriaMet: { type: 'boolean' },
    changes: {
      type: 'array',
      items: {
        type: 'object',
        properties: {
          priority: { type: 'integer', description: '1 = do first' },
          change: { type: 'string', description: 'the design change, concretely (component, behaviour, token)' },
          why: { type: 'string', description: 'which findings / violations it resolves, and how many participants hit it' },
          files: { type: 'array', items: { type: 'string' } }
        },
        required: ['priority', 'change', 'why', 'files']
      }
    }
  },
  required: ['report', 'journeySuccessRate', 'meanEase', 'conformance', 'exitCriteriaMet', 'changes']
}

const synthesis = await agent(
  `You are the UX research lead. Synthesise round ${round} of the design study for the AgentOS app "${app}" and write
the round report to ${reportPath} (create the folder if needed).

Participant sessions (JSON):
${JSON.stringify(sessions, null, 2)}

Conformance audit (JSON):
${JSON.stringify(audit, null, 2)}

${previous ? `The previous round's report is ${previous}: compare against it (what improved, what regressed, what is still open).` : 'This is the first round.'}

The report must contain, in this order:
  1. Summary: one paragraph, plus the scorecard (journey success matrix: journeys × participants with outcome and
     ease; success rate; mean ease; conformance score with per-category scores; exit criteria met or not, and which fail).
  2. Findings: deduplicated across participants and the audit; each with severity, how many participants hit it,
     the evidence paths and the participants' own words where useful. Most severe and most frequent first.
  3. Change list: the design changes to make before the next round, prioritised. Each says exactly what to change
     (component / token / behaviour, with file paths) and which findings it resolves. Prefer fixing root causes in
     the kit or tokens over one-off patches, and never propose something that breaks DESIGN_LANGUAGE.md — if the
     styleguide itself is wrong, say so as a separate "styleguide change" item.
  4. Participant quotes.

Exit criteria (all must hold, from .claude/skills/app-design/SKILL.md): every journey succeeded for every
participant; mean ease ≥ 6.0; no blocker or major findings open; conformance ≥ 90 with no category below 70.`,
  { label: 'synthesis', phase: 'Synthesis', schema: SYNTHESIS_SCHEMA }
)

return { app, round, report: reportPath, synthesis, participants: sessions.map((s) => s.participant), audit: audit && audit.score }
