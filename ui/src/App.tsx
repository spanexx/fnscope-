import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Inbox } from 'lucide-react';
import AppShell from './components/AppShell';
import type { Crumb } from './components/BreadcrumbBar';
import EmptyState from './components/EmptyState';
import ProvenanceStrip from './components/ProvenanceStrip';
import { ErrorState, LoadingState } from './components/StatusState';
import { ThemeProvider } from './components/ThemeProvider';
import { ToastProvider, useToast } from './components/Toast';
import TopBar from './components/TopBar';
import { Explorer } from './explorer';
import type { ExplorerError } from './explorer';
import {
  appendCrumb,
  backTrail,
  crumbFor,
  currentCrumb,
  prependCrumb,
  startTrail,
  truncateTrail,
} from './explorer/trail';
import { InvokeDrawer } from './invoke';
import InvokeEmpty from './invoke/InvokeEmpty';
import { fetchScopeGraph, fetchScopeList } from './api';
import { indexFunctions, resolveDefaultFunctionId } from './lib/graph';
import { isTypingTarget, viewForShortcut } from './lib/shortcuts';
import type { FnGraph, ScopeDescriptor, ScopeList, ShellStatus, ViewId } from './lib/types';
import { pickInitialScopeId, rememberScopeId } from './state/scope';
import { useMediaQuery } from './state/useMediaQuery';
import CanvasViews from './views';

type Boot =
  | { status: 'loading' }
  | { status: 'error'; message: string; detail?: string }
  | {
      status: 'ready';
      list: ScopeList;
      compatibility: boolean;
      prefetched?: { scopeId: string; graph: FnGraph };
    };

type GraphLoad =
  | { status: 'idle' }
  | { status: 'loading' }
  | { status: 'ready'; scopeId: string; graph: FnGraph }
  | { status: 'error'; scopeId: string; message: string; detail?: string };

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError';
}

function WorkspaceApp() {
  const { push } = useToast();
  const [boot, setBoot] = useState<Boot>({ status: 'loading' });
  const [reloadKey, setReloadKey] = useState(0);
  const [scopeId, setScopeId] = useState<string | null>(null);
  const [graphLoad, setGraphLoad] = useState<GraphLoad>({ status: 'idle' });
  const [view, setView] = useState<ViewId>('story');
  const [search, setSearch] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [trail, setTrail] = useState<Crumb[]>([]);
  const [sidebarOpen, setSidebarOpen] = useState(() =>
    typeof window === 'undefined' ? true : window.innerWidth >= 980
  );
  const [drawerOpen, setDrawerOpen] = useState(() =>
    typeof window === 'undefined' ? false : window.innerWidth >= 1280
  );
  const [focusSearchRequested, setFocusSearchRequested] = useState(false);
  const [runSignal, setRunSignal] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const graphRequest = useRef(0);

  const narrowSidebar = useMediaQuery('(max-width: 979px)');
  const narrowDrawer = useMediaQuery('(max-width: 1179px)');

  useEffect(() => {
    if (narrowSidebar) setSidebarOpen(false);
  }, [narrowSidebar]);

  useEffect(() => {
    if (narrowDrawer) setDrawerOpen(false);
  }, [narrowDrawer]);

  useEffect(() => {
    let cancelled = false;
    const controller = new AbortController();
    setBoot({ status: 'loading' });
    setScopeId(null);
    setGraphLoad({ status: 'idle' });
    setSearch('');
    setSelectedId(null);
    setTrail([]);

    (async () => {
      try {
        const result = await fetchScopeList(controller.signal);
        if (cancelled) return;
        const initialId = pickInitialScopeId(result.list.scopes, result.list.defaultScope);
        if (!initialId) throw new Error('The workspace did not list an available scope.');
        rememberScopeId(initialId);
        setBoot({
          status: 'ready',
          list: result.list,
          compatibility: result.compatibility,
          prefetched:
            result.prefetched && result.prefetchedScopeId
              ? { scopeId: result.prefetchedScopeId, graph: result.prefetched }
              : undefined,
        });
        setScopeId(initialId);
      } catch (error) {
        if (cancelled || isAbortError(error)) return;
        setBoot({
          status: 'error',
          message: 'Could not load the workspace',
          detail: errorMessage(error),
        });
      }
    })();

    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [reloadKey]);

  useEffect(() => {
    if (boot.status !== 'ready' || !scopeId) return;
    const scope = boot.list.scopes.find((entry) => entry.id === scopeId);
    if (!scope) {
      setGraphLoad({
        status: 'error',
        scopeId,
        message: `Unknown scope: ${scopeId}`,
        detail: 'The selected scope is not present in /api/scopes.',
      });
      return;
    }

    if (boot.prefetched && boot.prefetched.scopeId === scopeId) {
      setGraphLoad({ status: 'ready', scopeId, graph: boot.prefetched.graph });
      return;
    }

    const controller = new AbortController();
    const request = ++graphRequest.current;
    setGraphLoad({ status: 'loading' });

    fetchScopeGraph(scope, controller.signal)
      .then((graph) => {
        if (controller.signal.aborted || graphRequest.current !== request) return;
        setGraphLoad({ status: 'ready', scopeId, graph });
      })
      .catch((error) => {
        if (controller.signal.aborted || graphRequest.current !== request) return;
        setGraphLoad({
          status: 'error',
          scopeId,
          message: `Could not load ${scope.label}`,
          detail: errorMessage(error),
        });
      });

    return () => controller.abort();
  }, [boot, scopeId]);

  useEffect(() => {
    if (!focusSearchRequested) return;
    setSidebarOpen(true);
    const frame = window.requestAnimationFrame(() => {
      const input = searchRef.current;
      if (input) {
        input.focus();
        input.select();
      }
      setFocusSearchRequested(false);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusSearchRequested]);

  const focusSearch = useCallback(() => setFocusSearchRequested(true), []);
  const requestReload = useCallback(() => setReloadKey((key) => key + 1), []);

  const scopes = boot.status === 'ready' ? boot.list.scopes : [];
  const scope = scopes.find((entry) => entry.id === scopeId) || null;
  const graph =
    graphLoad.status === 'ready' && graphLoad.scopeId === scopeId ? graphLoad.graph : null;
  const graphError =
    graphLoad.status === 'error' && graphLoad.scopeId === scopeId ? graphLoad : null;
  const compatibility = boot.status === 'ready' ? boot.compatibility : false;

  const graphIndex = useMemo(() => (graph ? indexFunctions(graph) : null), [graph]);
  const selectedFn = selectedId && graphIndex ? (graphIndex.get(selectedId) ?? null) : null;

  // A loaded manifest owns a fresh default selection and a one-crumb trail.
  useEffect(() => {
    if (!graph) {
      setSelectedId(null);
      setTrail([]);
      return;
    }
    const id = resolveDefaultFunctionId(graph, scope?.defaultFunctionId ?? null);
    const fn = id ? (graph.functions.find((entry) => entry.id === id) ?? null) : null;
    setSelectedId(id);
    setTrail(fn ? startTrail(crumbFor(fn)) : []);
  }, [graph, scope?.defaultFunctionId]);

  let status: ShellStatus = 'loading';
  if (boot.status === 'error') status = 'error';
  else if (boot.status === 'ready' && graphError) status = 'error';
  else if (graph) status = graph.functions.length > 0 ? 'ready' : 'empty';

  const error: ExplorerError | null =
    boot.status === 'error'
      ? { message: boot.message, detail: boot.detail }
      : graphError
        ? { message: graphError.message, detail: graphError.detail }
        : null;

  const selectFunction = useCallback(
    (id: string) => {
      const fn = graphIndex?.get(id);
      if (!fn) return;
      setSelectedId(id);
      setTrail(startTrail(crumbFor(fn)));
    },
    [graphIndex]
  );

  const followCaller = useCallback(
    (id: string) => {
      const fn = graphIndex?.get(id);
      if (!fn) return;
      setSelectedId(id);
      setTrail((prev) => prependCrumb(prev, crumbFor(fn)));
    },
    [graphIndex]
  );

  const followCallee = useCallback(
    (id: string) => {
      const fn = graphIndex?.get(id);
      if (!fn) return;
      setSelectedId(id);
      setTrail((prev) => appendCrumb(prev, crumbFor(fn)));
    },
    [graphIndex]
  );

  const selectTrail = useCallback(
    (index: number) => {
      const trimmed = truncateTrail(trail, index);
      if (trimmed === trail) return;
      setTrail(trimmed);
      setSelectedId(currentCrumb(trimmed)?.id ?? null);
    },
    [trail]
  );

  const goBack = useCallback(() => {
    if (trail.length <= 1) return;
    const next = backTrail(trail);
    setTrail(next);
    setSelectedId(currentCrumb(next)?.id ?? null);
  }, [trail]);

  const openInvoke = useCallback(() => {
    if (!selectedFn) {
      push('Select a function in the explorer, then open the invoke sandbox.', 'info');
      return;
    }
    setDrawerOpen(true);
  }, [selectedFn, push]);

  const requestRun = useCallback(() => {
    if (!selectedFn) {
      push('Select a function in the explorer, then run it here.', 'info');
      return;
    }
    if (!drawerOpen) {
      setDrawerOpen(true);
      return;
    }
    setRunSignal((signal) => signal + 1);
  }, [selectedFn, drawerOpen, push]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      const typing = isTypingTarget(event.target);
      const command = event.metaKey || event.ctrlKey;

      if (command && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        focusSearch();
        return;
      }

      if (command && event.key === 'Enter') {
        event.preventDefault();
        requestRun();
        return;
      }

      if (event.altKey && event.key === 'ArrowLeft' && !typing) {
        event.preventDefault();
        goBack();
        return;
      }

      if (event.key === 'Escape') {
        if (drawerOpen) {
          event.preventDefault();
          setDrawerOpen(false);
          return;
        }
        if (searchRef.current && document.activeElement === searchRef.current) {
          searchRef.current.blur();
          return;
        }
        if (narrowSidebar && sidebarOpen) {
          event.preventDefault();
          setSidebarOpen(false);
        }
        return;
      }

      if (typing || command || event.altKey) return;

      if (event.key === '/') {
        event.preventDefault();
        focusSearch();
        return;
      }

      const nextView = viewForShortcut(event.key);
      if (nextView) {
        event.preventDefault();
        setView(nextView);
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [drawerOpen, sidebarOpen, narrowSidebar, focusSearch, requestRun, goBack]);

  const selectScope = useCallback(
    (nextId: string) => {
      if (boot.status !== 'ready') return;
      const next = boot.list.scopes.find((scopeEntry) => scopeEntry.id === nextId);
      if (!next) {
        push(`Unknown scope: ${nextId}`, 'error');
        return;
      }
      if (!next.available) {
        push(`${next.label} is unavailable${next.error ? `: ${next.error}` : ''}`, 'error');
        return;
      }
      if (nextId === scopeId) return;
      rememberScopeId(nextId);
      setScopeId(nextId);
      setView('story');
      setSearch('');
      setSelectedId(null);
      setTrail([]);
      push(`Switched to ${next.label}`, 'info');
    },
    [boot, scopeId, push]
  );

  return (
    <AppShell
      topBar={
        <TopBar
          scopes={scopes}
          activeScope={scope}
          onSelectScope={selectScope}
          scopeMenuDisabled={boot.status !== 'ready'}
          view={view}
          onChangeView={setView}
          trail={trail}
          onSelectTrail={selectTrail}
          onBack={goBack}
          backDisabled={trail.length <= 1}
          sidebarOpen={sidebarOpen}
          onToggleSidebar={() => setSidebarOpen((open) => !open)}
          drawerOpen={drawerOpen}
          onToggleDrawer={() => setDrawerOpen((open) => !open)}
        />
      }
      sidebar={
        <Explorer
          graph={graph}
          selectedId={selectedId}
          onSelect={selectFunction}
          search={search}
          onSearchChange={setSearch}
          searchRef={searchRef}
          status={status}
          scope={scope}
          error={error}
          onRetry={requestReload}
        />
      }
      canvas={
        graph ? (
          <CanvasViews
            key={`${scopeId}:${selectedId ?? 'none'}`}
            graph={graph}
            fn={selectedFn}
            view={view}
            onCaller={followCaller}
            onCallee={followCallee}
            onSelect={selectFunction}
            onInvoke={openInvoke}
            trail={trail}
          />
        ) : (
          <CanvasStatus status={status} scope={scope} error={error} onRetry={requestReload} />
        )
      }
      drawer={
        scope && selectedFn ? (
          <InvokeDrawer
            key={`${scope.id}:${selectedFn.id}`}
            scope={scope}
            fn={selectedFn}
            compatibility={compatibility}
            runSignal={runSignal}
            onClose={() => setDrawerOpen(false)}
          />
        ) : (
          <InvokeEmpty
            scope={scope}
            compatibility={compatibility}
            status={status}
            onClose={() => setDrawerOpen(false)}
          />
        )
      }
      provenance={
        <ProvenanceStrip
          status={status}
          scope={scope}
          graph={graph}
          compatibility={compatibility}
        />
      }
      sidebarOpen={sidebarOpen}
      drawerOpen={drawerOpen}
      onCloseSidebar={() => setSidebarOpen(false)}
      onCloseDrawer={() => setDrawerOpen(false)}
    />
  );
}

interface CanvasStatusProps {
  status: ShellStatus;
  scope: ScopeDescriptor | null;
  error: ExplorerError | null;
  onRetry: () => void;
}

/** Canvas states that precede a loaded graph; no view can invent facts without it. */
function CanvasStatus({ status, scope, error, onRetry }: CanvasStatusProps) {
  if (status === 'loading') {
    return (
      <LoadingState
        label={scope ? `Loading ${scope.label}` : 'Loading workspace'}
        detail="Reading the local manifest from the fnscope server."
      />
    );
  }

  if (status === 'error') {
    return (
      <ErrorState
        title="The manifest could not be loaded"
        message={error?.message}
        detail={error?.detail}
        onRetry={onRetry}
      />
    );
  }

  return (
    <EmptyState
      icon={<Inbox className="h-4 w-4" aria-hidden="true" />}
      title={status === 'empty' ? 'This scope is empty' : 'No scope selected'}
      description={
        status === 'empty'
          ? 'No function records exist, so there is nothing to show here. Scan a directory with fnscope before opening it.'
          : 'The workspace did not return an available scope.'
      }
    />
  );
}

/** App owns providers and the workspace state; main.tsx only mounts it. */
export default function App() {
  return (
    <ThemeProvider>
      <ToastProvider>
        <WorkspaceApp />
      </ToastProvider>
    </ThemeProvider>
  );
}
