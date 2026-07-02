/**
 * use-modal-focus — minimal, dependency-free focus management for aria-modal dialogs.
 *
 * While `active`:
 *   1. moves focus onto the dialog container (attach the returned ref + tabIndex={-1});
 *   2. traps Tab / Shift+Tab inside the dialog (loops first ⇄ last focusable);
 *   3. on close/unmount, restores focus to the element that opened the dialog.
 *
 * Usage:
 *   const dialogRef = useModalFocus<HTMLDivElement>(open);
 *   <div ref={dialogRef} tabIndex={-1} role="dialog" aria-modal="true">…</div>
 */
import { useEffect, useRef } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  '[tabindex]:not([tabindex="-1"])',
].join(", ");

/** Visible-ish check — skips display:none / detached candidates. */
const isTabbable = (el: HTMLElement) => el.getClientRects().length > 0;

export function useModalFocus<T extends HTMLElement>(active: boolean) {
  const ref = useRef<T | null>(null);

  useEffect(() => {
    if (!active) return;
    const dialog = ref.current;
    if (!dialog) return;

    // Remember the opener so we can hand focus back on close.
    const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;

    // Focus the container itself (requires tabIndex={-1} on the element),
    // unless something inside the dialog already grabbed focus (e.g. autoFocus).
    if (!dialog.contains(document.activeElement)) {
      dialog.focus({ preventScroll: true });
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const focusables = Array.from(
        dialog.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter(isTabbable);
      if (focusables.length === 0) {
        // Nothing tabbable — keep focus pinned on the container.
        e.preventDefault();
        dialog.focus({ preventScroll: true });
        return;
      }
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const current = document.activeElement;
      if (e.shiftKey) {
        if (current === first || current === dialog || !dialog.contains(current)) {
          e.preventDefault();
          last.focus();
        }
      } else if (current === last || !dialog.contains(current)) {
        e.preventDefault();
        first.focus();
      }
    };

    dialog.addEventListener("keydown", onKeyDown);
    return () => {
      dialog.removeEventListener("keydown", onKeyDown);
      // Restore the opener's focus (only if it is still in the document).
      if (opener && document.contains(opener)) {
        opener.focus({ preventScroll: true });
      }
    };
  }, [active]);

  return ref;
}
