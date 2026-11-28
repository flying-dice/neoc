/**
 * The form a skill is added and changed in, as the page holds it while it is typed in, and what the backend is sent
 * from it. Each file of the form keeps a `key`, so that a file taken out takes its own fields with it.
 */
type Skill = VosApp.Skills.Skill

/** A file of a skill besides its SKILL.md, as the form has it. */
export interface FileDraft {
  key: number
  path: string
  text: string
}

/** The form a skill is added or changed in. `skill` is the one being changed; without it the form adds one. */
export interface SkillDraft {
  skill: Skill | null
  name: string
  text: string
  files: FileDraft[]
}

let next = 1
const key = (): number => next++

export const blankFile = (): FileDraft => ({ key: key(), path: '', text: '' })

/** The form for `skill`, or for a new one. */
export function skillDraftOf(skill: Skill | null): SkillDraft {
  return { skill, name: skill?.name ?? '', text: skill?.text ?? '', files: (skill?.files ?? []).map((f) => ({ key: key(), path: f.path, text: f.text })) }
}

/** What the backend is sent for the form (`_add_skill`; `_update_skill` with the skill's id). A file left blank is not sent. */
export function skillParamsOf(draft: SkillDraft): VosApp.Skills.AddSkillParams {
  return {
    name: draft.name.trim(),
    text: draft.text,
    files: draft.files.filter((f) => f.path.trim() !== '' || f.text !== '').map((f) => ({ path: f.path.trim(), text: f.text }))
  }
}

/** What a skill's row says of its files: its SKILL.md, and how many others. */
export function filesOf(skill: Skill): string {
  const n = skill.files.length
  return n === 0 ? 'SKILL.md' : `SKILL.md and ${n} ${n === 1 ? 'file' : 'files'}`
}
