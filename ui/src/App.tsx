import React, { useCallback, useEffect, useRef, useState } from 'react';
import AppShell from './components/AppShell';
import ProvenanceStrip from './components/ProvenanceStrip';
import { ThemeProvider } from './components/ThemeProvider';
import { ToastProvider, useToast } from './components/Toast';
import TopBar from './components/TopBar';
import { CanvasPlaceholder, ExplorerPlaceholder, InvokePlaceholder } from './components/Placeholders';
import type { LoadError } from './components/Placeholders';
import { fetchScopeGraph, fetchScopeList } from './api';
import { viewForShortcut } from './lib/shortcuts';
import { isTypingTarget } from './lib/shortcuts';
import type { FnGraph, ScopeList, ShellStatus, ViewId } from './lib/types';
import { pickInitialScopeId, rememberScopeId } from './state/scope';
import { useMediaQuery } from './state/useMediaQuery';

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

const EMPTY_TRAIL: { id: string; label: string }[] = [];

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
  const [sidebarOpen, setSidebarOpen] = useState(() =>
    typeof window === 'undefined' ? true : window.innerWidth >= 980
  );
  const [drawerOpen, setDrawerOpen] = useState(() =>
    typeof window === 'undefined' ? false : window.innerWidth >= 1280
  );
  const [focusSearchRequested, setFocusSearchRequested] = useState(false);
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
  const handleBack = useCallback(() => {
    // No trail exists until the explorer lands; Back stays disabled at the root.
  }, []);
  const handleRun = useCallback(() => {
    push('Select a function once the explorer lands, then run it here.', 'info');
  }, [push]);

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
        if (!drawerOpen) setDrawerOpen(true);
        handleRun();
        return;
      }

      if (event.altKey && event.key === 'ArrowLeft' && !typing) {
        event.preventDefault();
        handleBack();
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
  }, [drawerOpen, sidebarOpen, narrowSidebar, focusSearch, handleRun, handleBack]);

  const selectScope = useCallback(
    (nextId: string) => {
      if (boot.status !== 'ready') return;
      const next = boot.list.scopes.find((scope) => scope.id === nextId);
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
      push(`Switched to ${next.label}`, 'info');
    },
    [boot, scopeId, push]
  );

  const scopes = boot.status === 'ready' ? boot.list.scopes : [];
  const scope = scopes.find((entry) => entry.id === scopeId) || null;
  const graph =
    graphLoad.status === 'ready' && graphLoad.scopeId === scopeId ? graphLoad.graph : null;
  const graphError =
    graphLoad.status === 'error' && graphLoad.scopeId === scopeId ? graphLoad : null;
  const compatibility = boot.status === 'ready' ? boot.compatibility : false;

  let status: ShellStatus = 'loading';
  if (boot.status === 'error') status = 'error';
  else if (boot.status === 'ready' && graphError) status = 'error';
  else if (graph) status = graph.functions.length > 0 ? 'ready' : 'empty';

  const error: LoadError | null =
    boot.status === 'error'
      ? { message: boot.message, detail: boot.detail }
      : graphError
        ? { message: graphError.message, detail: graphError.detail }
        : null;

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
          trail={EMPTY_TRAIL}
          onSelectTrail={() => undefined}
          onBack={handleBack}
          backDisabled
          sidebarOpen={sidebarOpen}
          onToggleSidebar={() => setSidebarOpen((open) => !open)}
          drawerOpen={drawerOpen}
          onToggleDrawer={() => setDrawerOpen((open) => !open)}
        />
      }
      sidebar={
        <ExplorerPlaceholder
          status={status}
          scope={scope}
          graph={graph}
          error={error}
          onRetry={requestReload}
          searchValue={search}
          onSearchChange={setSearch}
          searchRef={searchRef}
        />
      }
      canvas={
        <CanvasPlaceholder
          view={view}
          status={status}
          scope={scope}
          graph={graph}
          error={error}
          onRetry={requestReload}
        />
      }
      drawer={
        <InvokePlaceholder
          scope={scope}
          compatibility={compatibility}
          status={status}
          onClose={() => setDrawerOpen(false)}
        />
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
