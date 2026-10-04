// Mirrors MCP servers from the work profile into a secondary profile: the project-scoped ones
// (additive) and the user-scoped ones (work's definition wins).
//
// USER SCOPE is the top-level mcpServers map. Work is canonical for it: after a run the target
// holds every work server under the work definition - a missing one is added, one defined
// differently is replaced (key order is not a difference). A server only the target has is kept
// and named on the output line at every run, never deleted. Nothing else in the file is decided
// here. The output names servers and never a value from a definition.
//
// The TARGET ROOT is argv[2] (default: the personal profile, which is what every caller passed
// before the third account arrived on 2026-08-20). Hardcoding it was correct while there was
// exactly one secondary profile and became a silent bug the moment there were two: a session on
// the third account would lose its per-project servers - the exact failure this script exists to prevent.
// "personal" in the comments below therefore means "the target profile".
//
// User-scope MCP reaches a session through TWO channels: the launcher's --mcp-config
// (mcp-shared.json + the dynamic Rider port), and the top-level mcpServers of .claude.json, which
// `claude mcp add -s user` writes. PROJECT-scope servers live in .claude.json under
// projects[<dir>].mcpServers. That file is the one thing the profiles cannot share, because it
// also holds the account identity, so both of its MCP parts are mirrored here. Without this the
// secondary profile silently loses servers: one registered in the canonical profile was simply
// absent in the secondary one. Which channel SHOULD define a given server is not decided here.
//
// One direction only (work is canonical). The DECISION of what to change is scoped to the
// top-level mcpServers, projects[*].mcpServers and projects[*].disabledMcpServers only; project
// scope is additive (a server the personal profile has and work does not is left alone).
//
// Credentials travel with the server: a copy is verbatim, `env` (stdio) and `headers` (http/sse)
// included, because the accounts are one person's on one machine and should not differ.
// CLAUDE_AUTO_MIRROR_SECRETS=0 is the only way to strip those two fields (unset or any other value
// copies them); CLAUDE_AUTO_MIRROR_EXCLUDE (comma-separated server names) is never copied at all.
// Project scope is additive - a server already in the target is not updated - so there either
// switch affects only servers not mirrored yet. In the user scope an excluded name is never
// touched, and with SECRETS=0 the work definition is taken without those two fields while the
// ones the target already holds for that server stay.
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
    // Never the engine's message: V8 quotes the text around a bad token, and this file holds
    // tokens. The position alone.
    const at = /position (\d+)/.exec(e.message ?? '');
    throw new Error(`${label} is not valid JSON${at ? ` (at position ${at[1]})` : ''}`);
  }
}

// A name that would reach an object's prototype on assignment.
const UNSAFE_NAMES = ['__proto__', 'constructor', 'prototype'];

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

// The fields that carry a server's credentials: `env` (stdio) and `headers` (http/sse).
const SECRET_FIELDS = ['env', 'headers'];

// Key order is serialisation, not configuration: compare with keys sorted at every level.
function canonical(node) {
  if (Array.isArray(node)) return `[${node.map(canonical).join(',')}]`;
  if (node && typeof node === 'object') {
    return `{${Object.keys(node).sort().map(k => `${JSON.stringify(k)}:${canonical(node[k])}`).join(',')}}`;
  }
  return JSON.stringify(node) ?? 'null';
}

const isMap = (v) => !!v && typeof v === 'object' && !Array.isArray(v);

function withoutSecrets(cfg) {
  if (!isMap(cfg)) return cfg;
  const copy = { ...cfg };
  for (const field of SECRET_FIELDS) delete copy[field];
  return copy;
}

// Pure: mutates `personal.mcpServers` in place. `changed` are the edits that need a write,
// `notes` what is reported without one. Names only in both.
function mergeUserServers(work, personal, { exclude = [], copySecrets = true } = {}) {
  const changed = [];
  const notes = [];
  const servers = work.mcpServers;
  if (!isMap(servers)) return { changed, notes };
  const current = isMap(personal.mcpServers) ? personal.mcpServers : {};
  for (const [name, cfg] of Object.entries(servers)) {
    if (exclude.includes(name)) continue;
    if (UNSAFE_NAMES.includes(name)) { notes.push(`${name} @ user (unsafe name, skipped)`); continue; }
    const has = Object.hasOwn(current, name);
    let next = cfg;
    if (copySecrets) {
      if (has && canonical(current[name]) === canonical(cfg)) continue;
    } else {
      // Compared and written without the credential fields; the target's own ones are carried over.
      next = withoutSecrets(cfg);
      if (has && canonical(withoutSecrets(current[name])) === canonical(next)) continue;
      if (has && isMap(next) && isMap(current[name])) {
        for (const field of SECRET_FIELDS) if (field in current[name]) next[field] = current[name][field];
      }
    }
    // Assigned only now, so a run with nothing to change never creates an empty map.
    personal.mcpServers = current;
    current[name] = next;
    changed.push(`${name} @ user${has ? ' (updated)' : ''}`);
  }
  for (const name of Object.keys(current)) {
    if (!Object.hasOwn(servers, name) && !exclude.includes(name) && !UNSAFE_NAMES.includes(name)) notes.push(`${name} @ user (only in the target, kept)`);
  }
  return { changed, notes };
}

// Pure: mutates `personal` in place, returns the list of human-readable additions.
function mergeProjects(work, personal, { exclude = [], copySecrets = true } = {}) {
  const added = [];
  for (const [dir, wp] of Object.entries(work.projects ?? {})) {
    if (UNSAFE_NAMES.includes(dir)) continue;
    const servers = wp?.mcpServers;
    if (servers && Object.keys(servers).length > 0) {
      personal.projects ??= {};
      personal.projects[dir] ??= {};
      const target = (personal.projects[dir].mcpServers ??= {});
      for (const [name, cfg] of Object.entries(servers)) {
        if (name in target || exclude.includes(name) || UNSAFE_NAMES.includes(name)) continue;
        const dropped = [];
        let copy = cfg;
        if (!copySecrets && cfg && typeof cfg === 'object') {
          copy = { ...cfg };
          for (const field of SECRET_FIELDS) {
            if (field in copy) { delete copy[field]; dropped.push(field); }
          }
        }
        target[name] = copy;
        added.push(`${name} @ ${path.basename(dir)}${dropped.length ? ` (${dropped.join(', ')} not copied)` : ''}`);
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
export function runMirror({ workFile, personalFile, fsImpl = fs, log = console.log, exclude = [], copySecrets = true } = {}) {
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
    const user = mergeUserServers(work, personal, { exclude, copySecrets });
    const added = [...user.changed, ...mergeProjects(work, personal, { exclude, copySecrets })];
    return { baseline, personal, added, notes: user.notes };
  }

  let attempt;
  try {
    attempt = loadTargetAndMerge();
  } catch (e) {
    return fail(log, e.message);
  }

  // Nothing to write. A target-only user-scope server is still named, at every run, until the
  // owner resolves it.
  const reportOnly = (a) => { if (a.notes.length > 0) log(a.notes.join(', ')); return 0; };
  if (attempt.added.length === 0) return reportOnly(attempt);

  // Temp + rename so a crash cannot leave a half-written state file, which Claude Code would
  // read as a corrupt profile. The target is stat'ed before the tmp write AND after it, right
  // before the rename: serialising a large file takes long enough for a session to write in
  // between, and a check taken only before the tmp write would rename over that write.
  const tmp = `${personalFile}.tmp-${process.pid}`;
  const dropTmp = () => { try { fsImpl.unlinkSync(tmp); } catch { /* never written, or gone */ } };
  function writeIfUnchanged(a) {
    if (!sameStat(a.baseline, statOf(fsImpl, personalFile))) return false;
    fsImpl.writeFileSync(tmp, JSON.stringify(a.personal, null, 2));
    if (!sameStat(a.baseline, statOf(fsImpl, personalFile))) { dropTmp(); return false; }
    fsImpl.renameSync(tmp, personalFile);
    return true;
  }

  let written;
  try {
    written = writeIfUnchanged(attempt);
  } catch (e) {
    dropTmp();
    return fail(log, `could not write target file: ${e.message}`);
  }
  if (!written) {
    // Something else wrote the target while this merge ran - redo it once against the fresh copy
    // rather than overwrite whatever that write just put there.
    try {
      attempt = loadTargetAndMerge();
    } catch (e) {
      return fail(log, e.message);
    }
    if (attempt.added.length === 0) return reportOnly(attempt);
    try {
      written = writeIfUnchanged(attempt);
    } catch (e) {
      dropTmp();
      return fail(log, `could not write target file: ${e.message}`);
    }
    if (!written) {
      return fail(log, 'target file changed twice during merge - aborting rather than lose a concurrent write');
    }
  }

  log([...attempt.added, ...attempt.notes].join(', '));
  return 0;
}

const isMain = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isMain) {
  const workFile = path.join(os.homedir(), '.claude.json');
  const targetRoot = process.argv[2] || path.join(os.homedir(), '.claude-acct2');
  const personalFile = path.join(targetRoot, '.claude.json');
  const exclude = (process.env.CLAUDE_AUTO_MIRROR_EXCLUDE ?? '').split(',').map(s => s.trim()).filter(Boolean);
  const copySecrets = process.env.CLAUDE_AUTO_MIRROR_SECRETS !== '0';
  process.exit(runMirror({ workFile, personalFile, exclude, copySecrets }));
}
