// Assertions for sharing/claude-mirror-mcp.mjs. Invoked by Test-Mirror.ps1 - never run this
// against a real profile. Every fixture lives under the OS temp directory, and HOME / USERPROFILE
// are redirected there for the one test that exercises the real CLI entry point.
//
// Contract mirrors every Test-*.ps1 suite in this repo: 0 pass, 1 a real failure, 2 could not run
// (an uncaught exception - a missing module, a broken import). Two is never a pass.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const mirrorScript = path.join(__dirname, '..', 'sharing', 'claude-mirror-mcp.mjs');
const { runMirror } = await import(pathToFileURL(mirrorScript).href);

let Ran = 0;
let Failed = 0;
function assertEqual(expected, actual, because) {
  Ran++;
  const e = String(expected);
  const a = String(actual);
  if (e !== a) {
    console.log(`FAIL  ${because}`);
    console.log(`      expected: ${e}`);
    console.log(`      actual:   ${a}`);
    Failed++;
  } else {
    console.log(`ok    ${because}`);
  }
}

function freshDir(name) {
  return fs.mkdtempSync(path.join(os.tmpdir(), `claude-mirror-test-${name}-`));
}
function writeJson(file, obj) {
  fs.writeFileSync(file, JSON.stringify(obj, null, 2));
}

// ---------------------------------------------------------------- additive merge, then a no-op

{
  const home = freshDir('additive-home');
  const target = freshDir('additive-target');
  const workFile = path.join(home, '.claude.json');
  const personalFile = path.join(target, '.claude.json');
  writeJson(workFile, {
    projects: {
      '/repo/one': {
        mcpServers: { alpha: { command: 'alpha-cmd' } },
        disabledMcpServers: ['legacy-google'],
      },
    },
  });
  writeJson(personalFile, { projects: { '/repo/one': { mcpServers: {} } } });

  const logs = [];
  const code = runMirror({ workFile, personalFile, log: (m) => logs.push(m) });
  assertEqual(0, code, 'an additive merge exits 0');
  const after = JSON.parse(fs.readFileSync(personalFile, 'utf8'));
  assertEqual('alpha-cmd', after.projects['/repo/one'].mcpServers.alpha.command, 'a server missing from the target is added');
  assertEqual(true, Array.isArray(after.projects['/repo/one'].disabledMcpServers) && after.projects['/repo/one'].disabledMcpServers.includes('legacy-google'), 'a disabled server is unioned in too');
  assertEqual(true, logs.length === 1 && logs[0].includes('alpha'), 'the addition is reported on the one log line');

  const statBefore = fs.statSync(personalFile);
  const logs2 = [];
  const code2 = runMirror({ workFile, personalFile, log: (m) => logs2.push(m) });
  assertEqual(0, code2, 'a second run with nothing new still exits 0');
  assertEqual(0, logs2.length, 'a second run logs nothing - there is nothing to report');
  const statAfter = fs.statSync(personalFile);
  assertEqual(String(statBefore.mtimeMs), String(statAfter.mtimeMs), 'a second run does not touch the file at all - a true no-op, not just an empty diff');
}

// ---------------------------------------------------------------- malformed JSON

{
  const home = freshDir('malformed-home');
  const target = freshDir('malformed-target');
  const workFile = path.join(home, '.claude.json');
  const personalFile = path.join(target, '.claude.json');
  writeJson(workFile, { projects: { '/repo/two': { mcpServers: { beta: {} } } } });
  fs.writeFileSync(personalFile, '{ this is not json');

  const logs = [];
  const code = runMirror({ workFile, personalFile, log: (m) => logs.push(m) });
  assertEqual(1, code, 'malformed target JSON exits non-zero rather than throwing');
  assertEqual(true, logs.length === 1 && /not valid JSON/.test(logs[0]), 'and reports a one-line reason naming the problem');

  const target2 = freshDir('malformed-work-target');
  const workFile2 = path.join(home, 'work2.claude.json');
  const personalFile2 = path.join(target2, '.claude.json');
  fs.writeFileSync(workFile2, 'not json at all');
  writeJson(personalFile2, { projects: {} });
  const logs3 = [];
  const code3 = runMirror({ workFile: workFile2, personalFile: personalFile2, log: (m) => logs3.push(m) });
  assertEqual(1, code3, 'a malformed WORK file also exits non-zero');
  assertEqual(true, logs3.length === 1 && /not valid JSON/.test(logs3[0]), 'and also reports a one-line reason');
}

// ---------------------------------------------------------------- a changed target is refused

{
  // Real fixture files, real fs, and a statSync wrapper that - as a SIDE EFFECT of being asked for
  // the target's stat - performs one more concurrent-looking write to the same file. This
  // exercises the actual mtime+size guard against a genuine file mutation, not a mocked value.
  const home = freshDir('race-home');
  const target = freshDir('race-target');
  const workFile = path.join(home, '.claude.json');
  const personalFile = path.join(target, '.claude.json');
  writeJson(workFile, { projects: { '/repo/three': { mcpServers: { gamma: {} } } } });

  function withConcurrentWriterOnStat(mutateOnCalls) {
    let n = 0;
    return {
      ...fs,
      statSync: (p) => {
        n++;
        if (mutateOnCalls.includes(n)) {
          const current = JSON.parse(fs.readFileSync(p, 'utf8'));
          // Length varies with n on purpose: two mutations whose JSON happens to serialise to the
          // same byte length could land on the same size (and, on a coarse filesystem clock, even
          // the same mtime) and the guard would miss a real second conflict. A clearly different
          // length makes the size half of the check unambiguous regardless of clock granularity.
          current.someUnrelatedKeyWrittenByAnotherLauncher = `concurrent-write-${n}-` + 'x'.repeat(n * 40);
          fs.writeFileSync(p, JSON.stringify(current, null, 2));
        }
        return fs.statSync(p);
      },
    };
  }

  // One conflict (stat calls: 1=baseline, 2=pre-write check -> mutate here, 3=reload baseline,
  // 4=second pre-write check -> no further mutation): the merge must be redone once and SUCCEED,
  // carrying forward the concurrent writer's own key.
  writeJson(personalFile, { projects: {} });
  const logsSingle = [];
  const codeSingle = runMirror({ workFile, personalFile, fsImpl: withConcurrentWriterOnStat([2]), log: (m) => logsSingle.push(m) });
  assertEqual(0, codeSingle, 'a target that changes ONCE during the merge is retried and still succeeds');
  const afterSingle = JSON.parse(fs.readFileSync(personalFile, 'utf8'));
  assertEqual(true, `${afterSingle.someUnrelatedKeyWrittenByAnotherLauncher}`.startsWith('concurrent-write-2-'), 'the retried merge is built on the concurrent write, not on the stale copy');
  assertEqual(true, !!afterSingle.projects?.['/repo/three']?.mcpServers?.gamma, 'and still carries the merge this run was asked to make');

  // Two conflicts in a row (every pre-write check sees a further change): the script must abort
  // rather than guess which version to keep, and must NOT write its own merge on top.
  writeJson(personalFile, { projects: {} });
  const logsDouble = [];
  const codeDouble = runMirror({ workFile, personalFile, fsImpl: withConcurrentWriterOnStat([2, 4]), log: (m) => logsDouble.push(m) });
  assertEqual(1, codeDouble, 'a target that keeps changing is refused rather than guessed at');
  assertEqual(true, logsDouble.length === 1 && /changed twice/.test(logsDouble[0]), 'and the reason names what happened');
  const afterAbort = JSON.parse(fs.readFileSync(personalFile, 'utf8'));
  assertEqual(false, !!afterAbort.projects?.['/repo/three']?.mcpServers?.gamma, "the script's own merge was never written after the second conflict");
}

// ---------------------------------------------------------------- the real CLI entry point

{
  // HOME and USERPROFILE redirected to a fixture directory - the CLI hardcodes os.homedir() for
  // the work file and defaults argv[2] from it too, so this is the only way to exercise that path
  // without ever touching a real profile.
  const home = freshDir('cli-home');
  const target = freshDir('cli-target');
  writeJson(path.join(home, '.claude.json'), { projects: { '/repo/four': { mcpServers: { delta: {} } } } });
  writeJson(path.join(target, '.claude.json'), { projects: {} });

  const result = spawnSync(process.execPath, [mirrorScript, target], {
    env: { ...process.env, HOME: home, USERPROFILE: home, CLAUDE_AUTO_MIRROR_EXCLUDE: '', CLAUDE_AUTO_MIRROR_SECRETS: '' },
    encoding: 'utf8',
  });
  assertEqual(0, result.status, 'the real CLI entry point exits 0 on a clean additive merge');
  assertEqual(true, result.stdout.includes('delta'), 'and reports the addition on stdout');
  const merged = JSON.parse(fs.readFileSync(path.join(target, '.claude.json'), 'utf8'));
  assertEqual(true, !!merged.projects?.['/repo/four']?.mcpServers?.delta, 'the target file on disk carries the merged server');
}

// ---------------------------------------------------------------- credentials travel by default

{
  // A server's env block (stdio) and headers (http/sse) are where its token lives. The accounts
  // belong to one person on one machine, so the default copies a server verbatim, token included;
  // copySecrets=false (CLAUDE_AUTO_MIRROR_SECRETS=0) strips those two fields instead.
  const home = freshDir('secrets-home');
  const target = freshDir('secrets-target');
  const workFile = path.join(home, '.claude.json');
  const personalFile = path.join(target, '.claude.json');
  writeJson(workFile, {
    projects: {
      '/repo/five': {
        mcpServers: {
          stdio: { command: 'stdio-cmd', args: ['--x'], env: { API_TOKEN: 'fixture-token' } },
          web: { type: 'http', url: 'https://mcp.example.invalid', headers: { Authorization: 'Bearer fixture-token' } },
          skipped: { command: 'skipped-cmd' },
        },
      },
    },
  });
  writeJson(personalFile, { projects: {} });

  const logs = [];
  const code = runMirror({ workFile, personalFile, log: (m) => logs.push(m), exclude: ['skipped'] });
  assertEqual(0, code, 'a default merge of servers with credentials exits 0');
  const after = JSON.parse(fs.readFileSync(personalFile, 'utf8'));
  const servers = after.projects?.['/repo/five']?.mcpServers ?? {};
  assertEqual('stdio-cmd --x', `${servers.stdio?.command} ${servers.stdio?.args?.join(' ')}`, 'the server definition itself is copied');
  assertEqual('fixture-token', servers.stdio?.env?.API_TOKEN, 'a stdio server is copied WITH its env block by default');
  assertEqual('Bearer fixture-token', servers.web?.headers?.Authorization, 'an http server is copied WITH its headers by default');
  assertEqual(false, 'skipped' in servers, 'a server named in exclude is never copied');
  assertEqual(true, logs.length === 1 && !/not copied/.test(logs[0]), 'a verbatim copy reports nothing as left out');

  const target2 = freshDir('secrets-optout-target');
  const personalFile2 = path.join(target2, '.claude.json');
  writeJson(personalFile2, { projects: {} });
  const logs2 = [];
  runMirror({ workFile, personalFile: personalFile2, log: (m) => logs2.push(m), copySecrets: false });
  const s2 = JSON.parse(fs.readFileSync(personalFile2, 'utf8')).projects?.['/repo/five']?.mcpServers ?? {};
  assertEqual(false, 'env' in (s2.stdio ?? {}), 'copySecrets=false copies a stdio server WITHOUT its env block');
  assertEqual(false, 'headers' in (s2.web ?? {}), 'copySecrets=false copies an http server WITHOUT its headers');
  assertEqual('https://mcp.example.invalid', s2.web?.url, 'the stripped http server keeps everything else');
  assertEqual(true, logs2.length === 1 && /stdio[^,]*\(env not copied\)/.test(logs2[0]) && /web[^,]*\(headers not copied\)/.test(logs2[0]), 'the log line names what was left out, per server');

  // The CLI reads both switches from the environment: unset SECRETS copies, only "0" strips.
  const cliEnv = (extra) => {
    const env = { ...process.env, HOME: home, USERPROFILE: home, ...extra };
    for (const [k, v] of Object.entries(env)) if (v === undefined) delete env[k];
    return env;
  };
  const target3 = freshDir('secrets-cli-target');
  writeJson(path.join(target3, '.claude.json'), { projects: {} });
  const cli = spawnSync(process.execPath, [mirrorScript, target3], {
    env: cliEnv({ CLAUDE_AUTO_MIRROR_EXCLUDE: 'skipped, web', CLAUDE_AUTO_MIRROR_SECRETS: undefined }),
    encoding: 'utf8',
  });
  const s3 = JSON.parse(fs.readFileSync(path.join(target3, '.claude.json'), 'utf8')).projects?.['/repo/five']?.mcpServers ?? {};
  assertEqual('0|stdio|fixture-token', `${cli.status}|${Object.keys(s3).join(',')}|${s3.stdio?.env?.API_TOKEN}`, 'the CLI takes the exclude list from CLAUDE_AUTO_MIRROR_EXCLUDE and copies secrets with CLAUDE_AUTO_MIRROR_SECRETS unset');

  const target4 = freshDir('secrets-cli-optout-target');
  writeJson(path.join(target4, '.claude.json'), { projects: {} });
  const cli4 = spawnSync(process.execPath, [mirrorScript, target4], {
    env: cliEnv({ CLAUDE_AUTO_MIRROR_EXCLUDE: 'skipped', CLAUDE_AUTO_MIRROR_SECRETS: '0' }),
    encoding: 'utf8',
  });
  const s4 = JSON.parse(fs.readFileSync(path.join(target4, '.claude.json'), 'utf8')).projects?.['/repo/five']?.mcpServers ?? {};
  assertEqual('0|false|false|true', `${cli4.status}|${'env' in (s4.stdio ?? {})}|${'headers' in (s4.web ?? {})}|${/env not copied/.test(cli4.stdout)}`, 'CLAUDE_AUTO_MIRROR_SECRETS=0 makes the CLI strip env/headers and say so on stdout');
}

// ---------------------------------------------------------------- user-scope servers

{
  // The top-level mcpServers map is the user scope. Work is canonical for it: the target ends up
  // with every work server under the work definition, keeps a server only it has, and changes in
  // nothing else - the account record, the counters and the per-project map sit in the same file.
  const home = freshDir('user-home');
  const target = freshDir('user-target');
  const workFile = path.join(home, '.claude.json');
  const personalFile = path.join(target, '.claude.json');
  writeJson(workFile, {
    numStartups: 900,
    oauthAccount: { emailAddress: 'work@example.invalid' },
    mcpServers: {
      graph: { command: 'graph-cmd', args: ['--stdio'], env: { GRAPH_TOKEN: 'fixture-user-token' } },
      search: { type: 'http', url: 'https://search.example.invalid', headers: { Authorization: 'Bearer fixture-user-token' } },
      same: { command: 'same-cmd', args: ['a'], env: { K: 'v' } },
    },
  });
  const targetBefore = {
    numStartups: 7,
    oauthAccount: { emailAddress: 'personal@example.invalid', accountUuid: 'fixture-uuid' },
    mcpServers: {
      mine: { command: 'only-here-cmd' },
      search: { type: 'http', url: 'https://old.example.invalid' },
      // Same definition as work's, keys in another order: not a difference.
      same: { env: { K: 'v' }, args: ['a'], command: 'same-cmd' },
    },
    projects: { '/repo/six': { mcpServers: { local: { command: 'local-cmd' } }, lastCost: 1.5 } },
    userID: 'fixture-user-id',
  };
  writeJson(personalFile, targetBefore);

  const logs = [];
  const code = runMirror({ workFile, personalFile, log: (m) => logs.push(m) });
  const after = JSON.parse(fs.readFileSync(personalFile, 'utf8'));
  const line = logs.join('\n');
  assertEqual(0, code, 'a user-scope mirror exits 0');
  assertEqual(JSON.stringify(JSON.parse(fs.readFileSync(workFile, 'utf8')).mcpServers.graph), JSON.stringify(after.mcpServers?.graph), 'a user-scope server the target lacks is added with the work definition');
  assertEqual('https://search.example.invalid|Bearer fixture-user-token', `${after.mcpServers?.search?.url}|${after.mcpServers?.search?.headers?.Authorization}`, 'a user-scope server defined differently takes the work definition');
  assertEqual('only-here-cmd', after.mcpServers?.mine?.command, 'a user-scope server only the target has is kept');
  assertEqual('env,args,command', Object.keys(after.mcpServers?.same ?? {}).join(','), 'a server that differs only in key order is left as it is');
  const { mcpServers: _b, ...restBefore } = targetBefore;
  const { mcpServers: _a, ...restAfter } = after;
  assertEqual(JSON.stringify(restBefore), JSON.stringify(restAfter), 'nothing outside mcpServers changes: account record, counters, per-project map');
  assertEqual(Object.keys(targetBefore).join(','), Object.keys(after).join(','), 'the top-level key order is kept');
  assertEqual('mine,search,same,graph', Object.keys(after.mcpServers ?? {}).join(','), 'existing servers keep their place, a new one goes last');
  assertEqual(true, /graph @ user/.test(line) && /search @ user \(updated\)/.test(line) && /mine @ user \(only in the target, kept\)/.test(line), 'the line names the added, the updated and the target-only server');
  assertEqual(false, /fixture-user-token|example\.invalid|-cmd/.test(line), 'and carries names only, never a value from a definition');
  assertEqual(false, /same @ user/.test(line), 'a server that already agrees is not reported');

  const statBefore = fs.statSync(personalFile);
  const logs2 = [];
  const code2 = runMirror({ workFile, personalFile, log: (m) => logs2.push(m) });
  assertEqual(`0|${statBefore.mtimeMs}`, `${code2}|${fs.statSync(personalFile).mtimeMs}`, 'a second user-scope run writes nothing');
  assertEqual(true, logs2.length === 1 && /mine @ user \(only in the target, kept\)/.test(logs2[0]) && !/graph|search/.test(logs2[0]), 'and still reports the target-only server, alone');

  // A target with no user-scope map at all (a fresh account) gets one; a work file with none
  // leaves the target alone.
  const target2 = freshDir('user-empty-target');
  const personalFile2 = path.join(target2, '.claude.json');
  writeJson(personalFile2, { userID: 'fixture-user-id-2' });
  const code3 = runMirror({ workFile, personalFile: personalFile2, log: () => {}, exclude: ['search'] });
  const after2 = JSON.parse(fs.readFileSync(personalFile2, 'utf8'));
  assertEqual('0|userID,mcpServers|graph,same', `${code3}|${Object.keys(after2).join(',')}|${Object.keys(after2.mcpServers ?? {}).join(',')}`, 'a target with no user-scope map gets one, minus the excluded names');

  const workFile3 = path.join(home, 'work3.claude.json');
  writeJson(workFile3, { userID: 'w' });
  const stat3 = fs.statSync(personalFile);
  const logs3 = [];
  const code4 = runMirror({ workFile: workFile3, personalFile, log: (m) => logs3.push(m) });
  assertEqual(`0|${stat3.mtimeMs}|4`, `${code4}|${fs.statSync(personalFile).mtimeMs}|${Object.keys(JSON.parse(fs.readFileSync(personalFile, 'utf8')).mcpServers).length}`, 'a work file with no user-scope servers leaves the target untouched');

  // copySecrets=false: the work definition without env/headers; credentials the target already
  // holds for that server stay where they are.
  const target4 = freshDir('user-optout-target');
  const personalFile4 = path.join(target4, '.claude.json');
  writeJson(personalFile4, { mcpServers: { graph: { command: 'stale-cmd', env: { GRAPH_TOKEN: 'target-own-token' } } } });
  const logs4 = [];
  runMirror({ workFile, personalFile: personalFile4, log: (m) => logs4.push(m), copySecrets: false });
  const s4 = JSON.parse(fs.readFileSync(personalFile4, 'utf8')).mcpServers ?? {};
  assertEqual('graph-cmd|target-own-token|false', `${s4.graph?.command}|${s4.graph?.env?.GRAPH_TOKEN}|${'headers' in (s4.search ?? {})}`, 'copySecrets=false updates the definition, keeps the target credentials, adds a server without its headers');
  const stat4 = fs.statSync(personalFile4);
  runMirror({ workFile, personalFile: personalFile4, log: () => {}, copySecrets: false });
  assertEqual(String(stat4.mtimeMs), String(fs.statSync(personalFile4).mtimeMs), 'and a second copySecrets=false run writes nothing');
}

if (Ran !== 49) {
  console.log(`COULD NOT RUN: expected 49 assertions, ran ${Ran} - an assertion was skipped`);
  process.exit(2);
}
if (Failed) {
  console.log('');
  console.log(`${Failed} failed`);
  process.exit(1);
}
console.log('');
console.log('all passed');
process.exit(0);
