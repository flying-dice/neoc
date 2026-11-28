--!strict
-- The Skills app's backend: the skills agents are given, each a SKILL.md and the other files of its folder
-- (skills.luau). It serves on the RPC bus through the global `RPC`, typed by the generated `types/vos.d.luau`.
--
-- The page's methods are private (`_skills`, `_add_skill`, `_update_skill`, `_remove_skill`); each answers with the
-- skills as they now stand, so the window that asked draws what the backend has. Another app reads them with the
-- public `list_skills` (each skill's name and what it is for) and `get_skill` (one skill, with its files), and so can
-- an agent, as tools. Adding, changing and removing a skill are left to the person.
local skills = require("./skills")
local db = app.db

db:migrate {
  [1] = [[
    create table skills (
      id text primary key,
      name text not null unique,
      text text not null,
      files text not null default '[]'
    );
  ]],
}

RPC.on_request("_skills", function(): skills_SkillsResult
  return skills.all()
end)

RPC.on_request("_add_skill", function(params: skills_AddSkillParams, ctx: vos_app_Context): skills_AddSkillResult
  local problem = skills.add(params.name, params.text, params.files)
  if problem then
    return ctx:fail(problem)
  end
  return skills.all()
end)

RPC.on_request(
  "_update_skill",
  function(params: skills_UpdateSkillParams, ctx: vos_app_Context): skills_UpdateSkillResult
    local skill = skills.find(params.skill)
    if not skill then
      return ctx:fail("There is no such skill.")
    end
    local problem = skills.update(skill, params.name, params.text, params.files)
    if problem then
      return ctx:fail(problem)
    end
    return skills.all()
  end
)

RPC.on_request(
  "_remove_skill",
  function(params: skills_RemoveSkillParams, ctx: vos_app_Context): skills_RemoveSkillResult
    local skill = skills.find(params.skill)
    if not skill then
      return ctx:fail("There is no such skill.")
    end
    skills.remove(skill)
    return skills.all()
  end
)

RPC.on_request("list_skills", function(): skills_ListSkillsResult
  local listed: { skills_SkillSummary } = {}
  for _, skill in skills.all() do
    table.insert(listed, { name = skill.name, description = skill.description })
  end
  return { skills = listed }
end)

RPC.on_request("get_skill", function(params: skills_GetSkillParams, ctx: vos_app_Context): skills_GetSkillResult
  local skill = skills.named(params.name)
  if not skill then
    return ctx:fail(`There is no skill named {params.name}.`)
  end
  return { name = skill.name, text = skill.text, files = skill.files }
end)
