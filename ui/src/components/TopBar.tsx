import React from 'react';
import { ArrowLeft, Moon, PanelLeft, PanelRight, Sun } from 'lucide-react';
import BreadcrumbBar from './BreadcrumbBar';
import type { Crumb } from './BreadcrumbBar';
import IconButton from './IconButton';
import ScopeSwitcher from './ScopeSwitcher';
import ViewSwitcher from './ViewSwitcher';
import { useTheme } from './ThemeProvider';
import type { ScopeDescriptor, ViewId } from '../lib/types';

interface TopBarProps {
  scopes: ScopeDescriptor[];
  activeScope: ScopeDescriptor | null;
  onSelectScope: (scopeId: string) => void;
  scopeMenuDisabled: boolean;
  view: ViewId;
  onChangeView: (view: ViewId) => void;
  trail: Crumb[];
  onSelectTrail: (index: number) => void;
  onBack: () => void;
  backDisabled: boolean;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  drawerOpen: boolean;
  onToggleDrawer: () => void;
}

/** 52px workspace header: brand, scopes, call path, views, palette, drawer. */
export default function TopBar({
  scopes,
  activeScope,
  onSelectScope,
  scopeMenuDisabled,
  view,
  onChangeView,
  trail,
  onSelectTrail,
  onBack,
  backDisabled,
  sidebarOpen,
  onToggleSidebar,
  drawerOpen,
  onToggleDrawer,
}: TopBarProps) {
  const { theme, toggleTheme } = useTheme();
  const nextTheme = theme === 'paper' ? 'Graphite' : 'Paper';

  return (
    <header className="ws-topbar" role="banner">
      <IconButton
        label="Toggle explorer"
        aria-pressed={sidebarOpen}
        aria-controls="fnscope-sidebar"
        onClick={onToggleSidebar}
        title="Toggle explorer"
      >
        <PanelLeft className="h-4 w-4" aria-hidden="true" />
      </IconButton>

      <div className="flex shrink-0 items-center gap-2 pl-0.5">
        <div className="grid h-7 w-7 place-items-center rounded-sm bg-accent" aria-hidden="true">
          <span className="-mt-px font-mono text-[12px] font-semibold tracking-tighter text-accent-ink">
            {'{ }'}
          </span>
        </div>
        <span className="ws-brand-text font-serif text-[18px] font-semibold tracking-[-0.01em] text-ink">fnscope</span>
        <span className="hidden font-mono text-[11px] uppercase tracking-[0.14em] text-faint sm:inline">
          / workspace
        </span>
      </div>

      <span className="mx-1 hidden h-5 w-px bg-line sm:block" />

      <ScopeSwitcher
        scopes={scopes}
        activeScope={activeScope}
        onSelect={onSelectScope}
        disabled={scopeMenuDisabled}
      />

      <div className="mx-1 hidden h-5 w-px bg-line md:block" />

      <div className="hidden min-w-0 flex-1 items-center gap-1 md:flex">
        <IconButton label="Back in call path" size="sm" onClick={onBack} disabled={backDisabled}>
          <ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />
        </IconButton>
        <BreadcrumbBar trail={trail} onSelect={onSelectTrail} />
      </div>

      <div className="flex-1 md:hidden" />

      <ViewSwitcher view={view} onChange={onChangeView} />

      <IconButton
        label={`Switch to ${nextTheme} palette`}
        aria-pressed={theme === 'graphite'}
        onClick={toggleTheme}
        title={`Switch to ${nextTheme}`}
      >
        {theme === 'paper' ? (
          <Moon className="h-4 w-4" aria-hidden="true" />
        ) : (
          <Sun className="h-4 w-4" aria-hidden="true" />
        )}
      </IconButton>

      <IconButton
        label="Toggle invoke sandbox"
        aria-expanded={drawerOpen}
        aria-controls="fnscope-drawer"
        active={drawerOpen}
        onClick={onToggleDrawer}
        title="Toggle invoke sandbox"
      >
        <PanelRight className="h-4 w-4" aria-hidden="true" />
      </IconButton>
    </header>
  );
}
