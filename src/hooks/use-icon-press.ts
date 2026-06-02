import { useEffect } from "react";

/**
 * App-wide one-shot icon press animation. On pointerdown, finds the nearest
 * pressable control of the press target; if it contains an <svg data-icon>,
 * (re)triggers its `.rf-pressing` keyframe (defined per-icon in rf.css). This
 * animates every icon-bearing button / link / nav item without per-component
 * wiring. Safe under prefers-reduced-motion (the CSS disables the animation;
 * a timeout still cleans up the class).
 */
const PRESSABLE = 'button, a[href], [role="button"]';

export function useIconPressAnimation() {
  useEffect(() => {
    function onPointerDown(e: PointerEvent) {
      if (!e.isPrimary || e.button !== 0) return;
      const ctrl = (e.target as Element | null)?.closest?.(PRESSABLE) as HTMLElement | null;
      if (!ctrl || (ctrl as HTMLButtonElement).disabled) return;
      if (!ctrl.querySelector("svg[data-icon]")) return;

      // Re-trigger so rapid presses replay: remove, force reflow, re-add.
      ctrl.classList.remove("rf-pressing");
      void ctrl.offsetWidth;
      ctrl.classList.add("rf-pressing");

      let timer = 0;
      const clear = () => {
        ctrl.classList.remove("rf-pressing");
        ctrl.removeEventListener("animationend", clear);
        if (timer) clearTimeout(timer);
      };
      ctrl.addEventListener("animationend", clear);
      timer = window.setTimeout(clear, 1200); // fallback (reduced-motion / interrupt)
    }

    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, []);
}
