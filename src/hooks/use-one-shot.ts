import { useCallback, useEffect, useRef, useState } from "react";

/**
 * One-shot CSS-class trigger for event-driven icon animations (e.g. the send-icon "launch" on a
 * successful submit, or the download arrow "bounce" on a finished download). `fire()` flips `active`
 * to true for `durationMs`, then back to false — apply `active` as a class so the keyframe plays once.
 *
 * ```tsx
 * const launch = useOneShot();
 * // on mutation success: launch.fire();
 * <Ic name="send" cls={launch.active ? "ic launch" : "ic"} />
 * ```
 */
export function useOneShot(durationMs = 450) {
  const [active, setActive] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const fire = useCallback(() => {
    if (timer.current) clearTimeout(timer.current);
    setActive(true);
    timer.current = setTimeout(() => setActive(false), durationMs);
  }, [durationMs]);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  return { active, fire };
}
