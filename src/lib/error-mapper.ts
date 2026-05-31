import type { AppErrorPayload } from '../types';

/**
 * Map a thrown value (typically from Tauri invoke) into a user-safe Romanian message.
 * User-facing kinds (Validation, NotFound, Conflict) pass their message through.
 * Internal kinds (Database, Migration, Io, Json, Tauri, Other) log to console and return a
 * clean RO fallback — never leak SQLite, IO, or library internals to the toast.
 * Aligned to actual AppError variants in src-tauri/src/error.rs.
 */
export function formatError(e: unknown, fallback = 'A apărut o eroare neașteptată.'): string {
  if (e && typeof e === 'object' && 'kind' in e && 'message' in e) {
    const p = e as AppErrorPayload;
    switch (p.kind) {
      // ── User-facing: pass message through ──────────────────────────────
      case 'Validation':
        return p.message || fallback;
      case 'NotFound':
        return p.message || 'Element negăsit.';
      case 'Conflict':
        return p.message || 'Conflict de date.';
      // ── Export-specific: clean RO messages ─────────────────────────────
      case 'Xlsx':
        console.error('[app-error:xlsx]', p);
        return 'Eroare la generarea fișierului Excel.';
      case 'Xml':
        console.error('[app-error:xml]', p);
        return 'Eroare la generarea fișierului XML.';
      case 'Pdf':
        console.error('[app-error:pdf]', p);
        return 'Eroare la generarea PDF.';
      case 'Archive':
        console.error('[app-error:archive]', p);
        return `Eroare la arhivă/backup: ${p.message || 'operațiune eșuată.'}`;
      // ── Internal: log + generic fallback ───────────────────────────────
      case 'Database':
      case 'Migration':
      case 'Io':
      case 'Json':
      case 'Tauri':
      case 'Other':
        console.error('[app-error]', p);
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
