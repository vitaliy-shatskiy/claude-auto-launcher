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
// profile has and work does not is left alone. The DECISION of what to change is
// scoped to projects[*].mcpServers and projects[*].disabledMcpServers only.
//
// NOT format-preserving, though: every write here re-serialises the WHOLE target file
// (JSON.parse then JSON.stringify(obj, null, 2)), because a JSON document cannot be edited in
// place - there is no way to touch one path in it without reparsing and rewriting everything
// around it. Measured against a real profile: a 12.7 MB minified file came back 20.4 MB
// pretty-printed, 12345678901234567890 became 12345678901234567000, and 1.0 became 1 - ordinary
// IEEE-754 double round-tripping, not a bug in this script. Anything outside the two paths above
// survives the trip as a JS value, never as the original bytes.
//
// Concurrency: the target is a file another live launcher can be writing to at the same moment
// (this script runs on every account switch, and several launchers run at once here). The write
// is guarded by an mtime+size check taken immediately before the rename: if the target moved
// since it was read, the merge is redone once against the fresh copy; if it moves again, the
// script aborts rather than clobber whatever the other writer just put there.
//
// disabledMcpServers added 2026-08-11. Same per-profile problem, same manual fix: on
// 2026-07-31 three unused Google connectors had to be disabled twice by hand, once per
// profile. Union of the two arrays, so a disable made only in personal survives.
//
// KNOWN AND DELIBERATE: the union can only ever disable MORE. Re-enabling a server in
// personal that work still lists disabled gets undone on the next personal launch -
// remove it from the work list too. Enabling is the default state, so this direction is
// the safe one to automate; the reverse would silently expose servers.
//
// Failure is never silent: a missing file is still a quiet no-op (there is nothing to mirror
// into), but a file that exists and cannot be read, is not valid JSON, or cannot be written back
// prints a one-line reason on stdout and exits non-zero - the caller (Env.ps1) is expected to
// print that reason rather than swallow it, which used to be the exact silent failure this
// script exists to prevent.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

function fail(log, reason) {
  log(`claude-mirror-mcp: ${reason}`);
  return 1;
}

function readJsonFile(fsImpl, file, label) {
  let raw;
  try {
    raw = fsImpl.readFileSync(file, 'utf8');
  } catch (e) {
    throw new Error(`could not read ${label}: ${e.message}`);
  }
  try {
    return JSON.parse(raw);
  } catch (e) {
    throw new Error(`${label} is not valid JSON: ${e.message}`);
  }
}

function statOf(fsImpl, file) {
  try {
    const s = fsImpl.statSync(file);
    return { mtimeMs: s.mtimeMs, size: s.size };
  } catch {
    return null; // vanished between read and check
  }
}

function sameStat(a, b) {
  return !!a && !!b && a.mtimeMs === b.mtimeMs && a.size === b.size;
}

// Pure: mutates `personal` in place, returns the list of human-readable additions.
function mergeProjects(work, personal) {
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
      const currentArr = Array.isArray(current) ? current : [];
      const missing = disabled.filter(n => !currentArr.includes(n));
      // Only written when something is genuinely missing: creating an empty key would
      // rewrite the file on every launch for no gain.
      if (missing.length > 0) {
        personal.projects[dir].disabledMcpServers = [...currentArr, ...missing];
        added.push(`-${missing.join(' -')} @ ${path.basename(dir)}`);
      }
    }
  }
  return added;
}

// The whole run, as a function so a test can inject fsImpl/log and explicit file paths instead
// of touching a real profile. Returns a process exit code; never throws.
export function runMirror({ workFile, personalFile, fsImpl = fs, log = console.log } = {}) {
  if (!fsImpl.existsSync(workFile) || !fsImpl.existsSync(personalFile)) return 0;

  let work;
  try {
    work = readJsonFile(fsImpl, workFile, 'work file');
  } catch (e) {
    return fail(log, e.message);
  }

  function loadTargetAndMerge() {
    const baseline = statOf(fsImpl, personalFile);
    const personal = readJsonFile(fsImpl, personalFile, 'target file');
    const added = mergeProjects(work, personal);
    return { baseline, personal, added };
  }

  let attempt;
  try {
    attempt = loadTargetAndMerge();
  } catch (e) {
    return fail(log, e.message);
  }

  if (attempt.added.length === 0) return 0;

  // Stat immediately before the write, not immediately after the read: the merge itself does no
  // I/O, so this is as close to "right before the rename" as a single-threaded script gets.
  let nowStat = statOf(fsImpl, personalFile);
  if (!sameStat(attempt.baseline, nowStat)) {
    // Something else wrote the target while this merge ran - redo it once against the fresh copy
    // rather than overwrite whatever that write just put there.
    try {
      attempt = loadTargetAndMerge();
    } catch (e) {
      return fail(log, e.message);
    }
    if (attempt.added.length === 0) return 0;
    nowStat = statOf(fsImpl, personalFile);
    if (!sameStat(attempt.baseline, nowStat)) {
      return fail(log, 'target file changed twice during merge - aborting rather than lose a concurrent write');
    }
  }

  // Temp + rename so a crash cannot leave a half-written state file, which Claude Code would
  // read as a corrupt profile.
  const tmp = `${personalFile}.tmp-${process.pid}`;
  try {
    fsImpl.writeFileSync(tmp, JSON.stringify(attempt.personal, null, 2));
    fsImpl.renameSync(tmp, personalFile);
  } catch (e) {
    return fail(log, `could not write target file: ${e.message}`);
  }

  log(attempt.added.join(', '));
  return 0;
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  const workFile = path.join(os.homedir(), '.claude.json');
  const targetRoot = process.argv[2] || path.join(os.homedir(), '.claude-acct2');
  const personalFile = path.join(targetRoot, '.claude.json');
  process.exit(runMirror({ workFile, personalFile }));
}
