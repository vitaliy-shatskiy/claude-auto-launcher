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
    env: { ...process.env, HOME: home, USERPROFILE: home },
    encoding: 'utf8',
  });
  assertEqual(0, result.status, 'the real CLI entry point exits 0 on a clean additive merge');
  assertEqual(true, result.stdout.includes('delta'), 'and reports the addition on stdout');
  const merged = JSON.parse(fs.readFileSync(path.join(target, '.claude.json'), 'utf8'));
  assertEqual(true, !!merged.projects?.['/repo/four']?.mcpServers?.delta, 'the target file on disk carries the merged server');
}

if (Ran !== 20) {
  console.log(`COULD NOT RUN: expected 20 assertions, ran ${Ran} - an assertion was skipped`);
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
