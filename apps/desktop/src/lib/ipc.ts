import {
  commands,
  type AppError,
  type ExportFormat,
  type ExportReceipt,
  type HealthInfo,
  type Organization,
  type RunSummary,
  type SearchRequest,
  type SearchRunInfo,
  type SourceKind,
} from '../bindings';

export type {
  AppError,
  ExportFormat,
  ExportReceipt,
  HealthInfo,
  Organization,
  RunSummary,
  SearchRequest,
  SearchRunInfo,
  SourceKind,
};

type IpcResult<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };

function unwrap<T>(result: IpcResult<T>): T {
  if (result.status === 'error') throw result.error;
  return result.data;
}

export const api = {
  startSearch: async (request: SearchRequest) => unwrap(await commands.startSearch(request)),
  cancelSearch: async () => unwrap(await commands.cancelSearch()),
  recentResults: async (limit = 500) => unwrap(await commands.recentResults(limit)),
  recentRuns: async (limit = 30) => unwrap(await commands.recentRuns(limit)),
  resultsForRun: async (runId: string, limit = 5000) =>
    unwrap(await commands.resultsForRun(runId, limit)),
  exportResults: async (runId: string, format: ExportFormat, limit = 5000) =>
    unwrap(await commands.exportResults(runId, format, limit)),
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
