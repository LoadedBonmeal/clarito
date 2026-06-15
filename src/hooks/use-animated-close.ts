import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Plays the design's `.modal-back.closing` exit animation (anModalOut, ~120ms) before the PARENT
 * unmounts the modal — so dialogs fade+scale out instead of vanishing instantly.
 *
 * Usage inside a modal component (the parent renders `{show && <Modal onClose={() => setShow(false)} />}`):
 * ```tsx
 * const { closing, close } = useAnimatedClose(onClose);
 * return createPortal(
 *   <div className={`modal-back ${closing ? "closing" : "show"}`}
 *        onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
 *     <div className="modal">… <button onClick={close}>✕</button> …</div>
 *   </div>, document.body);
 * ```
 * Call `close()` from the X button, backdrop click and Cancel button (instead of `onClose`). `onClose`
 * (the parent's `setShow(false)`) runs after the animation, performing the actual unmount.
 */
export function useAnimatedClose(onClose: () => void, durationMs = 140) {
  const [closing, setClosing] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const busy = useRef(false);

  const close = useCallback(() => {
    if (busy.current) return; // ignore repeated close requests during the exit animation
    busy.current = true;
    setClosing(true);
    timer.current = setTimeout(() => {
      // Reset BEFORE unmounting so components that stay mounted (an `open` prop + `if (!open) return
      // null`) re-open cleanly in `.show`; for components the parent unmounts this is a harmless no-op.
      busy.current = false;
      setClosing(false);
      onClose();
    }, durationMs);
  }, [onClose, durationMs]);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  return { closing, close };
}
