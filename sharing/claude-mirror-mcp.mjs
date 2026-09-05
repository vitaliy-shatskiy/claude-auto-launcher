// Mirrors project-scoped MCP servers from the work profile into a secondary profile.
//
// The TARGET ROOT is argv[2] (default: the personal profile, which is what every caller passed
// before the third account arrived on 2026-08-20). Hardcoding it was correct while there was
// exactly one secondary profile and became a silent bug the moment there were two: a session on
// the third account would lose its per-project servers - the exact failure this script exists to prevent.
// "personal" in the comments below therefore means "the target profile".
//
// User-scope MCP now comes from --mcp-config (mcp-shared.json + the dynamic Rider
// port), but PROJECT-scope servers live in .claude.json under
// projects[<dir>].mcpServers - and that file is the one thing the two profiles
// cannot share, because it also holds the account identity. Without this the
// secondary profile silently loses per-project servers: a repository-scoped server
// registered in the canonical profile was simply absent in the secondary one.
//
// One direction only (work is canonical) and additive: a server the personal
// profile has and work does not is left alone. Only the top-level identity keys
// are never touched here - this writes nothing outside projects[*].mcpServers
// and projects[*].disabledMcpServers.
//
// disabledMcpServers added 2026-08-11. Same per-profile problem, same manual fix: on
// 2026-07-31 three unused Google connectors had to be disabled twice by hand, once per
// profile. Union of the two arrays, so a disable made only in personal survives.
//
// KNOWN AND DELIBERATE: the union can only ever disable MORE. Re-enabling a server in
// personal that work still lists disabled gets undone on the next personal launch -
// remove it from the work list too. Enabling is the default state, so this direction is
// the safe one to automate; the reverse would silently expose servers.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const workFile = path.join(os.homedir(), '.claude.json');
const targetRoot = process.argv[2] || path.join(os.homedir(), '.claude-acct2');
const personalFile = path.join(targetRoot, '.claude.json');
if (!fs.existsSync(workFile) || !fs.existsSync(personalFile)) process.exit(0);

const work = JSON.parse(fs.readFileSync(workFile, 'utf8'));
const personal = JSON.parse(fs.readFileSync(personalFile, 'utf8'));

const added = [];
for (const [dir, wp] of Object.entries(work.projects ?? {})) {
  const servers = wp?.mcpServers;
  if (servers && Object.keys(servers).length > 0) {
    personal.projects ??= {};
    personal.projects[dir] ??= {};
    const target = (personal.projects[dir].mcpServers ??= {});
    for (const [name, cfg] of Object.entries(servers)) {
      if (name in target) continue;
      target[name] = cfg;
      added.push(`${name} @ ${path.basename(dir)}`);
    }
  }

  // Array, not an object map - so union by value rather than by key.
  const disabled = wp?.disabledMcpServers;
  if (Array.isArray(disabled) && disabled.length > 0) {
    personal.projects ??= {};
    personal.projects[dir] ??= {};
    const current = personal.projects[dir].disabledMcpServers;
    const target = Array.isArray(current) ? current : [];
    const missing = disabled.filter(n => !target.includes(n));
    // Only written when something is genuinely missing: creating an empty key would
    // rewrite the file on every launch for no gain.
    if (missing.length > 0) {
      personal.projects[dir].disabledMcpServers = [...target, ...missing];
      added.push(`-${missing.join(' -')} @ ${path.basename(dir)}`);
    }
  }
}

if (added.length === 0) process.exit(0);

// Temp + rename so a crash cannot leave a half-written state file, which Claude
// Code would read as a corrupt profile.
const tmp = `${personalFile}.tmp-${process.pid}`;
fs.writeFileSync(tmp, JSON.stringify(personal, null, 2));
fs.renameSync(tmp, personalFile);
console.log(added.join(', '));
