import React, { useState, useMemo, useEffect } from 'react';
import {
  Play,
  Copy,
  Check,
  ChevronRight,
  ChevronDown,
  Shield,
  Zap,
  AlertTriangle,
  Terminal,
  Code2,
  Eye,
  Columns,
  RotateCcw,
  GitBranch,
  ArrowUpRight,
  ArrowDownRight,
  Network,
  CornerUpLeft
} from 'lucide-react';

// Built-in sample graph used when no external `graph` prop (from .fnscope/fn-graph.json) is passed
const SAMPLE_FUNCTIONS = [
  {
    id: 'grade-sentence-exercise',
    name: 'gradeSentenceExercise',
    module: 'study/grading',
    version: '1.1.0',
    runtime: 'node20 / edge',
    purity: 'side-effect',
    timeoutMs: 3000,
    permissions: ['net:outbound'],
    description:
      'Entry-point handler that grades a learner answer using evaluateGrammarRole() and dispatches a telemetry webhook on completion.',
    tags: ['entrypoint', 'grading'],
    calledBy: [
      {
        fnId: null,
        name: 'POST /api/study/grade',
        module: 'routes/study.ts:42',
        callSite: 'const result = await gradeSentenceExercise(req.body)'
      }
    ],
    parameters: [
      {
        name: 'sentence',
        type: 'string',
        required: true,
        defaultValue: 'The quick fox jumped over the lazy dog.',
        description: 'Prompt sentence shown to the learner.'
      },
      {
        name: 'targetWord',
        type: 'string',
        required: true,
        defaultValue: 'fox',
        description: 'Highlighted word the learner was asked to classify.'
      },
      {
        name: 'selectedRoleId',
        type: 'string',
        required: true,
        defaultValue: 'doer',
        description: 'Role ID submitted by the learner.'
      }
    ],
    returns: {
      type: 'Promise<GradeOutcome>',
      fields: [
        { name: 'correct', type: 'boolean' },
        { name: 'expectedRole', type: 'RoleEvaluationResult' },
        { name: 'receipt', type: 'DeliveryReceipt' }
      ]
    },
    errors: [
      { code: 'TOKEN_NOT_FOUND', status: 422, when: 'Propagated from evaluateGrammarRole()' }
    ],
    steps: [
      {
        id: 'step-eval',
        kind: 'call',
        callsFnId: 'evaluate-grammar-role',
        title: 'Evaluate expected grammatical role',
        code: 'const expected = await evaluateGrammarRole({ sentence, targetWord, includeTeachingLayer: true })',
        inputs: ['input.sentence', 'input.targetWord'],
        output: 'expected: RoleEvaluationResult',
        detail: 'Delegates syntactic classification to evaluateGrammarRole().'
      },
      {
        id: 'step-compare',
        kind: 'transform',
        title: 'Compare learner selection against expected role',
        code: 'const correct = expected.roleId === input.selectedRoleId',
        inputs: ['expected.roleId', 'input.selectedRoleId'],
        output: 'correct: boolean',
        detail: 'Checks whether the user selected the matching grammatical job.'
      },
      {
        id: 'step-webhook',
        kind: 'call',
        callsFnId: 'dispatch-webhook-event',
        title: 'Emit exercise.graded webhook event',
        code: 'const receipt = await dispatchWebhookEvent({ endpointUrl, eventType: "exercise.graded", dryRun: true })',
        inputs: ['correct', 'expected'],
        output: 'receipt: DeliveryReceipt',
        detail: 'Sends signed completion telemetry via dispatchWebhookEvent().'
      },
      {
        id: 'step-grade-return',
        kind: 'return',
        title: 'Return grading verdict and receipt',
        code: 'return { correct, expectedRole: expected, receipt }',
        inputs: ['correct', 'expected', 'receipt'],
        output: 'GradeOutcome',
        detail: 'Returns immediate feedback plus delivery metadata to the API route.'
      }
    ],
    sourceCode: `export async function gradeSentenceExercise(input: {
  sentence: string;
  targetWord: string;
  selectedRoleId: string;
}): Promise<GradeOutcome> {
  // 1. Evaluate ground-truth role via nlp/roles
  const expected = await evaluateGrammarRole({
    sentence: input.sentence,
    targetWord: input.targetWord,
    includeTeachingLayer: true
  });

  // 2. Check learner answer
  const correct = expected.roleId === input.selectedRoleId;

  // 3. Dispatch signed telemetry webhook
  const receipt = await dispatchWebhookEvent({
    endpointUrl: 'https://hooks.example.dev/v1/ingest',
    eventType: 'exercise.graded',
    dryRun: true
  });

  return { correct, expectedRole: expected, receipt };
}`,
    execute: (args, allFns) => {
      const evalFn = allFns.find((f) => f.id === 'evaluate-grammar-role');
      const webhookFn = allFns.find((f) => f.id === 'dispatch-webhook-event');
      const evalRes = evalFn.execute(
        {
          sentence: args.sentence,
          targetWord: args.targetWord,
          includeTeachingLayer: true
        },
        allFns
      );
      if (!evalRes.ok) {
        return {
          ok: false,
          error: evalRes.error,
          trace: {
            'step-eval': { status: 'thrown', output: evalRes.error }
          }
        };
      }
      const correct = evalRes.data.roleId === String(args.selectedRoleId ?? '').trim();
      const hookRes = webhookFn.execute(
        {
          endpointUrl: 'https://hooks.example.dev/v1/ingest',
          eventType: 'exercise.graded',
          dryRun: true
        },
        allFns
      );
      return {
        ok: true,
        data: {
          correct,
          expectedRole: evalRes.data,
          receipt: hookRes.data
        },
        trace: {
          'step-eval': { status: 'ok', output: `${evalRes.data.roleId} (${evalRes.data.label})` },
          'step-compare': { status: 'ok', output: `correct = ${correct}` },
          'step-webhook': { status: 'ok', output: hookRes.data.deliveryId },
          'step-grade-return': { status: 'ok', output: 'GradeOutcome' }
        }
      };
    }
  },
  {
    id: 'evaluate-grammar-role',
    name: 'evaluateGrammarRole',
    module: 'nlp/roles',
    version: '1.2.0',
    runtime: 'node20 / edge',
    purity: 'pure',
    timeoutMs: 150,
    description:
      'Tokenizes a sentence, finds the target word token (throwing if absent), infers its syntactic job, and returns the role payload.',
    tags: ['nlp', 'parser', 'pure'],
    calledBy: [
      {
        fnId: 'grade-sentence-exercise',
        name: 'gradeSentenceExercise',
        module: 'study/grading.ts:8',
        callSite: 'const expected = await evaluateGrammarRole({ sentence, targetWord })'
      }
    ],
    parameters: [
      {
        name: 'sentence',
        type: 'string',
        required: true,
        defaultValue: 'The quick fox jumped over the lazy dog.',
        description: 'Full English sentence providing syntactic context.'
      },
      {
        name: 'targetWord',
        type: 'string',
        required: true,
        defaultValue: 'fox',
        description: 'Word inside the sentence whose grammatical job should be identified.'
      },
      {
        name: 'includeTeachingLayer',
        type: 'boolean',
        required: false,
        defaultValue: true,
        description: 'Attach plain-English intuition explaining why the word holds that job.'
      }
    ],
    returns: {
      type: 'RoleEvaluationResult',
      fields: [
        { name: 'roleId', type: "'doer' | 'engine' | 'receiver' | 'bridge' | 'painter'" },
        { name: 'label', type: 'string' },
        { name: 'confidence', type: 'number (0..1)' },
        { name: 'intuition', type: 'string | undefined' }
      ]
    },
    errors: [
      { code: 'TOKEN_NOT_FOUND', status: 422, when: 'targetWord does not appear in sentence' },
      { code: 'EMPTY_SENTENCE', status: 400, when: 'sentence contains zero word tokens' }
    ],
    steps: [
      {
        id: 'step-tokenize',
        kind: 'call',
        callsFnId: 'tokenize-sentence',
        title: 'Tokenize input sentence',
        code: 'const tokens = tokenize(input.sentence)',
        inputs: ['input.sentence'],
        output: 'tokens: Token[]',
        detail: 'Splits sentence into normalized word tokens with positional indices.'
      },
      {
        id: 'step-match',
        kind: 'transform',
        title: 'Find matching target token',
        code: 'const match = tokens.find(t => t.normalized === input.targetWord.trim().toLowerCase())',
        inputs: ['tokens', 'input.targetWord'],
        output: 'match: Token | undefined',
        detail: 'Normalizes targetWord and searches the token stream for an exact match.'
      },
      {
        id: 'step-guard',
        kind: 'branch',
        title: 'Guard: throw if token missing',
        code: 'if (!match) throw new FunctionError("TOKEN_NOT_FOUND", { target: input.targetWord })',
        inputs: ['match'],
        output: 'throws TOKEN_NOT_FOUND | continues',
        detail: 'Aborts execution immediately with 422 if targetWord is not in the sentence.'
      },
      {
        id: 'step-infer',
        kind: 'call',
        callsFnId: 'infer-syntactic-job',
        title: 'Infer syntactic role from context',
        code: 'const role = inferSyntacticJob(match, tokens)',
        inputs: ['match', 'tokens'],
        output: 'role: SyntacticJob',
        detail: 'Classifies the matched token into Doer, Engine, Receiver, Bridge, or Painter.'
      },
      {
        id: 'step-return',
        kind: 'return',
        title: 'Assemble & return evaluation result',
        code: 'return { roleId, label, confidence, ...(includeTeachingLayer && { intuition }) }',
        inputs: ['role', 'input.includeTeachingLayer'],
        output: 'RoleEvaluationResult',
        detail: 'Attaches plain-English intuition only when includeTeachingLayer is true.'
      }
    ],
    sourceCode: `export async function evaluateGrammarRole(input: {
  sentence: string;
  targetWord: string;
  includeTeachingLayer?: boolean;
}): Promise<RoleEvaluationResult> {
  // 1. Tokenize sentence into normalized word tokens
  const tokens = tokenize(input.sentence);

  // 2. Locate target word token in stream
  const normalizedTarget = input.targetWord.trim().toLowerCase();
  const match = tokens.find((t) => t.normalized === normalizedTarget);

  // 3. Guard: fail fast when target word is not present
  if (!match) {
    throw new FunctionError('TOKEN_NOT_FOUND', {
      target: input.targetWord
    });
  }

  // 4. Detect syntactic job from surrounding tokens
  const role = inferSyntacticJob(match, tokens);

  // 5. Return structured result
  return {
    roleId: role.id,
    label: role.label,
    confidence: role.confidence,
    ...(input.includeTeachingLayer ? { intuition: role.intuition } : {})
  };
}`,
    execute: (args) => {
      const sentence = String(args.sentence ?? '');
      const target = String(args.targetWord ?? '').trim();
      const rawTokens = sentence
        .replace(/[.,!?;:"()]/g, ' ')
        .trim()
        .split(/\s+/)
        .filter(Boolean);

      if (rawTokens.length === 0) {
        return {
          ok: false,
          error: 'EMPTY_SENTENCE: sentence must contain at least one word token.',
          trace: {
            'step-tokenize': { status: 'ok', output: '0 tokens' }
          }
        };
      }

      const matchIndex = rawTokens.findIndex(
        (w) => w.toLowerCase() === target.toLowerCase()
      );

      if (matchIndex === -1) {
        return {
          ok: false,
          error: `TOKEN_NOT_FOUND (422): "${target}" was not found in [${rawTokens.join(', ')}]`,
          trace: {
            'step-tokenize': { status: 'ok', output: `${rawTokens.length} tokens` },
            'step-match': { status: 'miss', output: 'undefined' },
            'step-guard': { status: 'thrown', output: 'FunctionError("TOKEN_NOT_FOUND")' }
          }
        };
      }

      const lower = target.toLowerCase();
      let roleId = 'doer';
      let label = 'Doer (subject)';
      let intuition = `"${target}" performs or anchors the main action in this clause.`;

      if (/(ed|ing|s)$/.test(lower) && matchIndex > 0) {
        roleId = 'engine';
        label = 'Engine (verb)';
        intuition = `"${target}" drives the action or state change in the sentence.`;
      } else if (['over', 'under', 'in', 'on', 'with', 'to', 'from', 'by'].includes(lower)) {
        roleId = 'bridge';
        label = 'Bridge (preposition)';
        intuition = `"${target}" links a noun phrase to the rest of the clause.`;
      } else if (['quick', 'lazy', 'red', 'small', 'bright', 'loud'].includes(lower)) {
        roleId = 'painter';
        label = 'Painter (adjective)';
        intuition = `"${target}" colors or modifies the noun right after it.`;
      } else if (matchIndex > 2) {
        roleId = 'receiver';
        label = 'Receiver (object)';
        intuition = `"${target}" receives or completes the action of the verb.`;
      }

      const data = {
        roleId,
        label,
        confidence: 0.94
      };
      if (args.includeTeachingLayer) {
        data.intuition = intuition;
      }

      return {
        ok: true,
        data,
        trace: {
          'step-tokenize': { status: 'ok', output: `${rawTokens.length} tokens` },
          'step-match': { status: 'ok', output: `Token[${matchIndex}] ("${rawTokens[matchIndex]}")` },
          'step-guard': { status: 'passed', output: 'match !== undefined' },
          'step-infer': { status: 'ok', output: `${roleId} → ${label}` },
          'step-return': { status: 'ok', output: 'RoleEvaluationResult' }
        }
      };
    }
  },
  {
    id: 'tokenize-sentence',
    name: 'tokenize',
    module: 'nlp/tokenizer',
    version: '1.0.3',
    runtime: 'universal',
    purity: 'pure',
    timeoutMs: 25,
    description:
      'Strips punctuation, splits raw text on whitespace boundaries, and attaches zero-based token indices.',
    tags: ['nlp', 'leaf', 'pure'],
    calledBy: [
      {
        fnId: 'evaluate-grammar-role',
        name: 'evaluateGrammarRole',
        module: 'nlp/roles.ts:7',
        callSite: 'const tokens = tokenize(input.sentence)'
      }
    ],
    parameters: [
      {
        name: 'sentence',
        type: 'string',
        required: true,
        defaultValue: 'The quick fox jumped over the lazy dog.',
        description: 'Raw input string to segment into word tokens.'
      }
    ],
    returns: {
      type: 'Token[]',
      fields: [
        { name: 'index', type: 'number' },
        { name: 'raw', type: 'string' },
        { name: 'normalized', type: 'string' }
      ]
    },
    errors: [],
    steps: [
      {
        id: 'step-clean',
        kind: 'transform',
        title: 'Strip punctuation delimiters',
        code: 'const cleaned = sentence.replace(/[.,!?;:"()]/g, " ").trim()',
        inputs: ['sentence'],
        output: 'cleaned: string',
        detail: 'Replaces sentence punctuation with spaces so words split cleanly.'
      },
      {
        id: 'step-split-tokens',
        kind: 'transform',
        title: 'Split on whitespace and map token objects',
        code: 'return cleaned.split(/\\s+/).filter(Boolean).map((raw, index) => ({ index, raw, normalized: raw.toLowerCase() }))',
        inputs: ['cleaned'],
        output: 'Token[]',
        detail: 'Produces indexed tokens with both original casing and lowercase normalized forms.'
      }
    ],
    sourceCode: `export function tokenize(sentence: string): Token[] {
  const cleaned = sentence.replace(/[.,!?;:"()]/g, ' ').trim();
  if (!cleaned) return [];

  return cleaned
    .split(/\\s+/)
    .filter(Boolean)
    .map((raw, index) => ({
      index,
      raw,
      normalized: raw.toLowerCase()
    }));
}`,
    execute: (args) => {
      const cleaned = String(args.sentence ?? '')
        .replace(/[.,!?;:"()]/g, ' ')
        .trim();
      const tokens = cleaned
        ? cleaned.split(/\s+/).filter(Boolean).map((raw, index) => ({
            index,
            raw,
            normalized: raw.toLowerCase()
          }))
        : [];
      return {
        ok: true,
        data: tokens,
        trace: {
          'step-clean': { status: 'ok', output: `"${cleaned.slice(0, 36)}..."` },
          'step-split-tokens': { status: 'ok', output: `${tokens.length} tokens` }
        }
      };
    }
  },
  {
    id: 'infer-syntactic-job',
    name: 'inferSyntacticJob',
    module: 'nlp/classifier',
    version: '1.1.0',
    runtime: 'universal',
    purity: 'pure',
    timeoutMs: 50,
    description:
      'Inspects morphology and token position within the clause to assign a plain-English grammatical role.',
    tags: ['nlp', 'leaf', 'pure'],
    calledBy: [
      {
        fnId: 'evaluate-grammar-role',
        name: 'evaluateGrammarRole',
        module: 'nlp/roles.ts:21',
        callSite: 'const role = inferSyntacticJob(match, tokens)'
      }
    ],
    parameters: [
      {
        name: 'word',
        type: 'string',
        required: true,
        defaultValue: 'jumped',
        description: 'Matched word token text.'
      },
      {
        name: 'tokenIndex',
        type: 'number',
        required: true,
        defaultValue: 3,
        description: 'Zero-based position of the word in the sentence.'
      }
    ],
    returns: {
      type: 'SyntacticJob',
      fields: [
        { name: 'id', type: 'string' },
        { name: 'label', type: 'string' },
        { name: 'confidence', type: 'number' },
        { name: 'intuition', type: 'string' }
      ]
    },
    errors: [],
    steps: [
      {
        id: 'step-morph',
        kind: 'branch',
        title: 'Check verb & preposition cues',
        code: 'if (/(ed|ing|s)$/.test(lower) && tokenIndex > 0) return { id: "engine", label: "Engine (verb)" }',
        inputs: ['word', 'tokenIndex'],
        output: 'SyntacticJob | continues',
        detail: 'Detects inflected action words and common relational prepositions.'
      },
      {
        id: 'step-pos',
        kind: 'return',
        title: 'Classify subject vs. object by clause position',
        code: 'return tokenIndex > 2 ? { id: "receiver", label: "Receiver (object)" } : { id: "doer", label: "Doer (subject)" }',
        inputs: ['word', 'tokenIndex'],
        output: 'SyntacticJob',
        detail: 'Assigns pre-verb nouns as Doer and post-verb nouns as Receiver.'
      }
    ],
    sourceCode: `export function inferSyntacticJob(
  match: Token,
  tokens: Token[]
): SyntacticJob {
  const lower = match.normalized;

  if (/(ed|ing|s)$/.test(lower) && match.index > 0) {
    return {
      id: 'engine',
      label: 'Engine (verb)',
      confidence: 0.94,
      intuition: \`"\${match.raw}" drives the action in this clause.\`
    };
  }

  return match.index > 2
    ? { id: 'receiver', label: 'Receiver (object)', confidence: 0.91, intuition: \`"\${match.raw}" receives the action.\` }
    : { id: 'doer', label: 'Doer (subject)', confidence: 0.94, intuition: \`"\${match.raw}" anchors the action.\` };
}`,
    execute: (args) => {
      const word = String(args.word ?? 'fox');
      const idx = Number(args.tokenIndex ?? 0);
      const lower = word.toLowerCase();
      const isVerb = /(ed|ing|s)$/.test(lower) && idx > 0;
      const result = isVerb
        ? {
            id: 'engine',
            label: 'Engine (verb)',
            confidence: 0.94,
            intuition: `"${word}" drives the action in this clause.`
          }
        : idx > 2
        ? {
            id: 'receiver',
            label: 'Receiver (object)',
            confidence: 0.91,
            intuition: `"${word}" receives the action.`
          }
        : {
            id: 'doer',
            label: 'Doer (subject)',
            confidence: 0.94,
            intuition: `"${word}" anchors the action.`
          };
      return {
        ok: true,
        data: result,
        trace: {
          'step-morph': { status: isVerb ? 'taken' : 'skipped', output: isVerb ? 'matched verb suffix' : 'not verb/prep' },
          'step-pos': { status: 'ok', output: `${result.id} (${result.label})` }
        }
      };
    }
  },
  {
    id: 'dispatch-webhook-event',
    name: 'dispatchWebhookEvent',
    module: 'events/delivery',
    version: '2.0.4',
    runtime: 'node20',
    purity: 'side-effect',
    timeoutMs: 5000,
    permissions: ['net:outbound', 'secrets:read:WEBHOOK_SIGNING_SECRET'],
    description:
      'Validates HTTPS endpoint, signs event payload with HMAC-SHA256, short-circuits on dryRun, or delivers with retry backoff.',
    tags: ['network', 'security', 'webhooks'],
    calledBy: [
      {
        fnId: 'grade-sentence-exercise',
        name: 'gradeSentenceExercise',
        module: 'study/grading.ts:17',
        callSite: 'const receipt = await dispatchWebhookEvent({ eventType: "exercise.graded" })'
      }
    ],
    parameters: [
      {
        name: 'endpointUrl',
        type: 'string (https)',
        required: true,
        defaultValue: 'https://hooks.example.dev/v1/ingest',
        description: 'Destination HTTPS webhook URL.'
      },
      {
        name: 'eventType',
        type: 'string',
        required: true,
        defaultValue: 'deck.completed',
        description: 'Canonical dot-separated event topic.'
      },
      {
        name: 'maxRetries',
        type: 'number',
        required: false,
        defaultValue: 3,
        description: 'Maximum exponential backoff attempts on 5xx responses.'
      },
      {
        name: 'dryRun',
        type: 'boolean',
        required: false,
        defaultValue: true,
        description: 'Sign and validate payload without sending over the network.'
      }
    ],
    returns: {
      type: 'Promise<DeliveryReceipt>',
      fields: [
        { name: 'deliveryId', type: 'string' },
        { name: 'signatureHeader', type: 'string' },
        { name: 'attemptsQueued', type: 'number' },
        { name: 'status', type: "'simulated' | 'delivered'" }
      ]
    },
    errors: [
      { code: 'INSECURE_PROTOCOL', status: 400, when: 'endpointUrl does not start with https://' }
    ],
    steps: [
      {
        id: 'step-protocol',
        kind: 'branch',
        title: 'Validate HTTPS protocol',
        code: 'if (!params.endpointUrl.startsWith("https://")) throw new FunctionError("INSECURE_PROTOCOL")',
        inputs: ['params.endpointUrl'],
        output: 'throws INSECURE_PROTOCOL | continues',
        detail: 'Rejects plain HTTP or malformed URLs before signing.'
      },
      {
        id: 'step-sign',
        kind: 'call',
        title: 'Generate HMAC-SHA256 signature',
        code: 'const signature = signHmacSha256(`${timestamp}.${params.eventType}`, env.WEBHOOK_SIGNING_SECRET)',
        inputs: ['timestamp', 'params.eventType', 'env.WEBHOOK_SIGNING_SECRET'],
        output: 'signature: string',
        detail: 'Binds Unix timestamp and event topic with the workspace signing secret.'
      },
      {
        id: 'step-dryrun',
        kind: 'branch',
        title: 'Check dryRun flag',
        code: 'if (params.dryRun) return { deliveryId, signatureHeader, status: "simulated" }',
        inputs: ['params.dryRun', 'signature'],
        output: 'simulated receipt | continues',
        detail: 'Short-circuits network delivery when dryRun is enabled.'
      },
      {
        id: 'step-send',
        kind: 'call',
        title: 'Deliver payload with retry backoff',
        code: 'return await sendWithBackoff(params, signature)',
        inputs: ['params', 'signature'],
        output: 'Promise<DeliveryReceipt>',
        detail: 'Sends signed POST request and queues up to maxRetries attempts on 5xx.'
      }
    ],
    sourceCode: `export async function dispatchWebhookEvent(params: {
  endpointUrl: string;
  eventType: string;
  maxRetries?: number;
  dryRun?: boolean;
}): Promise<DeliveryReceipt> {
  // 1. Enforce TLS transport
  if (!params.endpointUrl.startsWith('https://')) {
    throw new FunctionError('INSECURE_PROTOCOL');
  }

  // 2. Sign timestamp + event payload
  const timestamp = Math.floor(Date.now() / 1000);
  const signature = signHmacSha256(
    \`\${timestamp}.\${params.eventType}\`,
    env.WEBHOOK_SIGNING_SECRET
  );

  // 3. Short-circuit if dryRun is active
  if (params.dryRun) {
    return {
      deliveryId: createId('dlv'),
      signatureHeader: \`t=\${timestamp},v1=\${signature}\`,
      attemptsQueued: params.maxRetries ?? 3,
      status: 'simulated'
    };
  }

  // 4. Perform outbound POST with backoff
  return await sendWithBackoff(params, signature);
}`,
    execute: (args) => {
      const url = String(args.endpointUrl ?? '');
      if (!url.startsWith('https://')) {
        return {
          ok: false,
          error: 'INSECURE_PROTOCOL (400): endpointUrl must start with https://',
          trace: {
            'step-protocol': { status: 'thrown', output: 'FunctionError("INSECURE_PROTOCOL")' }
          }
        };
      }
      const ts = Math.floor(Date.now() / 1000);
      return {
        ok: true,
        data: {
          deliveryId: `dlv_${Math.random().toString(16).slice(2, 10)}`,
          signatureHeader: `t=${ts},v1=9f4c2b8e11a04d7c`,
          attemptsQueued: Number(args.maxRetries ?? 3),
          status: args.dryRun ? 'simulated' : 'delivered'
        },
        trace: {
          'step-protocol': { status: 'passed', output: 'https:// verified' },
          'step-sign': { status: 'ok', output: `t=${ts},v1=9f4c2b8e...` },
          'step-dryrun': { status: args.dryRun ? 'taken' : 'skipped', output: args.dryRun ? 'simulated' : 'live send' },
          'step-send': { status: args.dryRun ? 'skipped' : 'ok', output: args.dryRun ? 'not called' : '202 Accepted' }
        }
      };
    }
  }
];

function buildDefaultArgs(fn) {
  const initial = {};
  (fn?.parameters || []).forEach((p) => {
    initial[p.name] = p.defaultValue ?? '';
  });
  return initial;
}

function defaultRunPreview(fn, args, allFns) {
  if (typeof fn.execute === 'function') {
    return fn.execute(args, allFns);
  }
  const trace = {};
  (fn.steps || []).forEach((step) => {
    trace[step.id] = {
      status: 'simulated',
      output: step.output || 'completed'
    };
  });
  return {
    ok: true,
    data: {
      simulated: true,
      function: fn.name,
      inputs: args,
      expectedReturnType: fn.returns?.type || 'unknown'
    },
    trace
  };
}

export function FunctionExplorer({ graph, initialFunctionId, onInvoke }) {
  const functionsList = useMemo(() => {
    if (graph && Array.isArray(graph.functions) && graph.functions.length > 0) {
      return graph.functions;
    }
    return SAMPLE_FUNCTIONS;
  }, [graph]);

  const defaultSelected = useMemo(() => {
    if (initialFunctionId && functionsList.some((f) => f.id === initialFunctionId)) {
      return initialFunctionId;
    }
    const preferred = functionsList.find((f) => f.id === 'evaluate-grammar-role');
    return preferred ? preferred.id : functionsList[0]?.id;
  }, [functionsList, initialFunctionId]);

  const [selectedId, setSelectedId] = useState(defaultSelected);
  const [navTrail, setNavTrail] = useState(() => {
    if (defaultSelected === 'evaluate-grammar-role' && functionsList.some((f) => f.id === 'grade-sentence-exercise')) {
      return ['grade-sentence-exercise', 'evaluate-grammar-role'];
    }
    return defaultSelected ? [defaultSelected] : [];
  });

  // View mode toggle: 'split' | 'raw' | 'preview' | 'map'
  const [viewMode, setViewMode] = useState('split');
  // Sub-toggle inside raw pane: 'source' | 'manifest'
  const [rawFormat, setRawFormat] = useState('source');
  const [copied, setCopied] = useState(false);

  const [expanded, setExpanded] = useState({
    root: true,
    calledBy: true,
    parameters: true,
    steps: true,
    returns: true,
    errors: false,
    testRunner: true
  });

  const activeFn = useMemo(
    () => functionsList.find((f) => f.id === selectedId) || functionsList[0],
    [functionsList, selectedId]
  );

  // Downstream functions invoked by activeFn's execution steps
  const downstreamCalls = useMemo(() => {
    const ids = new Set();
    (activeFn?.steps || []).forEach((s) => {
      if (s.callsFnId) ids.add(s.callsFnId);
    });
    return Array.from(ids)
      .map((id) => functionsList.find((f) => f.id === id))
      .filter(Boolean);
  }, [activeFn, functionsList]);

  const [argValues, setArgValues] = useState(() => buildDefaultArgs(activeFn));
  const [lastRun, setLastRun] = useState(() =>
    defaultRunPreview(activeFn, buildDefaultArgs(activeFn), functionsList)
  );
  const [runDurationMs, setRunDurationMs] = useState(0.4);
  const [isRunning, setIsRunning] = useState(false);

  useEffect(() => {
    if (!functionsList.some((f) => f.id === selectedId) && functionsList[0]) {
      setSelectedId(functionsList[0].id);
      setNavTrail([functionsList[0].id]);
    }
  }, [functionsList, selectedId]);

  const navigateToFunction = (fnId, pushToTrail = true) => {
    const target = functionsList.find((f) => f.id === fnId);
    if (!target) return;
    setSelectedId(target.id);
    const defaults = buildDefaultArgs(target);
    setArgValues(defaults);
    setLastRun(defaultRunPreview(target, defaults, functionsList));
    if (pushToTrail) {
      setNavTrail((prev) => {
        const existingIdx = prev.indexOf(target.id);
        if (existingIdx !== -1) return prev.slice(0, existingIdx + 1);
        return [...prev, target.id];
      });
    } else {
      setNavTrail([target.id]);
    }
  };

  const toggleNode = (key) => {
    setExpanded((prev) => ({ ...prev, [key]: !prev[key] }));
  };

  const rawText = useMemo(() => {
    if (!activeFn) return '';
    if (rawFormat === 'source') {
      return activeFn.sourceCode || '// Source code unavailable';
    }
    const manifest = {
      id: activeFn.id,
      name: activeFn.name,
      module: activeFn.module,
      version: activeFn.version || '1.0.0',
      purity: activeFn.purity || 'pure',
      calledBy: activeFn.calledBy || [],
      parameters: activeFn.parameters || [],
      steps: (activeFn.steps || []).map(({ id, kind, title, code, inputs, output, callsFnId }) => ({
        id,
        kind,
        title,
        code,
        inputs,
        output,
        ...(callsFnId ? { callsFnId } : {})
      })),
      returns: activeFn.returns || { type: 'void', fields: [] },
      errors: activeFn.errors || []
    };
    return JSON.stringify(manifest, null, 2);
  }, [activeFn, rawFormat]);

  const handleCopy = () => {
    navigator.clipboard?.writeText(rawText);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const handleInvoke = async () => {
    if (!activeFn) return;
    setIsRunning(true);
    const start = performance.now();
    try {
      let res;
      if (typeof onInvoke === 'function') {
        res = await onInvoke(activeFn, argValues);
      } else {
        res = defaultRunPreview(activeFn, argValues, functionsList);
      }
      const elapsed = Math.max(0.1, Number((performance.now() - start).toFixed(2)));
      setRunDurationMs(elapsed);
      setLastRun(res);
      setExpanded((prev) => ({ ...prev, steps: true, testRunner: true }));
    } catch (err) {
      setLastRun({
        ok: false,
        error: err?.message || String(err),
        trace: {}
      });
    } finally {
      setIsRunning(false);
    }
  };

  const handleResetArgs = () => {
    const defaults = buildDefaultArgs(activeFn);
    setArgValues(defaults);
    setLastRun(defaultRunPreview(activeFn, defaults, functionsList));
  };

  if (!activeFn) {
    return (
      <div className="min-h-screen bg-[#121212] text-neutral-300 flex items-center justify-center p-6 font-mono text-sm">
        No functions found in graph manifest. Run `fnscope scan ./src` first.
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-[#121212] text-neutral-200 flex flex-col font-sans selection:bg-emerald-500/30">
      {/* Top Toolbar: fnscope brand + Function selector + Raw / Preview / Split / Map toggle */}
      <header className="border-b border-[#262626] bg-[#161616] px-4 py-2.5 flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2.5 overflow-x-auto">
          <div className="flex items-center gap-1.5 pr-2.5 mr-1 border-r border-[#2b2b2b] shrink-0">
            <span className="w-2 h-2 rounded-full bg-emerald-400" />
            <span className="font-mono text-xs font-semibold tracking-tight text-white">
              fnscope
            </span>
          </div>
          {functionsList.map((fn) => {
            const isSelected = fn.id === activeFn.id;
            return (
              <button
                key={fn.id}
                onClick={() => navigateToFunction(fn.id, false)}
                className={`px-3 py-1.5 rounded-lg text-xs font-mono transition-colors flex items-center gap-2 cursor-pointer shrink-0 ${
                  isSelected
                    ? 'bg-[#262626] text-white border border-[#3a3a3a]'
                    : 'text-neutral-400 hover:text-neutral-200 hover:bg-[#1e1e1e] border border-transparent'
                }`}
              >
                <span
                  className={`w-1.5 h-1.5 rounded-full ${
                    fn.purity === 'pure' ? 'bg-emerald-400' : 'bg-amber-400'
                  }`}
                />
                {fn.name}()
              </button>
            );
          })}
        </div>

        {/* View Toggle Pill */}
        <div className="flex items-center bg-[#1e1e1e] p-1 rounded-lg border border-[#2c2c2c]">
          <button
            onClick={() => setViewMode('raw')}
            className={`px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
              viewMode === 'raw'
                ? 'bg-[#2e2e2e] text-white shadow-sm'
                : 'text-neutral-400 hover:text-neutral-200'
            }`}
          >
            <Code2 className="w-3.5 h-3.5" />
            Raw
          </button>
          <button
            onClick={() => setViewMode('split')}
            className={`px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
              viewMode === 'split'
                ? 'bg-[#2e2e2e] text-white shadow-sm'
                : 'text-neutral-400 hover:text-neutral-200'
            }`}
          >
            <Columns className="w-3.5 h-3.5" />
            Split
          </button>
          <button
            onClick={() => setViewMode('preview')}
            className={`px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
              viewMode === 'preview'
                ? 'bg-[#2e2e2e] text-white shadow-sm'
                : 'text-neutral-400 hover:text-neutral-200'
            }`}
          >
            <Eye className="w-3.5 h-3.5" />
            Preview
          </button>
          <button
            onClick={() => setViewMode('map')}
            className={`px-2.5 py-1 rounded-md text-xs font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
              viewMode === 'map'
                ? 'bg-[#2e2e2e] text-white shadow-sm'
                : 'text-neutral-400 hover:text-neutral-200'
            }`}
          >
            <Network className="w-3.5 h-3.5" />
            1-Hop Map
          </button>
        </div>
      </header>

      {/* Main Workspace */}
      <main
        className={`flex-1 grid grid-cols-1 ${
          viewMode === 'split' ? 'lg:grid-cols-2' : 'grid-cols-1'
        } divide-y lg:divide-y-0 lg:divide-x divide-[#262626]`}
      >
        {/* 1-HOP NEIGHBORHOOD MAP VIEW */}
        {viewMode === 'map' && (
          <section className="flex flex-col bg-[#161616] p-6 overflow-auto">
            <div className="max-w-5xl w-full mx-auto space-y-6">
              <div className="flex items-center justify-between flex-wrap gap-2 border-b border-[#252525] pb-4">
                <div>
                  <h2 className="text-base font-medium text-neutral-100">
                    1-Hop Call Neighborhood: <span className="font-mono text-emerald-400">{activeFn.name}()</span>
                  </h2>
                  <p className="text-xs text-neutral-400 mt-0.5">
                    Click any upstream caller or downstream callee to traverse the codebase graph.
                  </p>
                </div>
                <span className="text-xs font-mono px-2.5 py-1 rounded-full bg-[#222222] text-neutral-300 border border-[#303030]">
                  {(activeFn.calledBy || []).length} callers · {downstreamCalls.length} callees
                </span>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-3 gap-6 items-center">
                {/* Column 1: Upstream Callers */}
                <div className="space-y-3">
                  <div className="text-xs font-mono uppercase tracking-wider text-neutral-400 flex items-center gap-1.5">
                    <ArrowUpRight className="w-3.5 h-3.5 text-sky-400" />
                    Upstream Callers ({(activeFn.calledBy || []).length})
                  </div>
                  {(activeFn.calledBy || []).length === 0 ? (
                    <div className="p-4 rounded-xl bg-[#1b1b1b] border border-dashed border-[#2e2e2e] text-xs text-neutral-500">
                      No internal callers detected (entry point or standalone utility).
                    </div>
                  ) : (
                    (activeFn.calledBy || []).map((caller, i) => (
                      <button
                        key={i}
                        disabled={!caller.fnId}
                        onClick={() => caller.fnId && navigateToFunction(caller.fnId, true)}
                        className={`w-full text-left p-3.5 rounded-xl border transition-all space-y-1.5 ${
                          caller.fnId
                            ? 'bg-[#1c1c1c] border-[#2e2e2e] hover:border-sky-500/50 cursor-pointer'
                            : 'bg-[#191919] border-[#262626] cursor-default'
                        }`}
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="font-mono text-xs font-medium text-sky-300">
                            {caller.name}
                          </span>
                          <span className="text-[10px] font-mono text-neutral-500">
                            {caller.module}
                          </span>
                        </div>
                        <div className="font-mono text-[11px] text-neutral-400 bg-[#131313] px-2 py-1 rounded truncate">
                          {caller.callSite}
                        </div>
                      </button>
                    ))
                  )}
                </div>

                {/* Column 2: Active Focus Function */}
                <div className="p-5 rounded-2xl bg-[#1f1f1f] border-2 border-emerald-500/50 shadow-lg space-y-3">
                  <div className="flex items-center justify-between">
                    <span className="text-[11px] font-mono text-emerald-400 uppercase tracking-wider">
                      Focused Node
                    </span>
                    <span className="text-[11px] font-mono text-neutral-400">
                      {activeFn.module}
                    </span>
                  </div>
                  <div className="font-mono text-base font-semibold text-white">
                    {activeFn.name}()
                  </div>
                  <p className="text-xs text-neutral-300 leading-relaxed">
                    {activeFn.description}
                  </p>
                  <div className="pt-2 flex items-center justify-between border-t border-[#2d2d2d]">
                    <span className="text-[11px] font-mono text-neutral-400">
                      {(activeFn.steps || []).length} execution steps
                    </span>
                    <button
                      onClick={() => setViewMode('split')}
                      className="text-xs font-medium text-emerald-400 hover:text-emerald-300 cursor-pointer"
                    >
                      Inspect steps →
                    </button>
                  </div>
                </div>

                {/* Column 3: Downstream Callees */}
                <div className="space-y-3">
                  <div className="text-xs font-mono uppercase tracking-wider text-neutral-400 flex items-center gap-1.5">
                    <ArrowDownRight className="w-3.5 h-3.5 text-emerald-400" />
                    Downstream Calls ({downstreamCalls.length})
                  </div>
                  {downstreamCalls.length === 0 ? (
                    <div className="p-4 rounded-xl bg-[#1b1b1b] border border-dashed border-[#2e2e2e] text-xs text-neutral-500">
                      Leaf function — calls no other internal modules.
                    </div>
                  ) : (
                    downstreamCalls.map((callee) => (
                      <button
                        key={callee.id}
                        onClick={() => navigateToFunction(callee.id, true)}
                        className="w-full text-left p-3.5 rounded-xl bg-[#1c1c1c] border border-[#2e2e2e] hover:border-emerald-500/50 transition-all space-y-1.5 cursor-pointer"
                      >
                        <div className="flex items-center justify-between gap-2">
                          <span className="font-mono text-xs font-medium text-emerald-300">
                            {callee.name}()
                          </span>
                          <span className="text-[10px] font-mono text-neutral-500">
                            {callee.module}
                          </span>
                        </div>
                        <p className="text-[11px] text-neutral-400 line-clamp-2">
                          {callee.description}
                        </p>
                      </button>
                    ))
                  )}
                </div>
              </div>
            </div>
          </section>
        )}

        {/* LEFT PANE: Raw Function Source / Manifest */}
        {(viewMode === 'raw' || viewMode === 'split') && (
          <section className="flex flex-col bg-[#161616] min-h-[420px]">
            <div className="px-4 py-3 border-b border-[#242424] flex items-center justify-between gap-2">
              <div className="flex items-center gap-3">
                <h2 className="text-sm font-medium text-neutral-200">
                  Function source
                </h2>
                <div className="flex items-center bg-[#202020] rounded-md p-0.5 border border-[#2d2d2d]">
                  <button
                    onClick={() => setRawFormat('source')}
                    className={`px-2 py-0.5 text-[11px] rounded transition-colors cursor-pointer ${
                      rawFormat === 'source'
                        ? 'bg-[#2f2f2f] text-neutral-100'
                        : 'text-neutral-400 hover:text-neutral-200'
                    }`}
                  >
                    .ts
                  </button>
                  <button
                    onClick={() => setRawFormat('manifest')}
                    className={`px-2 py-0.5 text-[11px] rounded transition-colors cursor-pointer ${
                      rawFormat === 'manifest'
                        ? 'bg-[#2f2f2f] text-neutral-100'
                        : 'text-neutral-400 hover:text-neutral-200'
                    }`}
                  >
                    .json
                  </button>
                </div>
              </div>

              <button
                onClick={handleCopy}
                className="text-xs text-neutral-400 hover:text-neutral-200 flex items-center gap-1.5 px-2.5 py-1 rounded-md hover:bg-[#222222] transition-colors cursor-pointer"
              >
                {copied ? (
                  <>
                    <Check className="w-3.5 h-3.5 text-emerald-400" />
                    <span className="text-emerald-400">Copied</span>
                  </>
                ) : (
                  <>
                    <Copy className="w-3.5 h-3.5" />
                    <span>Copy</span>
                  </>
                )}
              </button>
            </div>

            <div className="flex-1 p-4 overflow-auto">
              <pre className="font-mono text-[13px] leading-6 text-neutral-200 whitespace-pre overflow-x-auto">
                <code>{rawText}</code>
              </pre>
            </div>
          </section>
        )}

        {/* RIGHT PANE: Explorer Preview */}
        {(viewMode === 'preview' || viewMode === 'split') && (
          <section className="flex flex-col bg-[#181818] min-h-[420px]">
            <div className="px-4 py-3 border-b border-[#242424] flex items-center justify-between">
              <h2 className="text-sm font-medium text-neutral-200">
                Explorer preview
              </h2>
              <span className="text-[11px] font-mono text-neutral-500">
                {activeFn.module}
              </span>
            </div>

            {/* Breadcrumb bar */}
            <div className="px-5 py-2.5 border-b border-[#222222] flex items-center justify-between gap-2 flex-wrap text-xs font-mono text-neutral-400">
              <div className="flex items-center gap-1.5 flex-wrap">
                <span className="text-neutral-500">call-path:</span>
                {navTrail.map((fnId, idx) => {
                  const item = functionsList.find((f) => f.id === fnId);
                  if (!item) return null;
                  const isCurrent = idx === navTrail.length - 1;
                  return (
                    <React.Fragment key={`${fnId}-${idx}`}>
                      {idx > 0 && <span className="text-neutral-600">→</span>}
                      <button
                        onClick={() => navigateToFunction(item.id, true)}
                        className={`px-1.5 py-0.5 rounded transition-colors cursor-pointer ${
                          isCurrent
                            ? 'bg-[#262626] text-emerald-300 font-medium'
                            : 'text-neutral-400 hover:text-neutral-200 hover:bg-[#202020]'
                        }`}
                      >
                        {item.name}()
                      </button>
                    </React.Fragment>
                  );
                })}
              </div>
              {navTrail.length > 1 && (
                <button
                  onClick={() => navigateToFunction(navTrail[navTrail.length - 2], true)}
                  className="text-[11px] text-neutral-400 hover:text-neutral-200 flex items-center gap-1 cursor-pointer"
                >
                  <CornerUpLeft className="w-3 h-3" />
                  Back
                </button>
              )}
            </div>

            {/* Structured Explorer Tree */}
            <div className="flex-1 p-5 overflow-y-auto space-y-4">
              <div>
                <button
                  onClick={() => toggleNode('root')}
                  className="w-full flex items-center justify-between text-left group cursor-pointer"
                >
                  <div className="flex items-center gap-2">
                    {expanded.root ? (
                      <ChevronDown className="w-4 h-4 text-neutral-400" />
                    ) : (
                      <ChevronRight className="w-4 h-4 text-neutral-400" />
                    )}
                    <span className="text-base font-medium text-neutral-100">
                      {activeFn.name}
                    </span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-sky-950/50 text-sky-300 border border-sky-800/40">
                      {(activeFn.calledBy || []).length} callers
                    </span>
                    <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                      {(activeFn.steps || []).length} steps
                    </span>
                  </div>
                </button>

                {expanded.root && (
                  <div className="pl-6 mt-3 space-y-3 text-sm">
                    {/* id */}
                    <div className="grid grid-cols-[135px_1fr] items-center gap-3">
                      <span className="text-neutral-400">id</span>
                      <div className="flex items-center gap-2.5 flex-wrap">
                        <span className="font-mono text-neutral-300">{activeFn.id}</span>
                        {activeFn.runtime && (
                          <span className="text-[11px] px-2 py-0.5 rounded-full border border-[#323232] bg-[#202020] text-neutral-400">
                            {activeFn.runtime}
                          </span>
                        )}
                      </div>
                    </div>

                    {/* purity */}
                    <div className="grid grid-cols-[135px_1fr] items-center gap-3">
                      <span className="text-neutral-400">purity</span>
                      <div className="flex items-center gap-2">
                        {activeFn.purity === 'pure' ? (
                          <span className="inline-flex items-center gap-1.5 text-xs font-mono px-2.5 py-0.5 rounded-full bg-emerald-950/60 text-emerald-300 border border-emerald-700/40">
                            <Zap className="w-3 h-3" />
                            pure / deterministic
                          </span>
                        ) : (
                          <span className="inline-flex items-center gap-1.5 text-xs font-mono px-2.5 py-0.5 rounded-full bg-amber-950/60 text-amber-300 border border-amber-700/40">
                            <Shield className="w-3 h-3" />
                            side-effect ({activeFn.timeoutMs || 5000}ms timeout)
                          </span>
                        )}
                      </div>
                    </div>

                    {/* description */}
                    <div className="grid grid-cols-[135px_1fr] items-start gap-3">
                      <span className="text-neutral-400 pt-0.5">description</span>
                      <p className="text-neutral-100 leading-relaxed">
                        {activeFn.description}
                      </p>
                    </div>

                    {/* version */}
                    <div className="grid grid-cols-[135px_1fr] items-center gap-3">
                      <span className="text-neutral-400">version</span>
                      <span className="font-mono text-neutral-200">{activeFn.version || '1.0.0'}</span>
                    </div>

                    {/* capabilities */}
                    {activeFn.permissions && activeFn.permissions.length > 0 && (
                      <div className="grid grid-cols-[135px_1fr] items-start gap-3">
                        <span className="text-neutral-400 pt-0.5">capabilities</span>
                        <div className="flex flex-wrap gap-1.5">
                          {activeFn.permissions.map((perm) => (
                            <span
                              key={perm}
                              className="font-mono text-xs px-2 py-0.5 rounded bg-[#232323] text-neutral-300 border border-[#303030]"
                            >
                              {perm}
                            </span>
                          ))}
                        </div>
                      </div>
                    )}

                    {/* calledBy */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('calledBy')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.calledBy ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <ArrowUpRight className="w-3.5 h-3.5 text-sky-400" />
                          <span className="text-sm font-medium text-neutral-200">
                            calledBy
                          </span>
                        </div>
                        <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                          {(activeFn.calledBy || []).length}
                        </span>
                      </button>

                      {expanded.calledBy && (
                        <div className="pl-5 mt-2.5 space-y-2">
                          {(activeFn.calledBy || []).length === 0 ? (
                            <div className="text-xs text-neutral-500 italic">
                              No upstream callers recorded (root or standalone function).
                            </div>
                          ) : (
                            (activeFn.calledBy || []).map((caller, idx) => (
                              <div
                                key={idx}
                                className="p-2.5 rounded-lg bg-[#1d1d1d] border border-[#292929] flex items-center justify-between gap-2 flex-wrap"
                              >
                                <div className="space-y-1 min-w-0">
                                  <div className="flex items-center gap-2">
                                    {caller.fnId ? (
                                      <button
                                        onClick={() => navigateToFunction(caller.fnId, true)}
                                        className="font-mono text-xs text-sky-300 hover:text-sky-200 underline decoration-sky-500/40 underline-offset-2 cursor-pointer"
                                      >
                                        {caller.name}()
                                      </button>
                                    ) : (
                                      <span className="font-mono text-xs text-neutral-200">
                                        {caller.name}
                                      </span>
                                    )}
                                    <span className="font-mono text-[11px] text-neutral-500">
                                      {caller.module}
                                    </span>
                                  </div>
                                  <div className="font-mono text-[11px] text-neutral-400 truncate">
                                    {caller.callSite}
                                  </div>
                                </div>
                                {caller.fnId && (
                                  <button
                                    onClick={() => navigateToFunction(caller.fnId, true)}
                                    className="text-[11px] font-mono px-2 py-1 rounded bg-[#252525] hover:bg-[#2f2f2f] text-sky-300 transition-colors cursor-pointer shrink-0"
                                  >
                                    Jump ↑
                                  </button>
                                )}
                              </div>
                            ))
                          )}
                        </div>
                      )}
                    </div>

                    {/* parameters */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('parameters')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.parameters ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <span className="text-sm font-medium text-neutral-200">
                            parameters
                          </span>
                        </div>
                        <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                          {(activeFn.parameters || []).length}
                        </span>
                      </button>

                      {expanded.parameters && (
                        <div className="pl-5 mt-2.5 space-y-2.5">
                          {(activeFn.parameters || []).map((param) => (
                            <div
                              key={param.name}
                              className="p-2.5 rounded-lg bg-[#1d1d1d] border border-[#292929] space-y-1"
                            >
                              <div className="flex items-center justify-between gap-2">
                                <div className="flex items-center gap-2">
                                  <span className="font-mono text-xs text-emerald-300">
                                    {param.name}
                                  </span>
                                  <span className="font-mono text-[11px] text-neutral-400">
                                    : {param.type}
                                  </span>
                                </div>
                                <span
                                  className={`text-[10px] font-mono px-1.5 py-0.5 rounded ${
                                    param.required
                                      ? 'bg-rose-950/60 text-rose-300 border border-rose-800/40'
                                      : 'bg-[#262626] text-neutral-400'
                                  }`}
                                >
                                  {param.required ? 'required' : 'optional'}
                                </span>
                              </div>
                              {param.description && (
                                <p className="text-xs text-neutral-400">
                                  {param.description}
                                </p>
                              )}
                            </div>
                          ))}
                        </div>
                      )}
                    </div>

                    {/* executionSteps */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('steps')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.steps ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <GitBranch className="w-3.5 h-3.5 text-emerald-400" />
                          <span className="text-sm font-medium text-neutral-200">
                            executionSteps
                          </span>
                        </div>
                        <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                          {(activeFn.steps || []).length}
                        </span>
                      </button>

                      {expanded.steps && (
                        <div className="pl-5 mt-3 space-y-2.5 relative before:content-[''] before:absolute before:left-[28px] before:top-3 before:bottom-3 before:w-px before:bg-[#2c2c2c]">
                          {(activeFn.steps || []).map((step, idx) => {
                            const traceEntry = lastRun?.trace?.[step.id];
                            const kindBadge =
                              step.kind === 'branch'
                                ? 'bg-amber-950/60 text-amber-300 border-amber-700/40'
                                : step.kind === 'return'
                                ? 'bg-emerald-950/60 text-emerald-300 border-emerald-700/40'
                                : step.kind === 'call'
                                ? 'bg-sky-950/60 text-sky-300 border-sky-700/40'
                                : 'bg-purple-950/60 text-purple-300 border-purple-700/40';

                            const traceColor =
                              traceEntry?.status === 'thrown'
                                ? 'text-rose-400'
                                : traceEntry?.status === 'miss' || traceEntry?.status === 'skipped'
                                ? 'text-amber-400'
                                : 'text-emerald-400';

                            const targetCallee = step.callsFnId
                              ? functionsList.find((f) => f.id === step.callsFnId)
                              : null;

                            return (
                              <div
                                key={step.id}
                                className="relative flex items-start gap-3 pl-1"
                              >
                                <span className="z-10 mt-2 w-4 h-4 rounded-full bg-[#1e1e1e] border border-[#3a3a3a] text-[10px] font-mono text-neutral-300 flex items-center justify-center shrink-0">
                                  {idx + 1}
                                </span>

                                <div className="flex-1 p-2.5 rounded-lg bg-[#1d1d1d] border border-[#2a2a2a] space-y-1.5">
                                  <div className="flex items-center justify-between gap-2 flex-wrap">
                                    <div className="flex items-center gap-2 flex-wrap">
                                      <span className="text-xs font-medium text-neutral-100">
                                        {step.title}
                                      </span>
                                      <span
                                        className={`text-[10px] font-mono px-1.5 py-0.5 rounded border ${kindBadge}`}
                                      >
                                        {step.kind}
                                      </span>
                                      {targetCallee && (
                                        <button
                                          onClick={() => navigateToFunction(targetCallee.id, true)}
                                          className="inline-flex items-center gap-1 text-[10px] font-mono px-2 py-0.5 rounded bg-emerald-950/70 hover:bg-emerald-900/70 text-emerald-300 border border-emerald-700/50 transition-colors cursor-pointer"
                                        >
                                          <ArrowDownRight className="w-3 h-3" />
                                          inspect {targetCallee.name}()
                                        </button>
                                      )}
                                    </div>
                                    <span className="font-mono text-[11px] text-neutral-400">
                                      → {step.output}
                                    </span>
                                  </div>

                                  <div className="font-mono text-[11px] text-emerald-300/90 bg-[#141414] px-2.5 py-1.5 rounded border border-[#252525] overflow-x-auto">
                                    {step.code}
                                  </div>

                                  <div className="flex items-center justify-between gap-2 flex-wrap text-[11px]">
                                    <span className="text-neutral-400">{step.detail}</span>
                                    <span className="font-mono text-neutral-500">
                                      reads: [{(step.inputs || []).join(', ')}]
                                    </span>
                                  </div>

                                  {traceEntry && (
                                    <div className="pt-1.5 mt-1 border-t border-[#282828] flex items-center justify-between gap-2 text-[11px] font-mono">
                                      <span className={traceColor}>
                                        ● {traceEntry.status}
                                      </span>
                                      <span className="text-neutral-200 truncate">
                                        {traceEntry.output}
                                      </span>
                                    </div>
                                  )}
                                </div>
                              </div>
                            );
                          })}
                        </div>
                      )}
                    </div>

                    {/* returns */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('returns')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.returns ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <span className="text-sm font-medium text-neutral-200">
                            returns
                          </span>
                          <span className="font-mono text-xs text-emerald-400">
                            {activeFn.returns?.type || 'void'}
                          </span>
                        </div>
                        <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                          {(activeFn.returns?.fields || []).length}
                        </span>
                      </button>

                      {expanded.returns && (
                        <div className="pl-5 mt-2 space-y-1.5">
                          {(activeFn.returns?.fields || []).map((field) => (
                            <div
                              key={field.name}
                              className="grid grid-cols-[135px_1fr] items-center gap-2 text-xs font-mono py-1"
                            >
                              <span className="text-neutral-300">{field.name}</span>
                              <span className="text-neutral-400">{field.type}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>

                    {/* throws */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('errors')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.errors ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <span className="text-sm font-medium text-neutral-200">
                            throws
                          </span>
                        </div>
                        <span className="text-[11px] font-mono px-2 py-0.5 rounded-full bg-[#242424] text-neutral-400">
                          {(activeFn.errors || []).length}
                        </span>
                      </button>

                      {expanded.errors && (
                        <div className="pl-5 mt-2 space-y-1.5">
                          {(activeFn.errors || []).map((err, i) => (
                            <div
                              key={`${err.code}-${i}`}
                              className="flex items-center justify-between gap-2 text-xs px-2.5 py-1.5 rounded bg-[#1d1d1d] border border-[#292929]"
                            >
                              <div className="flex items-center gap-2 font-mono">
                                <AlertTriangle className="w-3.5 h-3.5 text-amber-400 shrink-0" />
                                <span className="text-amber-300">{err.code}</span>
                              </div>
                              <span className="text-neutral-400">{err.when}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>

                    {/* Interactive Sandbox Invocation */}
                    <div className="pt-2 border-t border-[#242424]">
                      <button
                        onClick={() => toggleNode('testRunner')}
                        className="w-full flex items-center justify-between text-left cursor-pointer"
                      >
                        <div className="flex items-center gap-2">
                          {expanded.testRunner ? (
                            <ChevronDown className="w-3.5 h-3.5 text-neutral-400" />
                          ) : (
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400" />
                          )}
                          <Terminal className="w-3.5 h-3.5 text-emerald-400" />
                          <span className="text-sm font-medium text-neutral-200">
                            invokeSandbox
                          </span>
                        </div>
                        <span className="text-[11px] font-mono text-emerald-400">
                          live runner
                        </span>
                      </button>

                      {expanded.testRunner && (
                        <div className="pl-5 mt-3 space-y-3">
                          <div className="p-3 rounded-lg bg-[#1d1d1d] border border-[#2a2a2a] space-y-2.5">
                            {(activeFn.parameters || []).map((param) => (
                              <div
                                key={param.name}
                                className="grid grid-cols-[135px_1fr] items-center gap-2"
                              >
                                <label className="font-mono text-xs text-neutral-300 truncate">
                                  {param.name}
                                </label>
                                {typeof param.defaultValue === 'boolean' ? (
                                  <input
                                    type="checkbox"
                                    checked={Boolean(argValues[param.name])}
                                    onChange={(e) =>
                                      setArgValues((prev) => ({
                                        ...prev,
                                        [param.name]: e.target.checked
                                      }))
                                    }
                                    className="w-4 h-4 accent-emerald-500 rounded cursor-pointer"
                                  />
                                ) : (
                                  <input
                                    type={
                                      typeof param.defaultValue === 'number'
                                        ? 'number'
                                        : 'text'
                                    }
                                    step="any"
                                    value={argValues[param.name] ?? ''}
                                    onChange={(e) =>
                                      setArgValues((prev) => ({
                                        ...prev,
                                        [param.name]:
                                          typeof param.defaultValue === 'number'
                                            ? Number(e.target.value)
                                            : e.target.value
                                      }))
                                    }
                                    className="w-full bg-[#141414] border border-[#2f2f2f] focus:border-emerald-500/60 focus:outline-none rounded px-2.5 py-1 text-xs font-mono text-neutral-100"
                                  />
                                )}
                              </div>
                            ))}

                            <div className="pt-1 flex items-center justify-between gap-2">
                              <button
                                onClick={handleResetArgs}
                                className="text-xs text-neutral-400 hover:text-neutral-200 flex items-center gap-1 px-2 py-1 rounded hover:bg-[#262626] transition-colors cursor-pointer"
                              >
                                <RotateCcw className="w-3 h-3" />
                                Reset
                              </button>
                              <button
                                onClick={handleInvoke}
                                disabled={isRunning}
                                className="px-3 py-1.5 rounded-md bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 text-white text-xs font-medium flex items-center gap-1.5 transition-colors cursor-pointer"
                              >
                                <Play className="w-3 h-3 fill-current" />
                                {isRunning ? 'Running...' : `Run ${activeFn.name}()`}
                              </button>
                            </div>
                          </div>

                          {/* Output Console */}
                          {lastRun && (
                            <div className="rounded-lg bg-[#131313] border border-[#292929] overflow-hidden">
                              <div className="px-3 py-1.5 border-b border-[#222222] flex items-center justify-between text-[11px] font-mono">
                                <span
                                  className={
                                    lastRun.ok ? 'text-emerald-400' : 'text-rose-400'
                                  }
                                >
                                  {lastRun.ok ? '200 OK' : '422 THROWN'}
                                </span>
                                <span className="text-neutral-500">{runDurationMs} ms</span>
                              </div>
                              <pre className="p-3 text-xs font-mono text-neutral-200 overflow-x-auto">
                                {lastRun.ok
                                  ? JSON.stringify(lastRun.data, null, 2)
                                  : lastRun.error}
                              </pre>
                            </div>
                          )}
                        </div>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </div>
          </section>
        )}
      </main>
    </div>
  );
}

export default FunctionExplorer;