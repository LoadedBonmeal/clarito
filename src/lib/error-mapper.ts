import type { AppErrorPayload } from '../types';

/**
 * Map a thrown value (typically from Tauri invoke) into a user-safe Romanian message.
 * Internal error kinds (Database/Migration/Io/Json/Tauri/Http/Other) are logged to console
 * and replaced with the fallback — never leak SQLite, IO, or library internals to the toast.
 */
export function formatError(e: unknown, fallback = 'A apărut o eroare neașteptată.'): string {
  if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
    const p = e as AppErrorPayload;
    switch (p.kind) {
      case 'Validation':
        return p.message || fallback;
      case 'NotFound':
        return p.message || 'Element negăsit.';
      case 'Conflict':
        return p.message || 'Conflict de date.';
      case 'Database':
      case 'Migration':
      case 'Io':
      case 'Json':
      case 'Tauri':
      case 'Http':
      case 'Xml':
      case 'Pdf':
      case 'Other':
        console.error('[app-error]', p);
        return fallback;
      default:
        console.error('[app-error:unknown-kind]', p);
        return fallback;
    }
  }
  if (typeof e === 'string') return e;
  if (e instanceof Error) {
    console.error('[js-error]', e);
    return fallback;
  }
  console.error('[unknown-error]', e);
  return fallback;
}
