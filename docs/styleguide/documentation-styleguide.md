# Documentation Style Guide

Standards for all platform documentation. Modeled on MDN patterns.

---

## 1. Information architecture

Three-tier hierarchy. Every page belongs to exactly one tier.

### Tier 1: Landing pages

Navigational hub. No teaching, no deep explanations.

Contains:
- One-to-two paragraph overview
- Categorized link list with one-sentence descriptions
- No code examples

### Tier 2: Guides

Teaches a concept, workflow, or technique. Narrative, sequential. Prose is primary content; code examples support.

### Tier 3: Reference pages

Documents a single discrete item: endpoint, header, method, status code, config option. Structured, predictable, scannable.

### URL structure

```
/docs/{topic}/                          → Landing page
/docs/{topic}/guides/{guide-slug}       → Guide
/docs/{topic}/reference/{type}/{item}   → Reference page
```

Lowercase kebab-case. No version numbers, dates, or author names in URLs.

---

## 2. Page templates

### 2.1 Landing page

```
# {Topic}: {Subtitle}

{One to two paragraphs: scope and what reader will find.}

## Guides

{Guide Title}
:   One-sentence description.

## Reference

{Category}
:   - `Item` — one-line summary
```

### 2.2 Guide page

```
# {Guide Title}

{Opening: what it covers, who it's for, prerequisites. Link prereqs.}

## {Conceptual Section}

{Explanatory prose. Define terms before use.}

### {Subsection with code}

{Introduce example: what it demonstrates.}

```language
{code}
```

{Explain result. Call out non-obvious behavior.}

## See also

- {Related links}
```

### 2.3 Reference page

Rigid template. All pages in same category use identical section ordering.

```
# {Item Name}

{Status badge: Stable | Experimental | Deprecated}

{One to three sentences: what it is, what it does. Bold item name first mention.}

|Key|Value|
|---|-----|
|{field}|{value}|

## Syntax

```language
{Formal syntax or signature}
```

## Parameters

`param`
:   {Type.} {Description. Default. Required/optional.}

## Description

{Behavior, edge cases, interactions. Optional if opening + params suffice.}

## Examples

### {Scenario}

{One sentence intro.}

```language
{code}
```

{Output/result explanation.}

## Specifications

|Specification|Status|
|---|---|
|{name + link}|{Standard/Draft}|

## Compatibility

{Platform/version support.}

## See also

- {Related links}
```

Section order is fixed: definition → metadata → Syntax → Parameters → Description → Examples → Specifications → Compatibility → See Also. Omit empty sections entirely.

---

## 3. Voice and language

### Voice

Neutral, declarative, third-person. Subject = the thing documented, not reader or author.

**Correct:** "The `Cache-Control` header holds directives that control caching."

**Incorrect:** "You can use the `Cache-Control` header to control caching."

Guides may use "you" sparingly for direct instructions. Reference pages avoid "you" entirely.

### Tone

Professional, precise. Contractions acceptable. No humor, rhetorical questions, exclamation marks, colloquialisms, editorializing, or unnecessary hedging.

### The three Cs

- **Clear.** Active voice. Unambiguous pronouns. Short sentences. One idea per sentence.
- **Concise.** Say what needs saying. Stop.
- **Consistent.** Same term for same concept everywhere.

### Introductions

Every page opens with a self-contained summary paragraph.

Reference page opening pattern:
> **The {category} `{item name}`** {verb phrase defining what it does}.

---

## 4. Structure rules

### Headings

- Markdown `#` through `####`. No skipped levels.
- Sentence case. Exception: proper nouns, technical names.
- Specific and descriptive. No vague "Overview" or "Details."
- No inline code in H1. Allowed in H2+.

### Paragraphs

- Two to five sentences. Single point per paragraph.

### Definition lists

Use for parameters, directives, glossary terms:

```markdown
`max-age`
:   Maximum time in seconds a response is fresh.

`no-cache`
:   Forces validation with origin before serving cached copy.
```

### Tables

Structured data with parallel attributes. Header row required. Left-align text, right-align numbers.

---

## 5. Formatting

### Inline code

Wrap: method names, function names, endpoint paths, parameter names, header names, status codes, file paths, env vars, literal values, CLI input.

Do not wrap: general concepts in prose, product/protocol names.

### Bold

Only for: item name first mention in reference opening, notice labels, table headers.

### Italics

Only for: introducing new terms first time, external document titles.

### Numbers

- Spell out 1–9 in prose. Digits for 10+.
- Digits for measurements, versions, technical values.
- Serial comma.

---

## 6. Code examples

### Principles

1. **Complete** — runnable or understandable in isolation.
2. **Minimal** — only code needed for the point.
3. **Sparse comments** — only for surprising behavior.

### Structure

1. `###` heading describing what example demonstrates
2. One-two sentence intro
3. Code block with language identifier
4. One-three sentence result explanation

### Language identifiers

Always specified: `http`, `json`, `javascript`, `typescript`, `python`, `bash`, `yaml`, `xml`, `html`, `css`, `sql`, `plaintext`, `diff`.

### Diff annotations

`+` = proposed/added. `-` = deprecated/removed. ` ` (space) = unchanged context.

Rules:
- Introduce diff with sentence explaining the transition
- Include enough unchanged context
- Do not mix with inline comments

### Placeholder values

Descriptive, obviously fake: `https://api.example.com`, `abc123`, `usr_12345`.

---

## 7. Linking

### Internal

- Link terms to reference page on first mention per section. Do not re-link.
- Item name as link text. Never "click here."
- Link to most specific page.

### External

- Authoritative sources only: specs, official docs.
- No forums, aggregator blogs, social media.

### See also

Every page ends with 3–8 links: related references, concept guide, external spec.

---

## 8. Notices and badges

### Status badges

Every reference page displays one below H1: **Stable** | **Experimental** | **Deprecated** | **Non-standard**.

### Inline notices

Three types only:

- **Note:** — helpful but not critical
- **Warning:** — pitfalls, data loss, security risks
- **Deprecated:** — feature no longer recommended, state replacement

No other types. Never stack two notices in a row.

---

## 9. Naming and terminology

### Capitalization

- Headings: sentence case
- Product names: official casing
- Protocol names: uppercase acronyms (HTTP, TLS, OAuth)
- General terms: lowercase ("the server," "a response header")

### Preferred wording

- "parameters" not "arguments"
- "invoke"/"call" not "hit"/"ping"
- "returns" not "gives back"
- "indicates"/"specifies" not "tells"

---

## 10. Inclusive language

|Avoid|Use|
|---|---|
|master/slave|primary/replica|
|whitelist/blacklist|allowlist/denylist|
|sanity check|validation|
|dummy value|placeholder value|

Gender-neutral pronouns. Singular "they" preferred. Better: restructure to eliminate pronouns.

No directional language ("below," "to the right"). Refer by section name. Descriptive link text always.

---

## 11. SEO

- Title: `{Item} — {Category}`. Most specific term first.
- Meta description: 120–160 chars, complete sentence.
- Opening paragraph works as search snippet standalone.
- Heading anchors: lowercase, hyphenated.

---

## 12. Review checklist

**Structure:**
- [ ] Conforms to template (Landing/Guide/Reference)
- [ ] Sections in prescribed order
- [ ] "See Also" present with 3–8 links

**Content:**
- [ ] Opening defines topic in 1–4 sentences
- [ ] Terms linked on first mention per section
- [ ] No broken links
- [ ] Code examples complete, minimal, language-tagged

**Voice:**
- [ ] Third-person declarative (reference) or limited second-person (guides)
- [ ] No humor, exclamations, editorializing
- [ ] Inclusive terminology

**Formatting:**
- [ ] Inline code on all technical names
- [ ] Bold only for first-mention + notice labels
- [ ] Status badge on reference pages
