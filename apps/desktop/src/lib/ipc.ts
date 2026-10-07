import {
  commands,
  type AppError,
  type CatalogCategory,
  type CatalogCity,
  type CatalogRubric,
  type CollectionJobInfo,
  type CollectionPreset,
  type ExportFormat,
  type ExportReceipt,
  type HealthInfo,
  type Organization,
  type ProviderSearchConfig,
  type RunResultsPage,
  type RunSummary,
  type SearchRequest,
  type SearchRunInfo,
  type SourceKind,
} from '../bindings';

export type ExportColumn = 'company' | 'category' | 'sources' | 'address' | 'phone' | 'inn' | 'website' | 'tags';

export type {
  AppError,
  CatalogCategory,
  CatalogCity,
  CatalogRubric,
  CollectionJobInfo,
  CollectionPreset,
  ExportFormat,
  ExportReceipt,
  HealthInfo,
  Organization,
  ProviderSearchConfig,
  RunResultsPage,
  RunSummary,
  SearchRequest,
  SearchRunInfo,
  SourceKind,
};

export type ProviderRunState = 'queued' | 'running' | 'paused' | 'rateLimited' | 'blocked' | 'captchaRequired' | 'challengeRequired' | 'completed' | 'failed' | 'cancelled';

export type ProviderStatus = {
  source: SourceKind;
  region: string;
  state: ProviderRunState;
  current: number;
  total: number | null;
  message: string;
  retryAfterSeconds: number | null;
};

export type ScrapeProgress = {
  phase: 'discovering' | 'enriching' | 'resolving' | 'saving' | 'done';
  current: number;
  total: number | null;
  message: string;
  source: SourceKind | null;
  region: string | null;
  state: ProviderRunState | null;
  retryAfterSeconds: number | null;
};

type IpcResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };

function typed<T>(promise: Promise<T>): Promise<IpcResult<T>> {
  return Promise.resolve(promise).then(
    (data): IpcResult<T> => ({ status: 'ok', data }),
    (error): IpcResult<T> => ({ status: 'error', error: error as AppError }),
  );
}

function unwrap<T>(result: IpcResult<T>): T {
  if (result.status === 'error') throw result.error;
  return result.data;
}

export const api = {
  startSearch: async (request: SearchRequest) => unwrap(await commands.startSearch(request)),
  resumeSearch: async (jobId: string) => unwrap(await commands.resumeSearch(jobId)),
  pauseProvider: async (source: SourceKind) => unwrap(await commands.pauseProvider(source)),
  resumeProvider: async (source: SourceKind) => unwrap(await commands.resumeProvider(source)),
  cancelSearch: async () => unwrap(await commands.cancelSearch()),
  recentResults: async (limit = 500) => unwrap(await commands.recentResults(limit)),
  recentRuns: async (limit = 30) => unwrap(await commands.recentRuns(limit)),
  recentCollectionJobs: async (limit = 30) => unwrap(await commands.recentCollectionJobs(limit)),
  resultsForRun: async (runId: string, limit = 5000) =>
    unwrap(await commands.resultsForRun(runId, limit)),
  resultsForRunPage: async (runId: string, offset = 0, limit = 500) =>
    unwrap(await commands.resultsForRunPage(runId, offset, limit)),
  exportResults: async (runId: string, format: ExportFormat) =>
    unwrap(await commands.exportResults(runId, format)),
  catalogCities: async () => unwrap(await typed(commands.catalogCitiesList())),
  catalogCityRubrics: async (cityId: string) => unwrap(await typed(commands.catalogCityRubrics(cityId))),
  catalogCategories: async () => unwrap(await typed(commands.catalogCategoriesList())),
  checkForUpdates: async () => unwrap(await commands.checkForUpdates()),
  installUpdate: async () => unwrap(await commands.installUpdate()),
  health: () => commands.health(),
};

export function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message;
    if (typeof message === 'string') return message;
  }
  return 'Неизвестная ошибка';
}

export function searchRequestLabel(request: SearchRequest): string {
  const rubrics = request.query.split(',');
  const sources = request.sources ?? ['twoGis'];
  if (sources.length === 1 && sources[0] === 'twoGis' && rubrics.every((id) => /^\d+$/.test(id))) {
    return `2GIS · ${rubrics.length} рубрик`;
  }
  return request.query;
}
