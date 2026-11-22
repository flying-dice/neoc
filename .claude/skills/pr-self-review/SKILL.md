---
name: pr-self-review
description: Use before handing a pull request to independent review, and again before every push that answers a review. The author's checklist, one line per rule of REVIEW-READY.md, each with evidence (a test name, a command and its result, what was seen on the real host), posted as the Self-review note for the head SHA.
---

# Self-review

You are the first reviewer of your change. The independent reviewer (`pr-review`) uses the change the way its consumer
will, gives it hostile input, breaks it half way and checks every sentence. Whatever you skip here, they find, and each
finding costs a round. The rules and the reasons for them are in [REVIEW-READY.md](../../../REVIEW-READY.md); this skill
is the checklist that proves you followed them.

## Standard

- **Evidence or FAIL.** Every line carries a test name, a command and its result at the head SHA, or what you saw on the real
  host. "Checked", "covered", "should work" and "PASS" alone are not evidence.
- **N/A needs a reason.** Write `N/A — <reason>` ("no queue or limit in this change", "nothing a person sees").
- **Fix what must not ship, record the rest.** Fix before the merge what stops the feature working in ordinary use, what is
  unsafe (crash, hang, data lost or leaked, one app or client harming another, outside input handled unsafely), any untrue
  claim and any test that cannot fail. Everything else goes to Jira NEOC as a Bug or a Story with the area's label, grouped,
  in the backlog, filed when the pull request goes to review, with the keys in its body (REVIEW-READY.md, rule 4.3).
- **No mutation testing.** Judge a test by reading it and the code it covers (CLAUDE.md, "The gate").
- A note is valid only for the SHA it names. Push after posting, and you post a new note.

## Process

1. Merge `origin/main` if it has moved (a merge commit). Regenerate generated files. Push, and record `git rev-parse HEAD`.
2. Run the gate that "The gate" in CLAUDE.md requires for the paths in `git diff --name-only origin/main...HEAD`, at that
   SHA. A skipped lint or a required e2e you did not run is unverified, not passed.
3. Work through the checklist below. For any line you cannot fill with evidence, do the work, push, and restart at 1.
4. Post the note (template below) on the pull request.
5. On a re-request: do the same for the delta since the last reviewed head (`git diff <last-reviewed>..HEAD`), and fill the
   prior-threads table.

## Checklist

Each line is a rule in REVIEW-READY.md. The note carries one row per line.

| # | Rule | Evidence the row must carry |
|---|---|---|
| 0.1 | Who has already solved it | the class of problem; the pattern, standard and mature systems read; adopt/adapt/build and why; any library in the core path cleared with the owner first |
| 0.2 | Five whys on a repeat (re-request) | the five whys and the options sent to the owner through whoever assigned the work, or "no repeat" |
| 1.1 | Reviewer's questions answered | where in the body each one is answered |
| 1.2 | Reference matched | the reference named; each difference and why |
| 1.3 | Invariants for shared resources, no timer decides | each queue, limit, cap and timeout: the invariant and its test; any timer and why |
| 1.4 | Outside input mapped to every sink | the input-to-place table, the routine per target, and the table-driven test |
| 2.1 | Every input gets an answer | a test name per refusal or skip path |
| 2.2 | Errors say what to write instead | the asserted message texts |
| 2.3 | Failure, teardown, twice at once | a test name per failing stage, and the concurrent run |
| 2.4 | Tests call the way the consumer calls | the call forms covered (property and attribute, framework, test-tool click), both sides of each limit |
| 3.1 | Used as its consumer, from the docs | what you built, the commands, which doc lines you fixed |
| 3.2 | Driven on the real host | the `verify-change` record, the states seen, and the baselines opened |
| 3.3 | Each new test can fail | per new test: the assertion that fails without the change, and why |
| 3.4 | Every sentence true at this head | the claims checked and the searches run (`git grep` for the term and stale ids); each negative claim with its search and one known hit it finds |
| 3.5 | Gate green at this head | the command and its summary line, and the head SHA |
| 4.1 | Class fixed, not case (re-request) | per finding: the class, the invariant, the members, the test over them |
| 4.2 | Fix treated as new code (re-request) | section 3 run again on the delta |
| 4.3 | Must-not-ship fixed, the rest ticketed | per finding: the commit, or the Bug/Story key with its area label |

Also, from the reviewer's rubric (`pr-review`, G3, G4 and G11):

- Each acceptance criterion maps to a test name or `file:line`.
- No unrelated change. No `Closes`/`Fixes`. A breaking change has `!` and a `BREAKING CHANGE:` footer.
- `cargo tree -p neoc -p neoc-vm -e all --prefix none | grep '^vos-'` prints nothing.
- For vos paths, the "vos checks" rows apply.

## Self-review note

```
## Self-review — <sha8>

Gate: `<command>` → <summary line> @ <sha8>; `origin/main` <sha8> merged
Diff: <files> files, +<add> / -<del>

| # | Evidence |
|---|---|
| 1.1 | ... |
| ... | ... |
| 3.5 | ... |

### Acceptance criteria
| Criterion | Proof | Result |
|---|---|---|

### Prior threads (re-request only)
| Thread | Class and invariant | Fix SHA | Test |
|---|---|---|---|

Tickets filed (Bug or Story, area label): <NEOC-n — one line each> | none
```

## Don't

- Write a row without evidence, or write PASS in place of evidence.
- Hand off before the gate passes at the head SHA, or push while a reviewer is reading.
- Fix only the case a finding names when it belongs to a class.
- `@mention` attributively: cite a comment by its link. A mention starts a review pass.
