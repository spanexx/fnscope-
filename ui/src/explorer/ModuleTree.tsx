import React, { useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, FileCode, Folder } from 'lucide-react';
import EmptyState from '../components/EmptyState';
import { formatCount } from '../lib/format';
import type { FnGraph } from '../lib/types';
import { FunctionRow } from './FunctionList';
import { buildModuleTree } from './grouping';

interface ModuleTreeProps {
  graph: FnGraph;
  selectedId: string | null;
  onSelect: (id: string) => void;
}

/**
 * Directory-shaped view of the real manifest. Groups and files start closed,
 * but the selected function's path opens automatically so the row is visible.
 */
export default function ModuleTree({ graph, selectedId, onSelect }: ModuleTreeProps) {
  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(() => new Set());
  const [expandedFiles, setExpandedFiles] = useState<Set<string>>(() => new Set());
  const tree = useMemo(() => buildModuleTree(graph.functions || []), [graph]);

  const locations = useMemo(() => {
    const map = new Map<string, { group: string; file: string }>();
    for (const group of tree) {
      for (const file of group.files) {
        for (const fn of file.functions) map.set(fn.id, { group: group.key, file: file.path });
      }
    }
    return map;
  }, [tree]);

  useEffect(() => {
    if (!selectedId) return;
    const location = locations.get(selectedId);
    if (!location) return;
    setExpandedGroups((prev) => {
      if (prev.has(location.group)) return prev;
      const next = new Set(prev);
      next.add(location.group);
      return next;
    });
    setExpandedFiles((prev) => {
      if (prev.has(location.file)) return prev;
      const next = new Set(prev);
      next.add(location.file);
      return next;
    });
  }, [selectedId, locations]);

  const toggle = (
    setter: React.Dispatch<React.SetStateAction<Set<string>>>,
    key: string
  ) => {
    setter((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  if (!tree.length) {
    return (
      <EmptyState
        compact
        icon={<Folder className="h-4 w-4" aria-hidden="true" />}
        title="No files in this scope"
        description="The manifest loaded but contains no function file paths."
      />
    );
  }

  return (
    <div className="py-1">
      {tree.map((group) => {
        const open = expandedGroups.has(group.key);
        return (
          <div key={group.key}>
            <button
              type="button"
              aria-expanded={open}
              onClick={() => toggle(setExpandedGroups, group.key)}
              className="flex w-full items-center gap-1.5 px-2.5 py-2 text-left transition-colors hover:bg-wash focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent/50 motion-reduce:transition-none"
            >
              {open ? (
                <ChevronDown className="h-3 w-3 shrink-0 text-muted" aria-hidden="true" />
              ) : (
                <ChevronRight className="h-3 w-3 shrink-0 text-muted" aria-hidden="true" />
              )}
              <Folder className="h-3.5 w-3.5 shrink-0 text-warn/70" aria-hidden="true" />
              <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-ink">
                {group.label}
              </span>
              <span className="rounded bg-raised px-1.5 py-0.5 font-mono text-[11px] text-muted">
                {formatCount(group.functionCount)}
              </span>
            </button>

            {open
              ? group.files.map((file) => {
                  const fileOpen = expandedFiles.has(file.path);
                  return (
                    <div key={file.path}>
                      <button
                        type="button"
                        aria-expanded={fileOpen}
                        onClick={() => toggle(setExpandedFiles, file.path)}
                        title={file.path}
                        className="flex w-full items-center gap-1.5 py-1.5 pl-7 pr-3 text-left transition-colors hover:bg-wash focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent/50 motion-reduce:transition-none"
                      >
                        {fileOpen ? (
                          <ChevronDown
                            className="h-2.5 w-2.5 shrink-0 text-faint"
                            aria-hidden="true"
                          />
                        ) : (
                          <ChevronRight
                            className="h-2.5 w-2.5 shrink-0 text-faint"
                            aria-hidden="true"
                          />
                        )}
                        <FileCode className="h-3 w-3 shrink-0 text-info/60" aria-hidden="true" />
                        <span className="min-w-0 flex-1 truncate font-mono text-[12.5px] text-muted">
                          {file.label}
                        </span>
                        <span className="font-mono text-[11px] text-faint">
                          {formatCount(file.functionCount)}
                        </span>
                      </button>

                      {fileOpen
                        ? file.functions.map((fn) => (
                            <FunctionRow
                              key={fn.id}
                              fn={fn}
                              selected={fn.id === selectedId}
                              onSelect={onSelect}
                              indent
                            />
                          ))
                        : null}
                    </div>
                  );
                })
              : null}
          </div>
        );
      })}
    </div>
  );
}
