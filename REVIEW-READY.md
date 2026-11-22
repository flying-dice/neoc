# Review-ready

What to do while you build a change so that the independent review finds nothing you could have found first. It is short on
purpose: read it when you start a change, and again before you open the pull request and before every push that answers a
review. The `pr-self-review` skill turns it into the note you post; each rule below says what that note must show.

Every rule comes from a finding that cost a review round on pull requests #52 to #61; its **Cost** line names one.
"Evidence" is a test name, a command and its result, or what you saw on the real host. "Checked" is not evidence. A
reviewer uses your change the way its consumer will, gives it hostile input, breaks it half way and checks every sentence
you wrote: do those things first.

## 0. Before anything else

Nothing in this product is novel. A sandboxed frame talking to its host, a kit of form controls, a generated API client, a
queue for a slow consumer, validating input: each is a solved class of problem, with libraries, specifications and written
patterns behind it. Don't plug holes one at a time.

**0.1 Before you design, find who has already solved it.** Name the class of problem. Name the established solutions:
libraries, the protocol or specification, how two or three mature systems do it. Read which edge cases they handle; those
are the review findings you would otherwise meet one at a time. Then choose adopt, adapt or build, and give the reason.
Adopting is the default, and building needs the justification. A new dependency needs the owner's sign-off: raise it early
with your recommendation, and don't build by hand to avoid asking.
*Evidence:* the class, the solutions you read, and the choice with its reason, in your first report and in the body.
*Cost:* #58 re-implemented what native form controls and the shell's `Button` already do, and took six rounds over forms,
disabled states, touch targets and clicks.

**0.2 When the same kind of finding comes back a second time, stop and ask why five times.** Don't patch the case. Go up
past the implementation: why does this design make the mistake possible, and why are we maintaining this ourselves? Write
the five whys down. Take them to the coordinator and the owner as a question with options, before the next push. A repeat
means something is missing a layer above your implementation choices.
*Evidence:* the five whys and the options, linked from the reply. *Cost:* one app starved the shell's shared connection
three times in three forms on #56. Generated names and the output folder came back in each of #57's three rounds.

## 1. When you design

**1.1 Answer the reviewer's questions before they ask.** If you were given the questions your reviewer will get, answer each
one in the pull request body. If not, the sections of this file are the questions.
*Evidence:* one answer per question in the body. *Cost:* #61 answered each note of #59's final review and passed in one round.

**1.2 Put the reference next to your design.** Name what the change must match: the shell's own component, the native
element, the Apple framework, the standard library, the specification. List its names, defaults, sizes, states and opt-ins,
and match them. Where you differ, say why.
*Evidence:* the reference, and where you differ, in the body. *Cost:* #58, round 1: six findings where the kit differed from
native form controls or the shell's `Button`; round 3: 44 pt targets always on where the shell's are opt-in.

**1.3 Write each invariant down before the code.** For every queue, limit, permit, cap and timeout: who can fill it, what
everyone else sees when it is full, and which side frees a slot (if both sides count, they must agree when). Bound memory
in bytes, not items. Never let a timer decide what is true: wait on an event, a handshake or a condition.
*Evidence:* each invariant and its test; any timer and why it must be one. *Cost:* #56: the shell's shared connection was
starved three times (an oversize message, 64 calls in flight, a slot freed by a timeout while the host still held it), and
a 1.5 s timer declared a busy app gone. #60: a queue bounded at 512 messages of up to 1 MiB each.

**1.4 Map every outside input to every place it is written.** List each value that crosses a trust boundary: an app's
contract or manifest, a title or body, a name, a file path, a header, an option table, a message from a frame, a colour. For
each, list every place it is written: generated code, a comment, a path, a style, an attribute, a log line, an error sent to
a client. Give each target one escaping or checking routine, and use it everywhere. Take that routine from the platform or a
standard library: `JSON.stringify`, the path API with a containment check, `style.setProperty`, Unicode properties. Never a
hand-kept list or string concatenation. A name that becomes an identifier must be legal in every language it reaches, and it
must not shadow a reserved word, a built-in or a name the generator uses.
*Evidence:* the input-to-place table, the routine for each, and one table-driven test of awkward values at every place
(below). *Cost:* #57: a contract title injected code into the generated client, a migration path read outside the project,
and generated names (`new`, `Record`, `buffer`) broke the output in two rounds. #58: a colour string injected CSS.

What the repository uses today: on the host, `serde` types with `deny_unknown_fields` for wire and declaration shapes, and
newtypes with one checking constructor (`AppId::parse`). For a Luau option table, the adapter or module has its own
unknown-key check. In the client, `zod` parses what arrives. Shell text from an app is cleaned in
`client/src/shell/notificationText.ts`. Extend the one that fits, and don't write another. A standard approach to guarding
input by default is being introduced under its own story. Until it lands on `main`, this paragraph describes what exists.

## 2. While you build

**2.1 Every input gets an answer.** Each call returns a result, an error or a log line. Nothing is dropped, skipped or
ignored without a trace, and every promise settles. A missing tool, a skipped check or an unknown option fails the operation
and says so.
*Evidence:* a test per refusal that asserts the answer. *Cost:* #56: a rejected call never settled. #57: the build passed
without `luau-lsp`, then `check: false` passed silently. #55 and #59 dropped headers and a sender without a word.

**2.2 An error says what was wrong and what to write instead,** and it names the real cause. It carries no host path, stack
or memory address.
*Evidence:* the message text asserted in a test. *Cost:* #52 and #56 each took two rounds over messages: "unknown key" for
a positional entry; a load error reported as a missing `mount`, then as "Script error.".

**2.3 Walk the failure and teardown paths.** Make each step fail, or panic, at each stage. The previous good state must
survive, or nothing must change. Run the operation twice at once, and while the app is uninstalled, reloaded or replaced or
the client reconnects. When something is removed, decide where focus goes, who is told and what is released.
*Evidence:* a test per stage, and one with two runs at once. *Cost:* #57 took three rounds: a failed build overwrote the last
good output, a later failure still did, then two builds collided. #59: focus dropped to the page when a banner went.

**2.4 Test the way the consumer calls, on both sides of every boundary.** Frameworks set properties, not attributes. Test
tools click the centre of an element, and `locator.click()`, `userEvent.click()`, `fireEvent.click()` and `el.click()` are
four different calls. To test a limit, hit the limit and one past it, not half of it. To test a cap per caller, end on a
count that would differ if the cap were shared.
*Evidence:* the test names. *Cost:* #58: forms submitted nothing from React and Svelte, because the tests set `name` as an
attribute only. #60: a burst test sent half the bound. #52: a test landed exactly on 64.

## 3. Before you open the pull request

**3.1 Use it as its consumer, from nothing.** Follow only your own docs and build the smallest real consumer: an app, a
script, a page. Run every example in the docs and type-check it. Each place where the docs were wrong or missing is a defect.
*Evidence:* what you built, the commands, and what you fixed in the docs. *Cost:* #57: the README's start command pointed
at a missing file, and step 1 did not build. #54: the front-page example failed strict types.

**3.2 Drive anything a person sees on the real host** (`verify-change`). Use the keyboard only, light and dark, and the
minimum window. Leave the tab in the background, and park the pointer on the change. Open every new or changed baseline
image: a baseline that matches another, or is blank, proves nothing.
*Evidence:* the "Manual verification" record and the images you looked at. *Cost:* #59: banners timed out in a hidden tab
and never expired under a resting pointer, and seven new stories had the same picture as another.

**3.3 Say why each new test can fail.** By reading, name the assertion that would fail without your change, and the reason.
No mutation testing: don't edit the code or revert the fix to watch the test fail (see "The gate" in CLAUDE.md).
*Evidence:* one line per new test. *Cost:* #53: a test passed even with the leak it guarded. #56: the sandbox probes threw
in both cases, so they could not fail, and that took two rounds.

**3.4 Make every sentence true at the final head.** That covers docs, comments, error text, test names, the body and the
breaking note. Check each claim against the code as it now is. Search for every other place the same claim is made, and for
stale ids. The body lists everything that changed, including each changed screenshot and each fix made on the way, and which
checks ran at which commit.
*Evidence:* the claims you checked and the searches you ran. *Cost:* untrue sentences were the largest class, about one
finding in four. #56 said "the frame has no network" and "nothing for existing apps", and left a stale journey id. #52
corrected the cap wording in one place and left it in two.

**3.5 Leave the reviewer nothing mechanical.** Merge `main`, then run the gate that "The gate" in CLAUDE.md requires, at the
exact head you push. A new package is covered by lint and format. Push once per round, and never while a reviewer is reading.
*Evidence:* the gate command and its result at the head SHA. *Cost:* #58: the new kit was outside eslint and prettier.

## 4. When you answer a review

**4.1 Fix the class, not the case.** When a finding arrives, name the class it belongs to and write the invariant it breaks.
Examples: "a tap inside a control's drawn box always activates that control"; "a contract name is legal at every position
it is written"; "a failed build changes no byte of the output". List every member of the class. Then fix them all, and test
the invariant over a table of cases (control by layout by way of clicking; name by position by language), in one push. If
the same kind of finding has come back, do 0.2 first.
*Evidence:* in the reply, the class, the invariant, the members and the test. *Cost:* in rounds after the first, 21 of 29
findings were the same mechanism again; #58's hit areas took five more rounds, each fix exposing the next.

**4.2 Treat your fix as new code.** Run section 3 again on the delta: the consumer's calls, the class's table and the
sentences the fix changed. The reviewer will.
*Evidence:* the self-review note for the new head. *Cost:* #58, rounds 5 and 6: `pointer-events: none`, added to fix round
4, broke `locator.click()` and then `userEvent.click()`.

**4.3 Fix everything raised, in this pull request.** That includes non-blocking notes, docs and wording: there are no
follow-up tickets. A bug in unrelated code gets one Bug ticket, with file, line and how to reproduce it. Reply on each thread
with the commit that fixes it.
*Evidence:* one reply per thread, and the prior-threads table in the note. *Cost:* the notes left open on #59 at its merge
needed a pull request of their own (#61) and another review.

## What this does not replace

A reviewer who drives the real host will still find things nobody thought of, and make design calls differently. This file
removes the findings you could have made yourself; the gate, the skills and CLAUDE.md still apply.
