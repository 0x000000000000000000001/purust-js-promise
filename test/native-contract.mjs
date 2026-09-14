import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { codegenPrelude } from '../../purust/output/Purust.CodeGen/index.js';
import { threadedRust, threadedPrelude } from '../../purust/src/Purust/Threading.js';
import { empty } from '../../purust/output/Data.Set/index.js';
import * as JS from '../src/Promise/Internal.js';

// The original JavaScript FFI is the ordering oracle, including extra jobs
// introduced by adoption and finally. No fixed sleeps are needed.
const trace = [];
const record = label => () => { trace.push(label); return JS.resolve(undefined); };
const p = JS.resolve(1);
JS.then_(record('chain'), JS.then_(record('then'), p));
JS.then_(record('after-finally'), JS.finally(record('finally'), p));
JS.then_(record('marker'), JS.resolve(9));
JS.then_(record('all'), JS.all([p, JS.resolve(2)]));
JS.then_(record('race'), JS.race([p, JS.resolve(2)]));
JS.then_(record('adopted'), JS.new(resolve => resolve(p)));
trace.push('sync');
await new Promise(resolve => setImmediate(resolve));

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const body = `pub mod microtasks { ${read('../../purust/src/Purust/Microtasks.rs')} }
mod Purs_Effect_Exception { ${read('../../purust-exceptions/src/Effect/Exception.rs')} }
${read('../src/Promise/Internal.rs')}
#[cfg(test)] mod checks { const EXPECTED_TRACE: &[&str] = &${JSON.stringify(trace)}; ${read('./native-contract.rs')} }`;
const runtime = fileURLToPath(new URL('../../purust/tests/runtime/perceus_ptr/src/lib.rs', import.meta.url));
const directory = mkdtempSync(join(tmpdir(), 'purust-promise-contract-'));
try {
  for (const threaded of [false, true]) {
    const prelude = codegenPrelude(empty);
    const file = join(directory, 'checks.rs');
    const binary = join(directory, 'checks');
    writeFileSync(file, `${threaded ? threadedPrelude(prelude) : prelude}\nextern crate self as purust_core;\n#[path = ${JSON.stringify(runtime)}] mod perceus_ptr;\n${threaded ? threadedRust(body) : body}`);
    for (const [command, args] of [
      ['rustc', ['--test', '--edition=2021', '-Awarnings', file, '-o', binary, ...(threaded ? ['--cfg', 'feature="threaded"'] : [])]],
      [binary, ['--test-threads=1']],
    ]) {
      const result = spawnSync(command, args, { encoding: 'utf8', timeout: 30_000 });
      assert.equal(result.status, 0, `${result.error ?? ''}\n${result.stdout}\n${result.stderr}`);
      if (command === binary) console.log(`${threaded ? 'Arc' : 'Rc'}: ${result.stdout}`);
    }
  }
} finally { rmSync(directory, { recursive: true, force: true }); }
