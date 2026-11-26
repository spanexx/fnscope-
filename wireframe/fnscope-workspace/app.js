(() => {
  'use strict';

  const DATA = window.FNSCOPE_DATA;
  const $ = (id) => document.getElementById(id);

  if (!DATA || !Array.isArray(DATA.projects) || DATA.projects.length === 0) {
    document.body.innerHTML = '<div style="padding:32px;font-family:monospace;color:rgb(var(--err))">data.js did not load — open this folder through a local server.</div>';
    return;
  }

  const esc = (value) => String(value ?? '').replace(/[&<>"']/g, (ch) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  }[ch]));
  const fmt = (n) => Number(n).toLocaleString('en-US');

  const LANG_BY_EXT = {
    '.rs': 'Rust', '.ts': 'TypeScript', '.tsx': 'TSX', '.js': 'JavaScript', '.jsx': 'JSX',
    '.py': 'Python', '.go': 'Go', '.java': 'Java', '.c': 'C', '.h': 'C', '.cpp': 'C++',
    '.cc': 'C++', '.hpp': 'C++', '.kt': 'Kotlin', '.cs': 'C#', '.rb': 'Ruby', '.php': 'PHP',
    '.swift': 'Swift',
  };

  // Language is metadata, not state. One neutral badge keeps the palette quiet.
  const LANG_TONE = 'border-line bg-raised text-muted';

  const ENTRY_NAMES = new Set([
    'main', 'invoke', 'router', 'health', 'fn_graph', 'index', 'app_js', 'app_css',
    'read_manifest', 'functionexplorer', 'app', 'run',
    'evaluategrammarrole', 'gradesentenceexercise', 'dispatchwebhookevent',
  ]);

  const KIND = {
    call: { label: 'CALL', text: 'text-muted', bg: 'bg-raised', border: 'border-line', dot: 'bg-faint' },
    transform: { label: 'TRANSFORM', text: 'text-muted', bg: 'bg-raised', border: 'border-line', dot: 'bg-faint' },
    branch: { label: 'BRANCH', text: 'text-warn', bg: 'bg-warn/[0.08]', border: 'border-warn/25', dot: 'bg-warn' },
    return: { label: 'RETURN', text: 'text-ok', bg: 'bg-ok/[0.08]', border: 'border-ok/25', dot: 'bg-ok' },
    error: { label: 'ERROR', text: 'text-err', bg: 'bg-err/[0.08]', border: 'border-err/25', dot: 'bg-err' },
  };

  const THEME_KEY = 'fnscope-workspace-theme';

  function initialTheme() {
    try {
      const saved = window.localStorage.getItem(THEME_KEY);
      if (saved === 'paper' || saved === 'graphite') return saved;
    } catch {
      // Storage can be blocked; the prototype still works with the default.
    }
    return 'paper';
  }

  const state = {
    theme: initialTheme(),
    projectId: DATA.projects[0].id,
    selectedId: DATA.projects[0].defaultId,
    trail: [DATA.projects[0].defaultId],
    mode: 'entries',
    search: '',
    view: 'story',
    drawerOpen: (window.innerWidth || 1280) >= 1280,
    drawerTab: 'simulate',
    sidebarOpen: true,
    projectMenuOpen: false,
    expandedGroups: new Set(),
    expandedFiles: new Set(),
    stepLimit: 6,
    run: null,
    runError: '',
    running: false,
    argsDraft: null,
    bannerDismissed: false,
    toastSeq: 0,
  };

  let project = DATA.projects[0];
  let fnIndex = new Map();

  function activateProject(id) {
    const next = DATA.projects.find((p) => p.id === id);
    if (!next) return;
    project = next;
    state.projectId = id;
    state.selectedId = next.defaultId;
    state.trail = [next.defaultId];
    state.search = '';
    state.stepLimit = 6;
    state.run = null;
    state.runError = '';
    state.running = false;
    state.argsDraft = null;
    state.expandedGroups = new Set();
    state.expandedFiles = new Set();
    fnIndex = new Map(next.functions.map((f) => [f.id, f]));
    ensureSelectedExpanded();
    const search = $('search');
    if (search) search.value = '';
    syncSearchClear();
    renderAll();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function currentFn() {
    return fnIndex.get(state.selectedId) || project.functions[0];
  }

  function groupKey(filePath) {
    if (filePath.startsWith('crates/')) return filePath.split('/').slice(0, 2).join('/');
    if (filePath.startsWith('tests/fixtures/')) return filePath.split('/').slice(0, 3).join('/');
    if (filePath.startsWith('ui/')) return 'ui';
    return filePath.split('/')[0];
  }

  function fileLabel(filePath, group) {
    if (filePath.startsWith(group + '/')) return filePath.slice(group.length + 1);
    return filePath.split('/').pop();
  }

  function shortModule(module) {
    return String(module || '')
      .replace(/^crates\//, '')
      .replace(/^tests\/fixtures\//, 'fixtures/')
      .replace(/\/src$/, '')
      .replace(/\/src\/nlp$/, '/nlp')
      .replace(/\/src\/utils$/, '/utils')
      || module;
  }

  function langOf(filePath) {
    const idx = String(filePath).lastIndexOf('.');
    return LANG_BY_EXT[idx === -1 ? '' : String(filePath).slice(idx)] || 'Unknown';
  }

  function langTone() {
    return LANG_TONE;
  }

  function paramLabel(p) {
    const match = String(p.name || '').match(/\(([^)]+)\)/);
    return match ? match[1] : p.name;
  }

  function signature(fn) {
    const params = (fn.parameters || []).map((p) => `${paramLabel(p)}: ${p.type}`).join(', ');
    const ret = fn.returns && fn.returns.type ? `: ${fn.returns.type}` : '';
    return `${fn.name}(${params})${ret}`;
  }

  function sampleForType(type) {
    const t = String(type || '');
    if (/bool/i.test(t)) return false;
    if (/String|\bstr\b/i.test(t)) return 'example';
    if (/usize|u64|u32|u16|i32|i64|f64|f32|Int|number|Duration/i.test(t)) return 0;
    if (/Vec|\[|Array|BTreeSet|HashSet/i.test(t)) return [];
    if (/Option/i.test(t)) return null;
    if (/Path|PathBuf/i.test(t)) return '/tmp/fnscope';
    if (/Json|Request/i.test(t)) return { fnId: 'invoke', args: {} };
    if (/State/i.test(t)) return '<server state>';
    if (/Registry/i.test(t)) return '<registry>';
    return '…';
  }

  function argsTemplate(fn) {
    const out = {};
    for (const p of fn.parameters || []) {
      out[paramLabel(p)] = p.defaultValue === null || p.defaultValue === undefined
        ? sampleForType(p.type)
        : p.defaultValue;
    }
    return JSON.stringify(out, null, 2);
  }

  function formatDate(iso) {
    const date = new Date(iso);
    if (Number.isNaN(date.getTime())) return 'unknown';
    return date.toLocaleString(undefined, { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' });
  }

  function highlight(text, query) {
    const raw = String(text ?? '');
    if (!query) return esc(raw);
    const idx = raw.toLowerCase().indexOf(query);
    if (idx === -1) return esc(raw);
    return esc(raw.slice(0, idx)) + '<mark>' + esc(raw.slice(idx, idx + query.length)) + '</mark>' + esc(raw.slice(idx + query.length));
  }

  function badge(label, classes, icon) {
    return `<span class="inline-flex items-center gap-1 rounded border px-1.5 py-0.5 text-[10px] ${classes}">${icon ? `<i data-lucide="${icon}" class="h-2.5 w-2.5"></i>` : ''}${esc(label)}</span>`;
  }

  function refreshIcons() {
    if (window.lucide && typeof window.lucide.createIcons === 'function') {
      window.lucide.createIcons();
    }
  }

  function applyTheme() {
    document.documentElement.dataset.theme = state.theme;
    const button = $('theme-toggle');
    if (!button) return;
    const next = state.theme === 'paper' ? 'Graphite' : 'Paper';
    button.title = `Switch to ${next}`;
    button.setAttribute('aria-label', `Switch to ${next} palette`);
    button.innerHTML = `<i data-lucide="${state.theme === 'paper' ? 'moon' : 'sun'}" class="h-4 w-4"></i>`;
    refreshIcons();
  }

  function internalCallers(fn) {
    return (fn.calledBy || []).filter((caller) => caller.fnId);
  }

  function isEntryPoint(fn) {
    return internalCallers(fn).length === 0;
  }

  function ensureSelectedExpanded() {
    const fn = currentFn();
    if (!fn) return;
    state.expandedGroups.add(groupKey(fn.filePath));
    state.expandedFiles.add(fn.filePath);
  }

  function revealSelectedInSidebar() {
    window.requestAnimationFrame(() => {
      const id = window.CSS && CSS.escape ? CSS.escape(state.selectedId) : state.selectedId;
      const row = document.querySelector(`#sidebar-list [data-fn="${id}"]`);
      if (row) row.scrollIntoView({ block: 'nearest' });
    });
  }

  function resetRunState() {
    state.run = null;
    state.runError = '';
    state.running = false;
    state.argsDraft = null;
    state.stepLimit = 6;
  }

  function isTypingTarget(target) {
    if (!target) return false;
    const tag = target.tagName;
    return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || target.isContentEditable;
  }

  function setView(view) {
    state.view = view;
    renderTopbar();
    renderCanvas();
    refreshIcons();
    $('main').scrollTop = 0;
  }

  // ---------------------------------------------------------------- top bar

  function renderTopbar() {
    $('project-name').textContent = project.label;
    const langCount = project.languages.length;
    $('project-stats').textContent = `${fmt(project.functionCount)} fns · ${langCount} lang${langCount === 1 ? '' : 's'}`;
    $('project-button').setAttribute('aria-expanded', String(state.projectMenuOpen));

    const views = [
      { id: 'story', label: 'Story', icon: 'book-open' },
      { id: 'map', label: '1-Hop', icon: 'share-2' },
      { id: 'split', label: 'Split', icon: 'columns-2' },
      { id: 'raw', label: 'Raw', icon: 'braces' },
    ];
    $('view-switcher').innerHTML = views.map((v) => `
      <button data-action="view" data-view="${v.id}" title="${v.label} view" aria-pressed="${state.view === v.id}"
        class="flex items-center gap-1.5 rounded px-2 py-1 text-[11px] transition-colors ${state.view === v.id ? 'bg-active text-ink shadow-sm' : 'text-muted hover:text-ink'}">
        <i data-lucide="${v.icon}" class="h-3 w-3"></i>
        <span class="hidden md:inline">${v.label}</span>
      </button>`).join('');

    const trail = state.trail.map((id) => fnIndex.get(id)).filter(Boolean);
    $('breadcrumb').innerHTML = trail.map((f, i) => {
      const last = i === trail.length - 1;
      const sep = i > 0 ? '<i data-lucide="chevron-right" class="h-3 w-3 shrink-0 text-faint"></i>' : '';
      return `${sep}<button data-action="trail" data-index="${i}" title="${esc(f.module)}" class="max-w-[150px] truncate rounded border px-1.5 py-1 font-mono text-[11px] transition-colors ${last ? 'border-accent/25 bg-accent/10 text-accent' : 'border-transparent text-muted hover:bg-wash hover:text-ink'}">${esc(f.name)}()</button>`;
    }).join('');
    $('trail-back').disabled = trail.length <= 1;

    const drawerToggle = $('drawer-toggle');
    drawerToggle.classList.toggle('bg-accent/15', state.drawerOpen);
    drawerToggle.classList.toggle('text-accent', state.drawerOpen);
    drawerToggle.classList.toggle('text-muted', !state.drawerOpen);
    drawerToggle.classList.toggle('border-accent/30', state.drawerOpen);

    const sidebarToggle = $('sidebar-toggle');
    sidebarToggle.classList.toggle('bg-raised', state.sidebarOpen);
    sidebarToggle.classList.toggle('text-ink', state.sidebarOpen);
    sidebarToggle.classList.toggle('text-muted', !state.sidebarOpen);
  }

  function renderProjectMenu() {
    const menu = $('project-menu');
    menu.classList.toggle('hidden', !state.projectMenuOpen);
    $('project-button').setAttribute('aria-expanded', String(state.projectMenuOpen));
    if (!state.projectMenuOpen) return;

    menu.innerHTML = `
      <div class="p-2">
        <div class="px-2 pb-2 pt-1 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">Scanned scopes</div>
        ${DATA.projects.map(projectCard).join('')}
        <div class="mt-2 flex items-start gap-2 rounded-md border border-line bg-bg px-2.5 py-2 text-[10px] leading-4 text-muted">
          <i data-lucide="shield" class="mt-0.5 h-3 w-3 shrink-0 text-ok"></i>
          <span>Each scope owns its graph. Switching swaps the sidebar, breadcrumb, canvas and sandbox — no cross-repo edges.</span>
        </div>
        <button type="button" aria-disabled="true" class="mt-2 flex w-full cursor-not-allowed items-center justify-center gap-1.5 rounded-md border border-dashed border-line-strong px-2 py-2 text-[11px] text-faint">
          <i data-lucide="plus" class="h-3.5 w-3.5"></i> Scan another repository
        </button>
      </div>`;
  }

  function projectCard(p) {
    const active = p.id === state.projectId;
    const langs = p.languages.slice(0, 4).map((l) =>
      `<span class="rounded border border-line-strong bg-bg px-1 py-0.5 text-[9px] text-muted">${esc(l.name)} <span class="text-faint">${l.count}</span></span>`).join('');
    const more = p.languages.length > 4
      ? `<span class="rounded border border-line-strong px-1 py-0.5 text-[9px] text-faint">+${p.languages.length - 4}</span>`
      : '';
    return `
      <button data-action="switch-project" data-project="${esc(p.id)}" class="mb-1 w-full rounded-lg border p-2.5 text-left transition-colors ${active ? 'border-accent/40 bg-accent/[0.07]' : 'border-line bg-panel hover:border-line-strong'}">
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <div class="flex items-center gap-1.5">
              <span class="truncate text-[12px] font-medium text-ink">${esc(p.label)}</span>
              ${active ? '<i data-lucide="check" class="h-3 w-3 shrink-0 text-accent"></i>' : ''}
            </div>
            <div class="mt-0.5 truncate text-[10px] text-muted">${esc(p.note)}</div>
            ${p.id === 'fnscope' ? '<div class="mt-1 inline-flex items-center gap-1 rounded border border-warn/20 bg-warn/[0.06] px-1 py-0.5 text-[9px] text-warn/80"><i data-lucide="flask-conical" class="h-2.5 w-2.5"></i>tests/fixtures included</div>' : ''}
          </div>
          <div class="shrink-0 text-right font-mono text-[10px] leading-4 text-muted">${fmt(p.functionCount)} fns<br>${p.fileCount} files</div>
        </div>
        <div class="mt-2 flex flex-wrap gap-1">${langs}${more}</div>
        <div class="mt-2 flex items-center gap-1 text-[9.5px] text-faint">
          <i data-lucide="clock" class="h-2.5 w-2.5"></i>${formatDate(p.generatedAt)} · root ${esc(p.rootDir)}
        </div>
      </button>`;
  }

  // ---------------------------------------------------------------- sidebar

  function renderSidebar() {
    const sidebar = $('sidebar');
    sidebar.style.width = state.sidebarOpen ? '270px' : '0px';
    sidebar.style.borderRightWidth = state.sidebarOpen ? '1px' : '0px';
    sidebar.style.opacity = state.sidebarOpen ? '1' : '0';
    sidebar.classList.toggle('pointer-events-none', !state.sidebarOpen);

    $('sidebar-scope').textContent = project.rootDir;
    $('sidebar-summary').innerHTML = `<span class="font-mono text-muted">${fmt(project.functionCount)}</span> functions · <span class="font-mono text-muted">${project.fileCount}</span> files`;
    const origin = project.id === 'sample-nlp-app'
      ? 'Fixture app'
      : project.id === 'fnscope-crates' ? 'Self-scan · crates' : 'Self-scan · full tree';
    $('sidebar-footer').innerHTML = `${origin}<br><span class="text-muted">${formatDate(project.generatedAt)}</span>`;

    $('mode-switcher').innerHTML = [
      { id: 'entries', label: 'Entry points', icon: 'log-in' },
      { id: 'modules', label: 'Module tree', icon: 'folder-tree' },
    ].map((m) => `
      <button data-action="sidebar-mode" data-mode="${m.id}" aria-pressed="${state.mode === m.id}"
        class="flex items-center justify-center gap-1 rounded px-1.5 py-1 text-[10px] transition-colors ${state.mode === m.id ? 'bg-active text-ink shadow-sm' : 'text-muted hover:text-ink'}">
        <i data-lucide="${m.icon}" class="h-3 w-3"></i>${m.label}
      </button>`).join('');

    renderSidebarList();
  }

  function renderSidebarList() {
    const list = $('sidebar-list');
    const query = state.search.trim().toLowerCase();
    if (query) {
      list.innerHTML = searchResults(query);
      return;
    }
    list.innerHTML = state.mode === 'entries' ? entryList() : moduleTree();
  }

  function entryList() {
    const zeroCallers = project.functions.filter(isEntryPoint);
    const curated = zeroCallers.filter((f) => ENTRY_NAMES.has(f.name.toLowerCase()));
    // 803 zero-caller functions is not a useful entry list. Prefer roots,
    // handlers and top-level components; fall back to the largest orphans.
    const entries = (curated.length >= 4 ? curated : zeroCallers.slice(0, 12))
      .sort((a, b) => b.stepCount - a.stepCount || a.name.localeCompare(b.name));
    const hidden = zeroCallers.length - entries.length;
    return `
      <div class="flex items-center justify-between px-3 pb-1 pt-2">
        <span class="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted">Entry points</span>
        <span class="font-mono text-[9.5px] text-faint">${entries.length}</span>
      </div>
      <p class="px-3 pb-2 text-[10px] leading-4 text-faint">Roots, handlers and top-level components</p>
      ${entries.map((f) => fnRow(f, { entry: true })).join('')}
      ${hidden > 0 ? `<p class="px-3 pb-3 pt-1 text-[9.5px] leading-4 text-faint">${fmt(hidden)} other zero-caller functions are searchable but not treated as entry points.</p>` : ''}
      ${entries.length === 0 ? emptyState('log-in', 'No entry points', 'Every function in this scope has a resolved caller.') : ''}`;
  }

  function moduleTree() {
    const groups = new Map();
    for (const fn of project.functions) {
      const group = groupKey(fn.filePath);
      if (!groups.has(group)) groups.set(group, new Map());
      const files = groups.get(group);
      if (!files.has(fn.filePath)) files.set(fn.filePath, []);
      files.get(fn.filePath).push(fn);
    }

    return [...groups.keys()].sort().map((group) => {
      const files = groups.get(group);
      const count = [...files.values()].reduce((n, arr) => n + arr.length, 0);
      const open = state.expandedGroups.has(group);
      const fileRows = open ? [...files.keys()].sort().map((file) => {
        const fns = files.get(file).sort((a, b) => a.line - b.line);
        const fileOpen = state.expandedFiles.has(file);
        return `
          <div>
            <button data-action="toggle-file" data-file="${esc(file)}" aria-expanded="${fileOpen}"
              class="flex w-full items-center gap-1.5 py-1.5 pl-7 pr-3 text-left hover:bg-wash">
              <i data-lucide="${fileOpen ? 'chevron-down' : 'chevron-right'}" class="h-2.5 w-2.5 shrink-0 text-faint"></i>
              <i data-lucide="file-code" class="h-3 w-3 shrink-0 text-info/60"></i>
              <span class="min-w-0 flex-1 truncate font-mono text-[10.5px] text-muted">${esc(fileLabel(file, group))}</span>
              <span class="font-mono text-[9px] text-faint">${fns.length}</span>
            </button>
            ${fileOpen ? fns.map((f) => fnRow(f, { indent: true })).join('') : ''}
          </div>`;
      }).join('') : '';

      return `
        <div>
          <button data-action="toggle-group" data-group="${esc(group)}" aria-expanded="${open}"
            class="flex w-full items-center gap-1.5 px-2.5 py-2 text-left hover:bg-wash">
            <i data-lucide="${open ? 'chevron-down' : 'chevron-right'}" class="h-3 w-3 shrink-0 text-muted"></i>
            <i data-lucide="folder" class="h-3.5 w-3.5 shrink-0 text-warn/70"></i>
            <span class="min-w-0 flex-1 truncate font-mono text-[11px] text-ink">${esc(group)}</span>
            <span class="rounded bg-raised px-1.5 py-0.5 font-mono text-[9px] text-muted">${count}</span>
          </button>
          ${fileRows}
        </div>`;
    }).join('');
  }

  function fnRow(fn, opts = {}) {
    const active = fn.id === state.selectedId;
    const indent = opts.indent ? 'pl-10 pr-3' : 'px-3';
    const subtitle = opts.indent
      ? `line ${fn.line} · ${fn.stepCount} steps`
      : `${shortModule(fn.module)} · ${fn.filePath.split('/').pop()} · ${fn.stepCount} steps`;
    return `
      <button data-action="select-fn" data-fn="${esc(fn.id)}"
        class="group flex w-full items-start gap-2 border-l-2 ${indent} py-2 text-left transition-colors ${active ? 'border-accent bg-accent/[0.09]' : 'border-transparent hover:bg-wash'}">
        <span class="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full ${opts.entry ? 'bg-accent' : 'bg-faint'}"></span>
        <span class="min-w-0 flex-1">
          <span class="block truncate font-mono text-[12px] ${active ? 'text-accent' : 'text-ink'}">${highlight(fn.name, state.search.trim().toLowerCase())}<span class="text-faint">()</span></span>
          <span class="mt-0.5 block truncate text-[10px] text-muted">${esc(subtitle)}</span>
        </span>
        <span class="mt-1 flex shrink-0 items-center gap-1">
          ${fn.errors && fn.errors.length ? '<span class="h-1.5 w-1.5 rounded-full bg-err/90" title="Declared error paths"></span>' : ''}
          ${opts.entry ? '<i data-lucide="corner-down-right" class="h-3 w-3 text-accent/70"></i>' : ''}
        </span>
      </button>`;
  }

  function searchResults(query) {
    const all = project.functions.filter((fn) =>
      [fn.name, fn.id, fn.module, fn.filePath].some((value) => String(value).toLowerCase().includes(query)));
    all.sort((a, b) => {
      const rank = (fn) => fn.name.toLowerCase() === query ? 0 : fn.name.toLowerCase().startsWith(query) ? 1 : 2;
      return rank(a) - rank(b) || b.stepCount - a.stepCount || a.name.localeCompare(b.name);
    });
    const shown = all.slice(0, 60);
    return `
      <div class="flex items-center justify-between px-3 pb-1 pt-2">
        <span class="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted">Search</span>
        <span class="font-mono text-[9.5px] text-faint">${all.length} match${all.length === 1 ? '' : 'es'}</span>
      </div>
      ${shown.map((f) => fnRow(f, {})).join('')}
      ${all.length > shown.length ? `<p class="px-3 py-2 text-[10px] text-faint">Showing 60 of ${all.length}. Narrow the filter to see more.</p>` : ''}
      ${all.length === 0 ? emptyState('search-x', 'No functions match', `Nothing contains “${query}” in this scope.`) : ''}`;
  }

  function emptyState(icon, title, detail) {
    return `
      <div class="flex flex-col items-center justify-center px-6 py-12 text-center">
        <div class="grid h-9 w-9 place-items-center rounded-lg border border-line bg-panel">
          <i data-lucide="${icon}" class="h-4 w-4 text-muted"></i>
        </div>
        <p class="mt-2 text-[11px] font-medium text-muted">${esc(title)}</p>
        <p class="mt-1 max-w-[220px] text-[10px] leading-4 text-faint">${esc(detail)}</p>
      </div>`;
  }

  function emptyInline(icon, title, detail) {
    return `
      <div class="flex items-start gap-2.5 rounded-lg border border-dashed border-line-strong px-3 py-2.5">
        <i data-lucide="${icon}" class="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted"></i>
        <div>
          <p class="text-[10.5px] font-medium text-muted">${esc(title)}</p>
          <p class="mt-0.5 text-[10px] leading-4 text-faint">${esc(detail)}</p>
        </div>
      </div>`;
  }

  // ---------------------------------------------------------------- canvas

  function renderCanvas() {
    const fn = currentFn();
    const canvas = $('canvas');
    if (!fn) {
      canvas.innerHTML = `<div class="p-8">${emptyState('circle-slash', 'No function selected', 'Pick a function from the explorer.')}</div>`;
      return;
    }
    if (state.view === 'map') canvas.innerHTML = mapView(fn);
    else if (state.view === 'split') canvas.innerHTML = splitView(fn);
    else if (state.view === 'raw') canvas.innerHTML = rawView(fn);
    else canvas.innerHTML = storyView(fn);
  }

  function prototypeBanner() {
    if (state.bannerDismissed) return '';
    return `
      <div class="mb-3 flex items-start gap-2.5 rounded-lg border border-line border-l-2 border-l-accent bg-panel px-3 py-2.5">
        <i data-lucide="flask-conical" class="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent"></i>
        <p class="text-[10.5px] leading-4 text-muted"><span class="font-medium text-ink">Design-validation prototype.</span> Real functions and counts from the fnscope self-scan. Stories for <span class="font-mono text-ink">invoke</span>, <span class="font-mono text-ink">scan_with_registry</span> and <span class="font-mono text-ink">router</span> are curated from their source; other steps come from the manifest. Toggle <span class="font-medium text-ink">Paper / Graphite</span> in the top bar.</p>
        <button data-action="dismiss-banner" aria-label="Dismiss prototype note" class="ml-auto shrink-0 rounded p-0.5 text-faint hover:text-ink"><i data-lucide="x" class="h-3 w-3"></i></button>
      </div>`;
  }

  function storyView(fn) {
    return `
      <div class="mx-auto max-w-[1020px] px-5 py-5 lg:px-7">
        ${prototypeBanner()}
        ${headerCard(fn)}
        <div class="mt-4 space-y-3">
          ${callersSection(fn)}
          ${inputsSection(fn)}
          ${storySection(fn)}
          ${outputsSection(fn)}
        </div>
      </div>`;
  }

  function headerCard(fn) {
    const lang = langOf(fn.filePath);
    const pure = fn.purity === 'pure';
    const isEntry = isEntryPoint(fn);
    return `
      <section class="overflow-hidden rounded-xl border border-line bg-panel">
        <div class="h-[3px] bg-accent"></div>
        <div class="p-4 lg:p-5">
          <div class="flex flex-wrap items-center gap-1.5">
            ${badge(shortModule(fn.module), 'border-line-strong bg-bg text-muted', 'folder')}
            ${badge(lang, langTone(lang), 'code-2')}
            ${badge(pure ? 'pure' : 'side-effect', pure ? 'border-ok/25 bg-ok/10 text-ok' : 'border-warn/25 bg-warn/10 text-warn', pure ? 'sparkles' : 'zap')}
            ${isEntry ? badge('entry point', 'border-accent/25 bg-accent/10 text-accent', 'log-in') : ''}
          </div>
          <h1 class="mt-3 break-words font-mono text-[17px] leading-7 text-ink lg:text-[19px]">${esc(signature(fn))}</h1>
          <p class="mt-2 max-w-[78ch] text-[12.5px] leading-6 text-muted">${esc(fn.description || 'No description in the manifest.')}</p>
          <div class="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1.5 font-mono text-[10.5px] text-muted">
            <span class="inline-flex items-center gap-1"><i data-lucide="file-code" class="h-3 w-3"></i>${esc(fn.filePath)}:${fn.line}</span>
            <span class="inline-flex items-center gap-1"><i data-lucide="hash" class="h-3 w-3"></i>${esc(fn.id)}</span>
            <span class="inline-flex items-center gap-1"><i data-lucide="list-ordered" class="h-3 w-3"></i>${fn.stepCount} steps</span>
            <span class="inline-flex items-center gap-1"><i data-lucide="corner-up-left" class="h-3 w-3"></i>${fn.calledBy.length} callers</span>
          </div>
          <div class="mt-4 flex flex-wrap gap-2">
            <button data-action="open-drawer" class="inline-flex items-center gap-1.5 rounded-md border border-accent bg-accent px-2.5 py-1.5 text-[11px] font-medium text-accent-ink transition-colors hover:bg-accent/90">
              <i data-lucide="play" class="h-3 w-3"></i> Invoke
            </button>
            <button data-action="copy-signature" class="inline-flex items-center gap-1.5 rounded-md border border-line-strong bg-panel px-2.5 py-1.5 text-[11px] text-muted transition-colors hover:border-line-strong hover:text-ink">
              <i data-lucide="copy" class="h-3 w-3"></i> Copy signature
            </button>
            <button data-action="toast-source" class="inline-flex items-center gap-1.5 rounded-md border border-line-strong bg-panel px-2.5 py-1.5 text-[11px] text-muted transition-colors hover:border-line-strong hover:text-ink">
              <i data-lucide="external-link" class="h-3 w-3"></i> Open source
            </button>
          </div>
        </div>
      </section>`;
  }

  function callersSection(fn) {
    const callers = fn.calledBy || [];
    return `
      <details open class="group rounded-xl border border-line bg-panel">
        <summary class="flex cursor-pointer select-none items-center gap-2 px-4 py-3">
          <i data-lucide="arrow-down-left" class="h-4 w-4 text-info"></i>
          <span class="text-[12px] font-medium text-ink">Triggered by</span>
          <span class="font-mono text-[10px] text-muted">${callers.length}</span>
          <i data-lucide="chevron-down" class="ml-auto h-3.5 w-3.5 text-muted transition-transform group-open:rotate-180"></i>
        </summary>
        <div class="space-y-2 px-4 pb-4">
          ${callers.length === 0
            ? emptyInline('log-in', 'No callers in this scope', 'This is where execution can begin.')
            : callers.map(callerCard).join('')}
        </div>
      </details>`;
  }

  function callerCard(caller) {
    const target = caller.fnId ? fnIndex.get(caller.fnId) : null;
    const jump = target
      ? `<button data-action="caller" data-fn="${esc(target.id)}" class="inline-flex shrink-0 items-center gap-1 rounded border border-info/25 bg-info/10 px-1.5 py-1 font-mono text-[9.5px] text-info transition-colors hover:bg-info/20">Jump <i data-lucide="arrow-up" class="h-2.5 w-2.5"></i></button>`
      : `<span class="shrink-0 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[9px] text-faint">${caller.fnId ? 'outside scope' : 'external'}</span>`;
    return `
      <div class="flex items-start gap-2.5 rounded-lg border border-line bg-surface p-2.5">
        <div class="mt-0.5 grid h-6 w-6 shrink-0 place-items-center rounded-md border border-info/20 bg-info/10">
          <i data-lucide="arrow-up-right" class="h-3 w-3 text-info"></i>
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-x-2 gap-y-0.5">
            <span class="font-mono text-[11.5px] text-ink">${esc(caller.name)}()</span>
            <span class="truncate font-mono text-[9.5px] text-muted">${esc(caller.module)}</span>
          </div>
          ${caller.callSite ? `<pre class="mt-1 overflow-hidden text-ellipsis whitespace-nowrap font-mono text-[10px] text-muted">${esc(caller.callSite)}</pre>` : ''}
        </div>
        ${jump}
      </div>`;
  }

  function inputsSection(fn) {
    const params = fn.parameters || [];
    return `
      <section class="rounded-xl border border-line bg-panel p-4">
        <div class="mb-3 flex items-center gap-2">
          <i data-lucide="arrow-right-left" class="h-4 w-4 text-muted"></i>
          <span class="text-[12px] font-medium text-ink">Inputs</span>
          <span class="font-mono text-[10px] text-muted">${params.length}</span>
        </div>
        ${params.length === 0
          ? emptyInline('circle-slash', 'No parameters', 'This function reads only shared state, or nothing at all.')
          : `<div class="grid gap-2 sm:grid-cols-2">${params.map(paramCard).join('')}</div>`}
      </section>`;
  }

  function paramCard(p) {
    const label = paramLabel(p);
    const hasDefault = p.defaultValue !== null && p.defaultValue !== undefined;
    return `
      <div class="rounded-lg border border-line bg-surface px-3 py-2">
        <div class="flex items-center justify-between gap-2">
          <span class="truncate font-mono text-[11.5px] text-ink">${esc(label)}</span>
          <span class="shrink-0 rounded border border-line-strong bg-raised px-1.5 py-0.5 font-mono text-[9px] text-info">${esc(p.type)}</span>
        </div>
        <div class="mt-1 flex items-center gap-2 text-[9.5px] text-muted">
          <span class="${p.required ? 'text-err/80' : 'text-muted'}">${p.required ? 'required' : 'optional'}</span>
          ${hasDefault ? `<span class="text-faint">·</span><span class="truncate font-mono">default ${esc(JSON.stringify(p.defaultValue))}</span>` : ''}
        </div>
      </div>`;
  }

  function storySection(fn) {
    const available = fn.steps.length;
    const total = fn.stepCount || available;
    const shown = fn.steps.slice(0, state.stepLimit);
    const canExpand = available > shown.length;
    const canCollapse = state.stepLimit > 6;
    return `
      <section class="rounded-xl border border-line bg-panel">
        <div class="flex items-center gap-2 border-b border-line/80 px-4 py-3">
          <i data-lucide="route" class="h-4 w-4 text-accent"></i>
          <span class="text-[12px] font-medium text-ink">Execution story</span>
          <span class="font-mono text-[10px] text-muted">${shown.length}/${total}</span>
          <span class="ml-auto hidden text-[10px] text-faint sm:inline">ordered by execution</span>
        </div>
        <div class="p-4">
          ${available === 0
            ? emptyInline('circle-slash', 'No steps extracted', 'The parser found no body facts for this function.')
            : `<ol class="relative">${shown.map((step, i) => stepRow(step, i, i === shown.length - 1)).join('')}</ol>`}
          ${canExpand ? `
            <button data-action="more-steps" class="mt-2 flex w-full items-center justify-center gap-1 rounded border border-dashed border-line-strong px-2 py-1.5 text-[10px] text-muted transition-colors hover:border-line-strong hover:text-ink">
              <i data-lucide="chevrons-down" class="h-3 w-3"></i> Show ${available - shown.length} more step${available - shown.length === 1 ? '' : 's'}
            </button>` : ''}
          ${canCollapse ? `
            <button data-action="fewer-steps" class="mt-2 flex w-full items-center justify-center gap-1 rounded border border-dashed border-line-strong px-2 py-1.5 text-[10px] text-muted transition-colors hover:border-line-strong hover:text-ink">
              <i data-lucide="chevrons-up" class="h-3 w-3"></i> Collapse to 6
            </button>` : ''}
          ${total > available ? `<p class="mt-2 text-[9.5px] leading-4 text-faint">Prototype embeds the first ${available} of ${total} extracted steps for this function.</p>` : ''}
        </div>
      </section>`;
  }

  function stepRow(step, index, last) {
    const meta = KIND[step.kind] || KIND.transform;
    const callee = step.callsFnId ? fnIndex.get(step.callsFnId) : null;
    const unresolved = step.callsFnId && !callee;
    return `
      <li class="step-spine relative pb-3 pl-8 last:pb-0">
        <span class="absolute left-0 top-0 grid h-[22px] w-[22px] place-items-center rounded-full border ${meta.border} ${meta.bg} font-mono text-[9px] ${meta.text}">${index + 1}</span>
        <div class="rounded-lg border border-line bg-surface p-3">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="rounded border ${meta.border} ${meta.bg} px-1 py-0.5 font-mono text-[8.5px] tracking-wide ${meta.text}">${meta.label}</span>
            <span class="text-[11.5px] leading-5 text-ink">${esc(step.title)}</span>
          </div>
          ${step.code ? `<pre class="code-block mt-2 overflow-x-auto rounded-md border border-line bg-bg p-2.5 font-mono text-[10px] leading-5 text-ink">${esc(step.code)}</pre>` : ''}
          <div class="mt-2 flex flex-wrap items-center gap-1 font-mono text-[9.5px]">
            ${step.inputs && step.inputs.length ? `<span class="text-faint">in</span>${step.inputs.map((x) => `<span class="rounded border border-line-strong bg-raised px-1.5 py-0.5 text-muted">${esc(x)}</span>`).join('')}` : ''}
            ${step.inputs && step.inputs.length && step.output ? '<i data-lucide="arrow-right" class="h-3 w-3 text-faint"></i>' : ''}
            ${step.output ? `<span class="rounded border border-accent/20 bg-accent/[0.07] px-1.5 py-0.5 text-accent/90">${esc(step.output)}</span>` : ''}
          </div>
          ${step.detail ? `<p class="mt-1.5 text-[10.5px] leading-4 text-muted">${esc(step.detail)}</p>` : ''}
          ${callee ? `<button data-action="callee" data-fn="${esc(callee.id)}" class="mt-2 inline-flex items-center gap-1 rounded border border-accent/25 bg-accent/[0.08] px-1.5 py-1 font-mono text-[10px] text-accent transition-colors hover:bg-accent/15">Inspect ${esc(callee.name)}() <i data-lucide="arrow-right" class="h-3 w-3"></i></button>` : ''}
          ${unresolved ? `<span class="mt-2 inline-flex items-center gap-1 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[9px] text-faint">unresolved: ${esc(step.callsFnId)}</span>` : ''}
        </div>
      </li>`;
  }

  function outputsSection(fn) {
    const ret = fn.returns || { type: 'unknown', fields: [] };
    const fields = ret.fields || [];
    const errors = fn.errors || [];
    return `
      <div class="grid gap-3 md:grid-cols-2">
        <section class="rounded-xl border border-line bg-panel p-4">
          <div class="mb-2 flex items-center gap-2">
            <i data-lucide="corner-down-right" class="h-4 w-4 text-ok"></i>
            <span class="text-[12px] font-medium text-ink">Returns</span>
          </div>
          <div class="rounded border border-ok/20 bg-ok/[0.06] px-2 py-1.5 font-mono text-[10.5px] text-ok">${esc(ret.type || 'unknown')}</div>
          ${fields.length ? `<div class="mt-2 space-y-1">${fields.map((f) => `<div class="flex items-center justify-between gap-2 rounded border border-line bg-surface px-2 py-1"><span class="font-mono text-[10px] text-ink">${esc(f.name)}</span><span class="font-mono text-[9px] text-info">${esc(f.type)}</span></div>`).join('')}</div>` : ''}
        </section>
        <section class="rounded-xl border border-line bg-panel p-4">
          <div class="mb-2 flex items-center gap-2">
            <i data-lucide="alert-triangle" class="h-4 w-4 text-err"></i>
            <span class="text-[12px] font-medium text-ink">Error paths</span>
          </div>
          ${errors.length
            ? errors.map((e) => `<div class="mt-1 rounded border border-err/20 bg-err/[0.04] px-2 py-1.5"><div class="flex items-center gap-2"><span class="font-mono text-[10px] text-err">${esc(e.code)}</span>${e.status ? `<span class="rounded border border-err/20 px-1 py-0.5 font-mono text-[8.5px] text-err/80">${e.status}</span>` : ''}</div><p class="mt-0.5 text-[9.5px] leading-4 text-muted">${esc(e.when)}</p></div>`).join('')
            : emptyInline('shield-check', 'No declared errors', 'The manifest records no typed error paths for this function.')}
        </section>
      </div>`;
  }

  // ---------------------------------------------------------------- 1-hop map

  function mapView(fn) {
    const callers = fn.calledBy || [];
    const calleeMap = new Map();
    for (const step of fn.steps) {
      if (step.callsFnId && !calleeMap.has(step.callsFnId)) calleeMap.set(step.callsFnId, step);
    }
    const callees = [...calleeMap.entries()];
    const lang = langOf(fn.filePath);
    return `
      <div class="mx-auto max-w-[1120px] px-5 py-5 lg:px-7">
        ${prototypeBanner()}
        <div class="flex flex-wrap items-end justify-between gap-2">
          <div>
            <div class="flex items-center gap-2">
              <span class="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">1-Hop call neighborhood</span>
              ${badge(lang, langTone(lang), 'code-2')}
            </div>
            <div class="mt-1 font-mono text-[16px] text-ink">${esc(fn.name)}()</div>
          </div>
          <p class="max-w-[420px] text-[10.5px] leading-4 text-muted">Click a caller to prepend it to the breadcrumb, or a callee to append it. The current function stays in the center.</p>
        </div>
        <div class="mt-4 grid items-start gap-3 lg:grid-cols-[1fr_1.25fr_1fr]">
          <div>
            <div class="mb-2 flex items-center gap-1.5 text-[10px] uppercase tracking-[0.12em] text-muted">
              <i data-lucide="arrow-down-left" class="h-3 w-3 text-info"></i>Callers <span class="font-mono">${callers.length}</span>
            </div>
            <div class="space-y-2">${callers.length ? callers.map(mapCallerCard).join('') : mapEmpty('No callers')}</div>
          </div>
          <div>
            <div class="mb-2 flex items-center gap-1.5 text-[10px] uppercase tracking-[0.12em] text-accent">
              <i data-lucide="crosshair" class="h-3 w-3"></i>Current function
            </div>
            <div class="rounded-xl border border-accent/40 bg-accent/[0.06] p-4">
              <div class="flex flex-wrap gap-1">
                ${badge(shortModule(fn.module), 'border-line-strong bg-bg text-muted', 'folder')}
                ${badge(fn.purity === 'pure' ? 'pure' : 'side-effect', fn.purity === 'pure' ? 'border-ok/25 bg-ok/10 text-ok' : 'border-warn/25 bg-warn/10 text-warn', fn.purity === 'pure' ? 'sparkles' : 'zap')}
                ${isEntryPoint(fn) ? badge('entry point', 'border-accent/25 bg-accent/10 text-accent', 'log-in') : ''}
              </div>
              <div class="mt-2 break-words font-mono text-[14px] leading-6 text-ink">${esc(signature(fn))}</div>
              <p class="mt-2 text-[11px] leading-5 text-muted">${esc(fn.description || '')}</p>
              <div class="mt-3 font-mono text-[9.5px] text-muted">${esc(fn.filePath)}:${fn.line}</div>
            </div>
            <div class="mt-2 rounded-lg border border-dashed border-line-strong px-3 py-2 text-[10px] leading-4 text-muted">
              <span class="text-muted">Story tip:</span> the breadcrumb records the path you take; use <span class="font-mono text-muted">←</span> to step back.
            </div>
          </div>
          <div>
            <div class="mb-2 flex items-center gap-1.5 text-[10px] uppercase tracking-[0.12em] text-muted">
              <i data-lucide="arrow-up-right" class="h-3 w-3 text-ok"></i>Callees <span class="font-mono">${callees.length}</span>
            </div>
            <div class="space-y-2">${callees.length ? callees.map(([id, step]) => mapCalleeCard(id, step)).join('') : mapEmpty('No resolved callees')}</div>
          </div>
        </div>
      </div>`;
  }

  function mapCallerCard(caller) {
    const target = caller.fnId ? fnIndex.get(caller.fnId) : null;
    return `
      <div class="rounded-lg border border-line bg-panel p-2.5">
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate font-mono text-[11px] text-ink">${esc(caller.name)}()</div>
            <div class="mt-0.5 truncate font-mono text-[9px] text-muted">${esc(caller.module)}</div>
          </div>
          ${target ? `<button data-action="caller" data-fn="${esc(target.id)}" class="shrink-0 rounded border border-info/25 bg-info/10 px-1.5 py-0.5 font-mono text-[9px] text-info hover:bg-info/20">Jump ↑</button>` : `<span class="shrink-0 rounded border border-line-strong px-1.5 py-0.5 font-mono text-[8.5px] text-faint">${caller.fnId ? 'outside scope' : 'external'}</span>`}
        </div>
      </div>`;
  }

  function mapCalleeCard(id, step) {
    const target = fnIndex.get(id);
    if (!target) {
      return `
        <div class="rounded-lg border border-dashed border-line-strong bg-surface p-2.5 opacity-70">
          <div class="truncate font-mono text-[11px] text-muted">${esc(step.callsFnId)}</div>
          <div class="mt-0.5 text-[9px] text-faint">outside this scope</div>
        </div>`;
    }
    return `
      <button data-action="callee" data-fn="${esc(target.id)}" class="w-full rounded-lg border border-line bg-panel p-2.5 text-left transition-colors hover:border-ok/30 hover:bg-ok/[0.04]">
        <div class="flex items-center justify-between gap-2">
          <span class="truncate font-mono text-[11px] text-ink">${esc(target.name)}()</span>
          <i data-lucide="arrow-right" class="h-3 w-3 shrink-0 text-ok/70"></i>
        </div>
        <div class="mt-0.5 truncate text-[9px] text-muted">${esc(step.title)}</div>
      </button>`;
  }

  function mapEmpty(label) {
    return `<div class="rounded-lg border border-dashed border-line-strong px-3 py-6 text-center text-[10px] text-faint">${esc(label)}</div>`;
  }

  // ---------------------------------------------------------------- split / raw

  function splitView(fn) {
    const lang = langOf(fn.filePath);
    return `
      <div class="mx-auto max-w-[1200px] px-5 py-5 lg:px-7">
        ${prototypeBanner()}
        <div class="flex flex-wrap items-end justify-between gap-2">
          <div>
            <div class="flex items-center gap-2">
              <span class="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">Split · source + story</span>
              ${badge(lang, langTone(lang), 'code-2')}
            </div>
            <div class="mt-1 font-mono text-[16px] text-ink">${esc(fn.name)}()</div>
          </div>
          <span class="font-mono text-[9.5px] text-muted">${esc(fn.filePath)}:${fn.line}</span>
        </div>
        <div class="mt-3 grid gap-3 lg:grid-cols-2">
          <section class="overflow-hidden rounded-xl border border-line bg-surface">
            <div class="flex items-center gap-2 border-b border-line px-3 py-2">
              <i data-lucide="file-code" class="h-3.5 w-3.5 text-info"></i>
              <span class="text-[11px] text-ink">Source</span>
              <span class="ml-auto font-mono text-[9px] text-faint">${esc(fn.filePath)}</span>
            </div>
            ${fn.sourceCode
              ? `<pre class="code-block max-h-[70vh] overflow-auto p-3 font-mono text-[10.5px] leading-5 text-ink">${esc(fn.sourceCode)}</pre>`
              : `<div class="p-8 text-center"><i data-lucide="file-question" class="mx-auto h-6 w-6 text-faint"></i><p class="mt-2 text-[11px] text-muted">Source is embedded only for curated sample functions in this prototype.</p><p class="mt-1 font-mono text-[9.5px] text-faint">${esc(fn.filePath)}:${fn.line}</p></div>`}
          </section>
          <section class="rounded-xl border border-line bg-panel">
            <div class="flex items-center gap-2 border-b border-line px-3 py-2">
              <i data-lucide="route" class="h-3.5 w-3.5 text-accent"></i>
              <span class="text-[11px] text-ink">Execution story</span>
              <span class="ml-auto font-mono text-[9px] text-faint">${fn.stepCount} steps</span>
            </div>
            <div class="space-y-2 p-3">
              ${fn.steps.length ? fn.steps.map(compactStep).join('') : emptyInline('circle-slash', 'No steps extracted', 'Nothing to show for this function.')}
            </div>
          </section>
        </div>
      </div>`;
  }

  function compactStep(step, index) {
    const meta = KIND[step.kind] || KIND.transform;
    const callee = step.callsFnId ? fnIndex.get(step.callsFnId) : null;
    return `
      <div class="flex items-start gap-2 rounded-lg border border-line bg-surface p-2">
        <span class="mt-0.5 grid h-5 w-5 shrink-0 place-items-center rounded-full border ${meta.border} ${meta.bg} font-mono text-[8px] ${meta.text}">${index + 1}</span>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="font-mono text-[8.5px] ${meta.text}">${meta.label}</span>
            <span class="text-[10.5px] text-ink">${esc(step.title)}</span>
          </div>
          ${step.output ? `<div class="mt-0.5 font-mono text-[9px] text-muted">→ ${esc(step.output)}</div>` : ''}
          ${callee ? `<button data-action="callee" data-fn="${esc(callee.id)}" class="mt-1 font-mono text-[9px] text-accent hover:text-accent">Inspect ${esc(callee.name)}() →</button>` : ''}
        </div>
      </div>`;
  }

  function rawView(fn) {
    return `
      <div class="mx-auto max-w-[1000px] px-5 py-5 lg:px-7">
        ${prototypeBanner()}
        <div class="flex flex-wrap items-end justify-between gap-2">
          <div>
            <span class="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted">Raw manifest record</span>
            <div class="mt-1 font-mono text-[16px] text-ink">${esc(fn.name)}()</div>
          </div>
          <span class="font-mono text-[9.5px] text-muted">${esc(fn.id)}</span>
        </div>
        <div class="mt-3 overflow-hidden rounded-xl border border-line bg-surface">
          <div class="flex items-center justify-between border-b border-line px-3 py-2">
            <span class="font-mono text-[10px] text-muted">${esc(fn.id)}.json</span>
            <button data-action="copy-json" class="inline-flex items-center gap-1 rounded border border-line-strong px-1.5 py-1 text-[9.5px] text-muted hover:border-line-strong hover:text-ink"><i data-lucide="copy" class="h-2.5 w-2.5"></i> Copy JSON</button>
          </div>
          <pre class="max-h-[72vh] overflow-auto p-3 font-mono text-[10px] leading-5 text-ink">${esc(JSON.stringify(fn, null, 2))}</pre>
        </div>
        <p class="mt-2 text-[10px] text-faint">Trimmed prototype record. The live app serves the full manifest at <span class="font-mono">/api/fn-graph.json</span>.</p>
      </div>`;
  }

  // ---------------------------------------------------------------- drawer

  function renderDrawer() {
    const drawer = $('drawer');
    drawer.style.width = state.drawerOpen ? '320px' : '0px';
    drawer.style.borderLeftWidth = state.drawerOpen ? '1px' : '0px';
    drawer.style.opacity = state.drawerOpen ? '1' : '0';
    drawer.classList.toggle('pointer-events-none', !state.drawerOpen);
    const overlay = (window.innerWidth || 1280) < 1180;
    $('drawer-backdrop').classList.toggle('hidden', !(state.drawerOpen && overlay));
    if (!state.drawerOpen) return;

    const fn = currentFn();
    const body = $('drawer-body');
    if (!fn) {
      body.innerHTML = `<div class="p-6 text-center text-[11px] text-muted">Select a function first.</div>`;
      return;
    }

    const lang = langOf(fn.filePath);
    const args = state.argsDraft === null ? argsTemplate(fn) : state.argsDraft;
    const tab = state.drawerTab;
    body.innerHTML = `
      <div class="space-y-3">
        <div class="flex items-center gap-1 rounded-md border border-line bg-bg p-0.5">
          ${drawerTabButton('simulate', 'Simulate trace', 'flask-conical')}
          ${drawerTabButton('live', 'Live runtime', 'zap')}
        </div>
        <div class="rounded-lg border border-line bg-panel p-3">
          <div class="flex items-center gap-1.5">
            <span class="truncate font-mono text-[11.5px] text-ink">${esc(fn.name)}()</span>
            ${badge(lang, langTone(lang))}
          </div>
          <div class="mt-1 truncate font-mono text-[9px] text-muted">${esc(fn.module)}</div>
        </div>
        ${tab === 'live' ? `<div class="rounded-md border border-info/20 bg-info/[0.06] px-2.5 py-2 text-[10px] leading-4 text-info/80"><span class="font-medium">Prototype note:</span> the real app runs TS/JS through Node and falls back to a simulated trace for other languages. This static mock always simulates.</div>` : ''}
        <div>
          <div class="mb-1.5 flex items-center justify-between">
            <label for="args-input" class="text-[10px] font-medium uppercase tracking-[0.12em] text-muted">Arguments</label>
            <span class="font-mono text-[9px] text-faint">JSON</span>
          </div>
          <textarea id="args-input" spellcheck="false" class="h-[150px] w-full resize-y rounded-md border ${state.runError ? 'border-err/50' : 'border-line'} bg-bg p-2.5 font-mono text-[10.5px] leading-5 text-ink focus:border-accent/50 focus:outline-none focus:ring-1 focus:ring-accent/30">${esc(args)}</textarea>
          ${state.runError ? `<p class="mt-1 text-[10px] text-err">${esc(state.runError)}</p>` : ''}
        </div>
        <div class="flex gap-2">
          <button data-action="run" ${state.running ? 'disabled' : ''} class="flex flex-1 items-center justify-center gap-1.5 rounded-md border border-accent bg-accent px-3 py-2 text-[11px] font-medium text-accent-ink transition-colors hover:bg-accent/85 disabled:cursor-wait disabled:opacity-60">
            <i data-lucide="${state.running ? 'loader-2' : 'play'}" class="h-3.5 w-3.5 ${state.running ? 'animate-spin' : ''}"></i>${state.running ? 'Running…' : tab === 'live' ? 'Run live' : 'Run function'}
          </button>
          <button data-action="reset-args" class="rounded-md border border-line px-2.5 py-2 text-[11px] text-muted transition-colors hover:border-line-strong hover:text-ink">Reset</button>
        </div>
        <div>
          <div class="mb-1.5 flex items-center justify-between">
            <span class="text-[10px] font-medium uppercase tracking-[0.12em] text-muted">Trace output</span>
            ${state.run ? `<span class="font-mono text-[9px] text-ok">${state.run.trace.length} steps · ${state.run.durationMs.toFixed(1)} ms</span>` : ''}
          </div>
          ${state.run ? renderRun(state.run) : `<div class="rounded-lg border border-dashed border-line-strong px-3 py-6 text-center"><i data-lucide="terminal" class="mx-auto h-5 w-5 text-faint"></i><p class="mt-2 text-[10.5px] text-muted">Run the function to see a step-by-step trace.</p></div>`}
        </div>
        <div class="border-t border-line/70 pt-2 text-[9px] leading-4 text-faint">
          ${tab === 'live' ? 'Live mode is simulated in this prototype.' : 'Deterministic trace built from the manifest steps.'} No source is executed by this page.
        </div>
      </div>`;
  }

  function drawerTabButton(id, label, icon) {
    const active = state.drawerTab === id;
    return `
      <button data-action="drawer-tab" data-tab="${id}" aria-pressed="${active}"
        class="flex flex-1 items-center justify-center gap-1 rounded px-2 py-1.5 text-[10px] transition-colors ${active ? 'bg-active text-ink shadow-sm' : 'text-muted hover:text-ink'}">
        <i data-lucide="${icon}" class="h-3 w-3"></i>${label}
      </button>`;
  }

  function renderRun(run) {
    const tone = run.mode === 'live'
      ? { border: 'border-info/25', bg: 'bg-info/[0.04]', text: 'text-info', icon: 'text-info' }
      : { border: 'border-ok/25', bg: 'bg-ok/[0.04]', text: 'text-ok', icon: 'text-ok' };
    return `
      <div class="overflow-hidden rounded-lg border ${tone.border} ${tone.bg}">
        <div class="flex items-center gap-1.5 border-b ${tone.border} px-2.5 py-1.5">
          <i data-lucide="check" class="h-3 w-3 ${tone.icon}"></i>
          <span class="text-[10px] ${tone.text}">${run.mode === 'live' ? 'Live response (prototype)' : 'Simulated response'}</span>
        </div>
        <pre class="max-h-[260px] overflow-auto p-2.5 font-mono text-[9.5px] leading-5 text-ink">${esc(JSON.stringify(run.payload, null, 2))}</pre>
      </div>`;
  }

  // ---------------------------------------------------------------- actions

  function selectFunction(id) {
    if (!fnIndex.has(id)) return;
    state.selectedId = id;
    state.trail = [id];
    resetRunState();
    ensureSelectedExpanded();
    renderTopbar();
    renderSidebar();
    renderCanvas();
    renderDrawer();
    refreshIcons();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function goCaller(id) {
    if (!fnIndex.has(id)) return;
    state.selectedId = id;
    state.trail = [id, ...state.trail];
    resetRunState();
    ensureSelectedExpanded();
    renderAll();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function goCallee(id) {
    if (!fnIndex.has(id)) return;
    state.selectedId = id;
    state.trail = [...state.trail, id];
    resetRunState();
    ensureSelectedExpanded();
    renderAll();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function clickTrail(index) {
    if (index < 0 || index >= state.trail.length) return;
    state.trail = state.trail.slice(0, index + 1);
    state.selectedId = state.trail[state.trail.length - 1];
    resetRunState();
    ensureSelectedExpanded();
    renderAll();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function goBack() {
    if (state.trail.length <= 1) return;
    state.trail = state.trail.slice(0, -1);
    state.selectedId = state.trail[state.trail.length - 1];
    resetRunState();
    ensureSelectedExpanded();
    renderAll();
    $('main').scrollTop = 0;
    revealSelectedInSidebar();
  }

  function runInvoke() {
    const fn = currentFn();
    if (!fn || state.running) return;
    const raw = $('args-input') ? $('args-input').value : '';
    state.argsDraft = raw;
    let parsed;
    try {
      parsed = JSON.parse(raw);
      state.runError = '';
    } catch (err) {
      state.runError = `Invalid JSON: ${err.message}`;
      renderDrawer();
      refreshIcons();
      const input = $('args-input');
      if (input) input.focus();
      return;
    }

    state.running = true;
    state.run = null;
    renderDrawer();
    refreshIcons();

    const mode = state.drawerTab;
    window.setTimeout(() => {
      const trace = fn.steps.map((step) => ({
        id: step.id,
        kind: step.kind,
        status: mode === 'live' ? 'live (prototype)' : 'simulated',
        output: step.output,
      }));
      const durationMs = Number((8 + Math.random() * 12).toFixed(1));
      const payload = {
        ok: true,
        mode,
        function: fn.name,
        inputs: parsed,
        trace: Object.fromEntries(trace.map((t) => [t.id, { status: t.status, output: t.output }])),
        durationMs,
      };
      state.run = { mode, payload, trace, durationMs };
      state.running = false;
      renderDrawer();
      refreshIcons();
      toast(`${fn.name}() ${mode === 'live' ? 'ran in live (prototype) mode' : 'simulated'}`, 'success');
    }, 650);
  }

  function toast(message, tone = 'info') {
    const id = ++state.toastSeq;
    const host = $('toasts');
    const icon = tone === 'success' ? 'check' : tone === 'error' ? 'alert-circle' : 'info';
    const color = tone === 'success' ? 'text-ok' : tone === 'error' ? 'text-err' : 'text-info';
    const el = document.createElement('div');
    el.dataset.toast = String(id);
    el.className = 'toast pointer-events-auto flex items-start gap-2 rounded-lg border border-line-strong bg-raised px-3 py-2.5 shadow-xl';
    el.innerHTML = `<i data-lucide="${icon}" class="mt-0.5 h-3.5 w-3.5 shrink-0 ${color}"></i><span class="text-[11px] leading-4 text-ink">${esc(message)}</span>`;
    host.appendChild(el);
    refreshIcons();
    window.setTimeout(() => {
      el.style.opacity = '0';
      el.style.transform = 'translateY(4px)';
      el.style.transition = 'opacity .15s, transform .15s';
      window.setTimeout(() => el.remove(), 160);
    }, 2600);
  }

  async function copyText(text, label) {
    try {
      await navigator.clipboard.writeText(text);
      toast(`${label} copied`, 'success');
    } catch {
      toast('Clipboard blocked — select the text manually.', 'error');
    }
  }

  function syncSearchClear() {
    const input = $('search');
    const hasValue = input.value.length > 0;
    $('search-clear').classList.toggle('hidden', !hasValue);
    $('search-kbd').classList.toggle('hidden', hasValue);
  }

  // ---------------------------------------------------------------- events

  document.addEventListener('click', (event) => {
    const el = event.target.closest('[data-action]');
    if (!el) {
      if (state.projectMenuOpen && !event.target.closest('#project-switcher')) {
        state.projectMenuOpen = false;
        renderProjectMenu();
        refreshIcons();
      }
      return;
    }
    if (el.getAttribute('aria-disabled') === 'true') return;

    const action = el.dataset.action;
    switch (action) {
      case 'toggle-project-menu':
        state.projectMenuOpen = !state.projectMenuOpen;
        renderProjectMenu();
        refreshIcons();
        break;
      case 'switch-project':
        state.projectMenuOpen = false;
        activateProject(el.dataset.project);
        refreshIcons();
        toast(`Switched to ${project.label}`, 'info');
        break;
      case 'view':
        setView(el.dataset.view);
        break;
      case 'sidebar-mode':
        state.mode = el.dataset.mode;
        renderSidebar();
        refreshIcons();
        break;
      case 'toggle-group': {
        const group = el.dataset.group;
        if (state.expandedGroups.has(group)) state.expandedGroups.delete(group);
        else state.expandedGroups.add(group);
        renderSidebarList();
        refreshIcons();
        break;
      }
      case 'toggle-file': {
        const file = el.dataset.file;
        if (state.expandedFiles.has(file)) state.expandedFiles.delete(file);
        else state.expandedFiles.add(file);
        renderSidebarList();
        refreshIcons();
        break;
      }
      case 'select-fn':
        selectFunction(el.dataset.fn);
        break;
      case 'more-steps':
        state.stepLimit = currentFn().steps.length;
        renderCanvas();
        refreshIcons();
        break;
      case 'fewer-steps':
        state.stepLimit = 6;
        renderCanvas();
        refreshIcons();
        break;
      case 'caller':
        goCaller(el.dataset.fn);
        break;
      case 'callee':
        goCallee(el.dataset.fn);
        break;
      case 'trail':
        clickTrail(Number(el.dataset.index));
        break;
      case 'back':
        goBack();
        break;
      case 'toggle-theme':
        state.theme = state.theme === 'paper' ? 'graphite' : 'paper';
        try {
          window.localStorage.setItem(THEME_KEY, state.theme);
        } catch {
          // Theme still applies for this session when storage is blocked.
        }
        applyTheme();
        break;
      case 'toggle-drawer':
        state.drawerOpen = !state.drawerOpen;
        renderDrawer();
        renderTopbar();
        refreshIcons();
        break;
      case 'open-drawer':
        state.drawerOpen = true;
        renderDrawer();
        renderTopbar();
        refreshIcons();
        break;
      case 'drawer-tab':
        state.drawerTab = el.dataset.tab;
        state.run = null;
        state.runError = '';
        renderDrawer();
        refreshIcons();
        break;
      case 'run':
        runInvoke();
        break;
      case 'reset-args':
        state.argsDraft = null;
        state.run = null;
        state.runError = '';
        renderDrawer();
        refreshIcons();
        break;
      case 'clear-search':
        state.search = '';
        $('search').value = '';
        syncSearchClear();
        renderSidebarList();
        refreshIcons();
        $('search').focus();
        break;
      case 'dismiss-banner':
        state.bannerDismissed = true;
        renderCanvas();
        refreshIcons();
        break;
      case 'copy-signature':
        copyText(signature(currentFn()), 'Signature');
        break;
      case 'copy-json':
        copyText(JSON.stringify(currentFn(), null, 2), 'JSON');
        break;
      case 'toast-source':
        toast('Source view is not wired in the static prototype.', 'info');
        break;
      case 'rescan':
        toast('Rescan is simulated in this prototype.', 'info');
        break;
      case 'toggle-sidebar':
        state.sidebarOpen = !state.sidebarOpen;
        renderSidebar();
        renderTopbar();
        refreshIcons();
        break;
      default:
        break;
    }
  });

  document.addEventListener('input', (event) => {
    if (event.target.id === 'search') {
      state.search = event.target.value;
      syncSearchClear();
      renderSidebarList();
      refreshIcons();
    }
    if (event.target.id === 'args-input') {
      state.argsDraft = event.target.value;
      if (state.runError) {
        state.runError = '';
        event.target.classList.remove('border-err/50');
        event.target.classList.add('border-line');
        const message = event.target.parentElement.querySelector('p.text-err');
        if (message) message.remove();
      }
    }
  });

  document.addEventListener('keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
      event.preventDefault();
      $('search').focus();
      $('search').select();
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key === 'Enter' && state.drawerOpen) {
      event.preventDefault();
      runInvoke();
      return;
    }
    if (event.altKey && event.key === 'ArrowLeft' && !isTypingTarget(document.activeElement)) {
      event.preventDefault();
      goBack();
      return;
    }
    if (event.key === 'Escape') {
      if (state.projectMenuOpen) {
        state.projectMenuOpen = false;
        renderProjectMenu();
        refreshIcons();
      } else if (state.drawerOpen) {
        state.drawerOpen = false;
        renderDrawer();
        renderTopbar();
        refreshIcons();
      } else if (document.activeElement === $('search')) {
        $('search').blur();
      }
      return;
    }
    if (isTypingTarget(document.activeElement) || event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === '/') {
      event.preventDefault();
      $('search').focus();
      $('search').select();
      return;
    }
    const views = { '1': 'story', '2': 'map', '3': 'split', '4': 'raw' };
    if (views[event.key]) {
      event.preventDefault();
      setView(views[event.key]);
    }
  });

  // ---------------------------------------------------------------- boot

  function renderAll() {
    renderTopbar();
    renderProjectMenu();
    renderSidebar();
    renderCanvas();
    renderDrawer();
    refreshIcons();
  }

  fnIndex = new Map(project.functions.map((f) => [f.id, f]));
  ensureSelectedExpanded();
  syncSearchClear();
  applyTheme();
  renderAll();
  revealSelectedInSidebar();
})();
