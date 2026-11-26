import React, { useEffect, useRef } from 'react';
import { setInert } from '../lib/focus';
import { useMediaQuery } from '../state/useMediaQuery';
import { useFocusTrap } from '../state/useFocusTrap';

interface AppShellProps {
  topBar: React.ReactNode;
  sidebar: React.ReactNode;
  canvas: React.ReactNode;
  drawer: React.ReactNode;
  provenance: React.ReactNode;
  sidebarOpen: boolean;
  drawerOpen: boolean;
  onCloseSidebar: () => void;
  onCloseDrawer: () => void;
}

/**
 * Layout and focus boundaries only: 270px sidebar, canvas, 320px drawer,
 * provenance footer. Below 1180px the drawer is modal; below 980px the
 * sidebar is modal. Closed panels stay mounted but inert.
 */
export default function AppShell({
  topBar,
  sidebar,
  canvas,
  drawer,
  provenance,
  sidebarOpen,
  drawerOpen,
  onCloseSidebar,
  onCloseDrawer,
}: AppShellProps) {
  const overlayDrawer = useMediaQuery('(max-width: 1179px)');
  const overlaySidebar = useMediaQuery('(max-width: 979px)');
  const sidebarRef = useRef<HTMLElement>(null);
  const drawerRef = useRef<HTMLElement>(null);
  const mainRef = useRef<HTMLElement>(null);

  useFocusTrap(drawerRef, drawerOpen && overlayDrawer, onCloseDrawer, {
    initialFocus: true,
    restoreFocus: true,
  });

  useEffect(() => {
    const drawerModal = drawerOpen && overlayDrawer;
    const sidebarModal = sidebarOpen && overlaySidebar;
    setInert(mainRef.current, drawerModal || sidebarModal);
    setInert(sidebarRef.current, !sidebarOpen || drawerModal);
    setInert(drawerRef.current, !drawerOpen);
  }, [drawerOpen, overlayDrawer, sidebarOpen, overlaySidebar]);

  return (
    <div className="ws-shell">
      <a href="#fnscope-canvas" className="ws-skip-link">
        Skip to canvas
      </a>
      {topBar}

      <div className="ws-body">
        {overlaySidebar && sidebarOpen ? (
          <button
            type="button"
            className="ws-backdrop ws-backdrop-sidebar"
            aria-label="Close explorer"
            onClick={onCloseSidebar}
          />
        ) : null}

        <aside
          id="fnscope-sidebar"
          ref={sidebarRef}
          aria-label="Explorer"
          className="ws-sidebar"
          data-open={sidebarOpen ? 'true' : 'false'}
          aria-hidden={!sidebarOpen}
        >
          <div className="ws-sidebar-inner">{sidebar}</div>
        </aside>

        <main id="fnscope-canvas" ref={mainRef} tabIndex={-1} className="ws-canvas">
          {canvas}
        </main>

        {overlayDrawer && drawerOpen ? (
          <button
            type="button"
            className="ws-backdrop ws-backdrop-drawer"
            aria-label="Close invoke sandbox"
            onClick={onCloseDrawer}
          />
        ) : null}

        <aside
          id="fnscope-drawer"
          ref={drawerRef}
          aria-label="Invoke sandbox"
          className="ws-drawer"
          data-open={drawerOpen ? 'true' : 'false'}
          aria-hidden={!drawerOpen}
        >
          <div className="ws-drawer-inner">{drawer}</div>
        </aside>
      </div>

      {provenance}
    </div>
  );
}
