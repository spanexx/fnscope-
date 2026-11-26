#!/usr/bin/env node
// Local OpenAI-compatible LLM stand-in for scripts/enrich-e2e.sh.
// Uses only Node core modules so the end-to-end proof needs no npm install.
import { createServer } from 'node:http';
import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';

const port = Number.parseInt(process.argv[2] ?? '0', 10);
const countPath = process.argv[3];
const logPath = process.argv[4];
const portPath = process.argv[5];

const usage = 'usage: node mock-llm.mjs <port|0> <count-file> <log-file> [port-file]';
if (!Number.isInteger(port) || port < 0 || port > 65535 || !countPath || !logPath) {
  console.error(`MOCK_LLM_FAIL: ${usage}`);
  process.exit(2);
}

// The counter lives in a file so the shell can observe progress between scans.
function readCount() {
  try {
    const value = Number.parseInt(readFileSync(countPath, 'utf8').trim(), 10);
    return Number.isInteger(value) && value >= 0 ? value : 0;
  } catch {
    return 0;
  }
}

// Node handles each request callback to completion before the next one, so a
// synchronous read/modify/write is race-free for this single-process mock.
function bumpCount() {
  const next = readCount() + 1;
  writeFileSync(countPath, String(next), 'utf8');
  return next;
}

function log(line) {
  try {
    appendFileSync(logPath, `[${new Date().toISOString()}] ${line}\n`, 'utf8');
  } catch {
    // Logging must never turn a mock failure into a silent hang.
  }
}

// Collects the indented body lines that follow one of the prompt's section headers.
function sectionLines(prompt, header) {
  const lines = prompt.split(/\r?\n/);
  const start = lines.findIndex((line) => line.startsWith(header));
  if (start < 0) return [];
  const collected = [];
  for (let index = start + 1; index < lines.length; index += 1) {
    const line = lines[index];
    if (!/^\s{2}\S/.test(line)) break;
    collected.push(line.trim());
  }
  return collected;
}

// Derives a deterministic, readable answer from the skeleton the CLI sends.
function buildEnvelope(prompt) {
  const match = /^name:\s*(.+)$/m.exec(prompt);
  const name = match ? match[1].trim() : 'function';
  // Count step lines directly: multi-line step code can wrap a prompt line.
  const stepCount = prompt
    .split(/\r?\n/)
    .filter((line) => /^\s*step-\d+\s*\|/.test(line)).length;
  const defaults = {};
  for (const line of sectionLines(prompt, 'parameters (name: type):')) {
    const separator = line.indexOf(':');
    if (separator > 0) defaults[line.slice(0, separator).trim()] = 'sample';
  }

  const payload = {
    description: `Mock-enriched description for ${name}.`,
    stepTitles: Array.from(
      { length: stepCount },
      (_, index) => `Mock title for step ${index + 1} of ${name}`,
    ),
    stepDetails: Array.from(
      { length: stepCount },
      (_, index) => `Mock detail for step ${index + 1} of ${name}`,
    ),
    defaults,
    tags: ['mock', name],
  };

  // The Rust side strips ```json / ``` fences before parsing the inner object.
  const content = `\`\`\`json\n${JSON.stringify(payload)}\n\`\`\``;
  return JSON.stringify({ choices: [{ message: { content } }] });
}

const server = createServer((request, response) => {
  const chunks = [];
  request.on('data', (chunk) => chunks.push(chunk));
  request.on('end', () => {
    const body = Buffer.concat(chunks).toString('utf8');

    if (request.method === 'GET' && request.url === '/count') {
      const current = readCount();
      log(`GET /count -> ${current}`);
      response.writeHead(200, { 'Content-Type': 'text/plain; charset=utf-8' });
      response.end(String(current));
      return;
    }

    if (request.method === 'POST') {
      const current = bumpCount();
      let prompt = '';
      try {
        prompt = JSON.parse(body)?.messages?.[1]?.content ?? '';
      } catch {
        // A malformed body still counts as a request; answer with blank text.
      }
      log(`POST ${request.url} -> count=${current} bodyBytes=${body.length}`);
      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(buildEnvelope(prompt));
      return;
    }

    log(`${request.method} ${request.url} -> 404`);
    response.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    response.end('not found');
  });
});

server.listen(port, '127.0.0.1', () => {
  const address = server.address();
  const actual = address && typeof address === 'object' ? address.port : port;
  if (portPath) writeFileSync(portPath, String(actual), 'utf8');
  log(`listening on 127.0.0.1:${actual}`);
});

for (const signal of ['SIGTERM', 'SIGINT']) {
  process.on(signal, () => {
    log(`received ${signal}; exiting`);
    server.close();
    process.exit(0);
  });
}
