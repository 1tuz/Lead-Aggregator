import {
  commands,
  type AppError,
  type ExportFormat,
  type ExportReceipt,
  type HealthInfo,
  type Organization,
  type RunSummary,
  type SearchRequest,
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
  exportResults: async (format: ExportFormat, limit = 5000) =>
    unwrap(await commands.exportResults(format, limit)),
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
