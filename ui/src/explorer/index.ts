/** Barrel for the sidebar explorer; App owns selection, search and trail state. */
export { default as Explorer } from './Explorer';
export type { ExplorerProps, ExplorerError } from './Explorer';
export { default as Search } from './Search';
export { default as FunctionList, FunctionRow } from './FunctionList';
export { default as ModuleTree } from './ModuleTree';
export {
  ENTRY_POINT_LIMIT,
  SEARCH_LIMIT,
  entryPointCount,
  entryPointPage,
  searchFunctions,
} from './entryPoints';
export type { EntryPointPage, FunctionSearchResult } from './entryPoints';
export {
  ROOT_GROUP,
  basename,
  buildModuleTree,
  fileLabel,
  groupKey,
  normalizePath,
} from './grouping';
export type { ModuleFile, ModuleGroup } from './grouping';
export {
  appendCrumb,
  backTrail,
  crumbFor,
  currentCrumb,
  prependCrumb,
  startTrail,
  truncateTrail,
} from './trail';
export type { TrailCrumb } from './trail';
