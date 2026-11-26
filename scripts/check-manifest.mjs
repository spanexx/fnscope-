#!/usr/bin/env node
// Validate a fnscope manifest against the invariants the harness depends on.
import { readFileSync } from 'node:fs';

const manifestPath = process.argv[2];

if (!manifestPath) {
  console.error('MANIFEST_FAIL: usage: node check-manifest.mjs <manifest.json>');
  process.exit(1);
}

let graph;
try {
  graph = JSON.parse(readFileSync(manifestPath, 'utf8'));
} catch (err) {
  console.error(`MANIFEST_FAIL: cannot read or parse ${manifestPath}: ${err.message}`);
  process.exit(1);
}

const problems = [];
const fail = (message) => problems.push(message);

if (graph.tool !== 'fnscope') {
  fail(`tool must be "fnscope", got ${JSON.stringify(graph.tool)}`);
}
if (graph.schemaVersion !== '1.0.0') {
  fail(`schemaVersion must be "1.0.0", got ${JSON.stringify(graph.schemaVersion)}`);
}

const functions = Array.isArray(graph.functions) ? graph.functions : null;
if (functions === null) {
  fail('functions must be an array');
} else if (functions.length === 0) {
  fail('functions must be non-empty');
}

const ids = new Set();

// First pass: identity, required fields and step-id uniqueness (no cross-references yet).
if (functions) {
  functions.forEach((fn, index) => {
    const where = `functions[${index}]`;
    if (!fn || typeof fn !== 'object') {
      fail(`${where} must be an object`);
      return;
    }

    if (typeof fn.id !== 'string' || fn.id.trim() === '') {
      fail(`${where}.id must be a non-empty string`);
    } else if (ids.has(fn.id)) {
      fail(`${where}.id is duplicated: ${fn.id}`);
    } else {
      ids.add(fn.id);
    }

    if (typeof fn.astHash !== 'string' || !/^sha256:[0-9a-fA-F]{64}$/.test(fn.astHash)) {
      fail(`${where}.astHash must match sha256:<64 hex>, got ${JSON.stringify(fn.astHash)}`);
    }

    for (const field of ['filePath', 'module', 'sourceCode']) {
      if (typeof fn[field] !== 'string' || fn[field].trim() === '') {
        fail(`${where}.${field} must be a non-empty string`);
      }
    }
    if (typeof fn.line !== 'number' || !Number.isFinite(fn.line) || fn.line < 1) {
      fail(`${where}.line must be a number >= 1, got ${JSON.stringify(fn.line)}`);
    }

    const steps = Array.isArray(fn.steps) ? fn.steps : null;
    if (steps === null) {
      fail(`${where}.steps must be an array`);
    } else {
      const stepIds = new Set();
      steps.forEach((step, stepIndex) => {
        const stepWhere = `${where}.steps[${stepIndex}]`;
        if (!step || typeof step !== 'object') {
          fail(`${stepWhere} must be an object`);
          return;
        }
        if (typeof step.id !== 'string' || !/^step-[0-9]+$/.test(step.id)) {
          fail(`${stepWhere}.id must be step-N, got ${JSON.stringify(step.id)}`);
        } else if (stepIds.has(step.id)) {
          fail(`${stepWhere}.id is duplicated within ${where}: ${step.id}`);
        } else {
          stepIds.add(step.id);
        }
      });
    }
  });
}

// Second pass: every non-null call reference resolves to a known function id.
let edges = 0;
if (functions) {
  functions.forEach((fn, index) => {
    if (!fn || typeof fn !== 'object') return;
    const where = `functions[${index}]`;

    const callers = Array.isArray(fn.calledBy) ? fn.calledBy : null;
    if (callers === null) {
      fail(`${where}.calledBy must be an array`);
    } else {
      callers.forEach((caller, callerIndex) => {
        const ref = caller && caller.fnId;
        if (ref === null || ref === undefined) return;
        if (typeof ref !== 'string' || !ids.has(ref)) {
          fail(`${where}.calledBy[${callerIndex}].fnId does not resolve: ${JSON.stringify(ref)}`);
        } else {
          edges += 1;
        }
      });
    }

    const steps = Array.isArray(fn.steps) ? fn.steps : [];
    steps.forEach((step, stepIndex) => {
      if (!step || typeof step !== 'object') return;
      const ref = step.callsFnId;
      if (ref === null || ref === undefined) return;
      if (typeof ref !== 'string' || !ids.has(ref)) {
        fail(`${where}.steps[${stepIndex}].callsFnId does not resolve: ${JSON.stringify(ref)}`);
      } else {
        edges += 1;
      }
    });
  });
}

if (problems.length > 0) {
  for (const problem of problems) {
    console.error(`MANIFEST_FAIL: ${problem}`);
  }
  process.exit(1);
}

console.log(`MANIFEST_OK: ${functions.length} functions, ${edges} call edges`);
