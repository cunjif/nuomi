// fake_cli.js — deterministic CLI agent fixture for adapters::cli tests
// (SPEC docs/specs/cli-agents-m1.md, D8). argv[2] selects the flavor.
//
// This file lives under a repo whose package.json sets "type": "module",
// so it is written as ESM. Top-level `return` is illegal in ESM; control
// flow below avoids it instead.
import fs from 'node:fs';

function writeLine(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function readStdinSync() {
  try {
    return fs.readFileSync(0, 'utf8');
  } catch {
    return '';
  }
}

function runOnce(flavor) {
  if (flavor === 'claude_code') {
    writeLine({ type: 'system', subtype: 'init', session_id: 's-1' });
    writeLine({ type: 'assistant', message: { content: [{ type: 'text', text: 'hello ' }] } });
    writeLine({ type: 'assistant', message: { content: [{ type: 'text', text: 'world' }] } });
    writeLine({
      type: 'result',
      subtype: 'success',
      result: 'hello world',
      usage: { input_tokens: 3, output_tokens: 2 },
    });
  } else if (flavor === 'codex') {
    writeLine({ type: 'thread.started', thread_id: 't-1' });
    writeLine({ type: 'item.completed', item: { type: 'agent_message', text: 'hi from codex' } });
    writeLine({ type: 'turn.completed', usage: { input_tokens: 5, output_tokens: 4 } });
  } else {
    const prompt = readStdinSync();
    const firstLine = prompt.split(/\r?\n/)[0] || '';
    process.stdout.write('plain says: ' + firstLine + '\n');
  }
}

// Kill-on-drop probe: mark startup, then append a line every 100ms and
// mirror it to stdout, never exiting. The harness must reap this process.
function runAliveForever() {
  const markerPath = process.env.FAKE_CLI_ALIVE_FILE;
  fs.writeFileSync(markerPath, 'started\n');
  setInterval(() => {
    fs.appendFileSync(markerPath, 'alive\n');
    process.stdout.write('alive\n');
  }, 100);
  setInterval(() => {}, 1000);
}

const flavor = process.argv[2] || 'plain';

if (flavor === 'alive') {
  runAliveForever();
} else {
  runOnce(flavor);
  process.exit(0);
}
