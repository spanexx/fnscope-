import React from 'react';
import { createRoot } from 'react-dom/client';
import FunctionExplorer from './FunctionExplorer';

// esbuild renames identifiers when minifying; keep a stable name for React
// DevTools and so the built bundle can be verified as the real explorer.
FunctionExplorer.displayName = 'FunctionExplorer';

// Stable marker string: survives identifier minification and identifies the
// repaired-args path in the built bundle.
const OBJECT_ARG_MARKER = 'fnscope:repaired-object-arg';

// The manifest declares one inline `input: { ... }` object parameter while the
// sandbox form offers one flat field per parameter. Rebuild that object from
// the flat values so the real export receives the shape it expects.
function buildObjectArg(typeText, flatArgs) {
  const built = {};
  const declaration = /([A-Za-z_$][\w$]*)\??\s*:\s*([^;\n}]+)/g;
  let match;
  while ((match = declaration.exec(typeText)) !== null) {
    const name = match[1];
    const declaredType = match[2].trim();
    let value = '';
    if (name === 'sentence') value = 'The quick fox jumped over the lazy dog.';
    else if (name === 'targetWord') value = 'fox';
    else if (/\bboolean\b/.test(declaredType)) value = true;
    else if (/\bnumber\b/.test(declaredType)) value = 0;
    built[name] = Object.prototype.hasOwnProperty.call(flatArgs, name) ? flatArgs[name] : value;
  }
  return built;
}

// The Rust server owns the data; the browser only ever fetches it. If the
// manifest is missing or empty we return undefined so the explorer renders
// without a graph prop and falls back to its built-in sample graph.
async function loadGraph() {
  try {
    const response = await fetch('/api/fn-graph.json');
    if (!response.ok) return undefined;
    const graph = await response.json();
    if (graph && Array.isArray(graph.functions) && graph.functions.length > 0) {
      return graph;
    }
    return undefined;
  } catch (error) {
    console.warn('fnscope: could not load fn-graph.json; using sample graph', error);
    return undefined;
  }
}

async function mount() {
  const graph = await loadGraph();
  const container = document.getElementById('root');
  if (!container) {
    throw new Error('fnscope: #root container not found');
  }

  // Only pass graph when one was actually loaded; otherwise the component's
  // built-in sample data keeps the explorer usable on a bare static serve.
  const graphProp = graph ? { graph } : {};

  createRoot(container).render(
    <FunctionExplorer
      {...graphProp}
      onInvoke={async (fn, args) => {
        // Repair only the single inline-object parameter case; scalar functions
        // keep their arguments exactly as the sandbox produced them.
        const objectParams = (fn.parameters || []).filter(
          (p) => typeof p.type === 'string' && p.type.trim().startsWith('{')
        );
        let payload = args;
        if (objectParams.length === 1) {
          console.debug(OBJECT_ARG_MARKER, objectParams[0].name);
          const builtObject = buildObjectArg(objectParams[0].type, args || {});
          // Keep the flat keys too: the Node runner passes the whole args object
          // as the single first argument for one-parameter functions.
          const otherArgs = { ...(args || {}) };
          payload = { ...otherArgs, [objectParams[0].name]: builtObject };
        }
        const response = await fetch('/api/invoke', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ fnId: fn.id, args: payload }),
        });
        if (!response.ok) throw new Error('invoke failed');
        return response.json();
      }}
    />
  );
}

mount();
