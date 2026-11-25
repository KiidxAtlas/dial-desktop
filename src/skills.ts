// list_skills currently returns display text, not JSON rows. Only inventory rows are
// actionable; the engine also includes help and empty-state text in these responses.
export function listedSkills(text: string) {
  return text.split("\n").flatMap(line => {
    const row = /^(\S+) \[(on|off)(?:; ([^\]]+))?\] .+?(?: — (.*))?$/.exec(line);
    return row ? [{ name: row[1], on: row[2] === "on", origin: row[3] ?? "", description: row[4] ?? "" }] : [];
  });
}

export interface SkillPackage { name: string; scopes: Set<string>; skills: Set<string> }
export function installedPackages(text: string): SkillPackage[] {
  // Package mutations take a name, not a scope or an individual skill. The same
  // package can be present in both scopes, including with different skills.
  const packages = new Map<string, SkillPackage>();
  for (const line of text.split("\n")) {
    const row = /^(\S+) \[(project|user) scope\] .+ at \S+ — skills: (.*)$/.exec(line);
    if (!row) continue;
    const [, name, scope, skills] = row;
    const pkg = packages.get(name) ?? { name, scopes: new Set<string>(), skills: new Set<string>() };
    pkg.scopes.add(scope);
    for (const skill of skills.split(", ").filter(Boolean)) pkg.skills.add(skill);
    packages.set(name, pkg);
  }
  return [...packages.values()];
}
