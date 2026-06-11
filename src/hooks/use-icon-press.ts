import { useEffect } from "react";

/**
 * App-wide one-shot icon press animation, driven by the Web Animations API.
 *
 * Why WAAPI and not a CSS `.rf-pressing` class: toggling a class on the pressed
 * control was wiped by React reconciliation — a sidebar nav item gaining
 * `.active` on navigation, or the SPV pill gaining `.is-syncing`, re-renders and
 * resets the element's className to its JSX value, removing the class mid-press,
 * so the animation only showed on the *second* press. `element.animate()` runs
 * on the element's own animation timeline, independent of className/attribute
 * reconciliation, so it plays reliably on the first press.
 *
 * Each icon plays a gesture that fits its meaning; everything else gets a soft
 * pop. Sidebar nav + the ANAF·SPV pill animate more slowly (calmer). Disabled
 * under prefers-reduced-motion. Transform-only (GPU).
 */

type Gesture = { kf: Keyframe[]; dur: number; easing: string; origin?: string };

const EASE = "cubic-bezier(.23, 1, .32, 1)";
const SPRING = "cubic-bezier(.34, 1.56, .64, 1)";

const SPIN: Keyframe[] = [{ transform: "rotate(0)" }, { transform: "rotate(360deg)" }];
const SPIN_CCW: Keyframe[] = [{ transform: "rotate(0)" }, { transform: "rotate(-360deg)" }];
const POP: Keyframe[] = [{ transform: "scale(1)" }, { transform: "scale(.8)", offset: 0.35 }, { transform: "scale(1)" }];
const SWING: Keyframe[] = [
  { transform: "rotate(0)" }, { transform: "rotate(14deg)", offset: 0.2 },
  { transform: "rotate(-10deg)", offset: 0.4 }, { transform: "rotate(6deg)", offset: 0.6 },
  { transform: "rotate(-3deg)", offset: 0.8 }, { transform: "rotate(0)" },
];
const DOWN: Keyframe[] = [{ transform: "translateY(0)" }, { transform: "translateY(3px)", offset: 0.45 }, { transform: "translateY(0)" }];
const UP: Keyframe[] = [{ transform: "translateY(0)" }, { transform: "translateY(-3px)", offset: 0.45 }, { transform: "translateY(0)" }];
const FLY: Keyframe[] = [{ transform: "translate(0,0)" }, { transform: "translate(3px,-3px)", offset: 0.45 }, { transform: "translate(0,0)" }];
const WIGGLE: Keyframe[] = [
  { transform: "rotate(0)" }, { transform: "rotate(-9deg)", offset: 0.25 },
  { transform: "rotate(7deg)", offset: 0.5 }, { transform: "rotate(-4deg)", offset: 0.75 }, { transform: "rotate(0)" },
];
const NUDGE_R: Keyframe[] = [{ transform: "translateX(0)" }, { transform: "translateX(3px)", offset: 0.45 }, { transform: "translateX(0)" }];
const NUDGE_L: Keyframe[] = [{ transform: "translateX(0)" }, { transform: "translateX(-3px)", offset: 0.45 }, { transform: "translateX(0)" }];
const ROTATE: Keyframe[] = [{ transform: "rotate(0) scale(1)" }, { transform: "rotate(90deg) scale(.85)", offset: 0.4 }, { transform: "rotate(0) scale(1)" }];
const BLINK: Keyframe[] = [{ transform: "scaleY(1)" }, { transform: "scaleY(.12)", offset: 0.45 }, { transform: "scaleY(1)" }];
const PULSE: Keyframe[] = [{ transform: "scale(1)" }, { transform: "scale(1.18)", offset: 0.4 }, { transform: "scale(1)" }];
const TICK: Keyframe[] = [{ transform: "rotate(0)" }, { transform: "rotate(-18deg)", offset: 0.3 }, { transform: "rotate(10deg)", offset: 0.65 }, { transform: "rotate(0)" }];
const TWINKLE: Keyframe[] = [{ transform: "scale(1) rotate(0)" }, { transform: "scale(1.15) rotate(18deg)", offset: 0.4 }, { transform: "scale(1) rotate(0)" }];
const CHECK: Keyframe[] = [{ transform: "scale(1)" }, { transform: "scale(.7)", offset: 0.3 }, { transform: "scale(1)" }];

const DEFAULT: Gesture = { kf: POP, dur: 550, easing: EASE };

const GESTURES: Record<string, Gesture> = {
  // Spin — rotation / process / gears / rays / planet
  refresh: { kf: SPIN, dur: 900, easing: EASE }, repeat: { kf: SPIN, dur: 900, easing: EASE },
  settings: { kf: SPIN, dur: 900, easing: EASE }, sun: { kf: SPIN, dur: 900, easing: EASE },
  globe: { kf: SPIN, dur: 900, easing: EASE },
  // Spin CCW — undo / reversal
  reverse: { kf: SPIN_CCW, dur: 900, easing: EASE }, storno: { kf: SPIN_CCW, dur: 900, easing: EASE },
  // Swing — hangs / rings
  bell: { kf: SWING, dur: 850, easing: "ease", origin: "50% 18%" },
  anaf: { kf: SWING, dur: 850, easing: "ease", origin: "50% 18%" },
  tag: { kf: SWING, dur: 850, easing: "ease", origin: "50% 18%" },
  bookmark: { kf: SWING, dur: 850, easing: "ease", origin: "50% 18%" },
  // Bounce down — incoming / saved
  download: { kf: DOWN, dur: 700, easing: SPRING }, cloudDn: { kf: DOWN, dur: 700, easing: SPRING },
  save: { kf: DOWN, dur: 700, easing: SPRING }, archive: { kf: DOWN, dur: 700, easing: SPRING },
  fileIn: { kf: DOWN, dur: 700, easing: SPRING }, invoiceIn: { kf: DOWN, dur: 700, easing: SPRING },
  // Bounce up — outgoing
  upload: { kf: UP, dur: 700, easing: SPRING }, cloudUp: { kf: UP, dur: 700, easing: SPRING },
  fileOut: { kf: UP, dur: 700, easing: SPRING },
  // Fly — send / open elsewhere
  send: { kf: FLY, dur: 700, easing: SPRING }, external: { kf: FLY, dur: 700, easing: SPRING },
  link: { kf: FLY, dur: 700, easing: SPRING },
  // Wiggle — write / shake / attention / balance
  pen: { kf: WIGGLE, dur: 600, easing: "ease" }, edit: { kf: WIGGLE, dur: 600, easing: "ease" },
  trash: { kf: WIGGLE, dur: 600, easing: "ease" }, filter: { kf: WIGGLE, dur: 600, easing: "ease" },
  scale: { kf: WIGGLE, dur: 600, easing: "ease" }, alert: { kf: WIGGLE, dur: 600, easing: "ease" },
  alertTriangle: { kf: WIGGLE, dur: 600, easing: "ease" }, warning: { kf: WIGGLE, dur: 600, easing: "ease" },
  // Directional nudge
  chevRight: { kf: NUDGE_R, dur: 550, easing: "ease" }, chevronRight: { kf: NUDGE_R, dur: 550, easing: "ease" },
  arrowRight: { kf: NUDGE_R, dur: 550, easing: "ease" },
  chevLeft: { kf: NUDGE_L, dur: 550, easing: "ease" }, chevronLeft: { kf: NUDGE_L, dur: 550, easing: "ease" },
  arrowLeft: { kf: NUDGE_L, dur: 550, easing: "ease" },
  chevDown: { kf: DOWN, dur: 550, easing: "ease" }, chevronDown: { kf: DOWN, dur: 550, easing: "ease" },
  caret: { kf: DOWN, dur: 550, easing: "ease" }, arrowDown: { kf: DOWN, dur: 550, easing: "ease" },
  sortDsc: { kf: DOWN, dur: 550, easing: "ease" },
  arrowUp: { kf: UP, dur: 550, easing: "ease" }, sortAsc: { kf: UP, dur: 550, easing: "ease" },
  // Rotate-pop — add / close
  plus: { kf: ROTATE, dur: 600, easing: EASE }, x: { kf: ROTATE, dur: 600, easing: EASE },
  xCircle: { kf: ROTATE, dur: 600, easing: EASE }, cancel: { kf: ROTATE, dur: 600, easing: EASE },
  command: { kf: ROTATE, dur: 600, easing: EASE },
  // Blink
  eye: { kf: BLINK, dur: 500, easing: "ease" }, view: { kf: BLINK, dur: 500, easing: "ease" },
  // Pulse
  search: { kf: PULSE, dur: 550, easing: SPRING }, shield: { kf: PULSE, dur: 550, easing: SPRING },
  info: { kf: PULSE, dur: 550, easing: SPRING }, help: { kf: PULSE, dur: 550, easing: SPRING },
  dot: { kf: PULSE, dur: 550, easing: SPRING },
  // Tick / turn
  clock: { kf: TICK, dur: 500, easing: "ease" }, history: { kf: TICK, dur: 500, easing: "ease" },
  key: { kf: TICK, dur: 500, easing: "ease" },
  // Twinkle
  star: { kf: TWINKLE, dur: 550, easing: SPRING },
  // Check-pop
  check: { kf: CHECK, dur: 600, easing: SPRING }, checkCircle: { kf: CHECK, dur: 600, easing: SPRING },
  // ── design (Ic) names ──
  sync: { kf: SPIN, dur: 900, easing: EASE }, loop: { kf: SPIN, dur: 900, easing: EASE },
  cog: { kf: SPIN, dur: 900, easing: EASE },
  undo: { kf: SPIN_CCW, dur: 900, easing: EASE },
  dl: { kf: DOWN, dur: 700, easing: SPRING }, docDown: { kf: DOWN, dur: 700, easing: SPRING },
  docUp: { kf: UP, dur: 700, easing: SPRING },
  funnel: { kf: WIGGLE, dur: 600, easing: "ease" }, wrench: { kf: WIGGLE, dur: 600, easing: "ease" },
  chevR: { kf: NUDGE_R, dur: 550, easing: "ease" }, chevD: { kf: DOWN, dur: 550, easing: "ease" },
  chevUD: { kf: DOWN, dur: 550, easing: "ease" },
  xMark: { kf: ROTATE, dur: 600, easing: EASE },
  lens: { kf: PULSE, dur: 550, easing: SPRING },
  checkC: { kf: CHECK, dur: 600, easing: SPRING },
  calendar: { kf: DOWN, dur: 550, easing: "ease" }, columns: { kf: DOWN, dur: 550, easing: "ease" },
  printer: { kf: DOWN, dur: 700, easing: SPRING }, copy: { kf: FLY, dur: 700, easing: SPRING },
  truck: { kf: NUDGE_R, dur: 700, easing: SPRING }, mail: { kf: SWING, dur: 850, easing: "ease", origin: "50% 18%" },
  team: { kf: PULSE, dur: 550, easing: SPRING }, lang: { kf: PULSE, dur: 550, easing: SPRING },
  exit: { kf: NUDGE_R, dur: 550, easing: "ease" }, code: { kf: PULSE, dur: 550, easing: SPRING },
};

// Calmer, slower durations for the sidebar nav (by gesture group).
const SIDEBAR_SPIN = new Set(["repeat", "storno", "loop", "undo", "sync"]);
const SIDEBAR_SWING = new Set(["anaf", "bell", "tag", "bookmark", "mail"]);

const PRESSABLE = 'button, a[href], [role="button"]';

export function useIconPressAnimation() {
  useEffect(() => {
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)");

    function onPointerDown(e: PointerEvent) {
      if (!e.isPrimary || e.button !== 0) return;
      if (reduce?.matches) return;
      const ctrl = (e.target as Element | null)?.closest?.(PRESSABLE) as HTMLElement | null;
      if (!ctrl || (ctrl as HTMLButtonElement).disabled) return;
      const svg = ctrl.querySelector<SVGElement>("svg[data-ic], svg[data-icon]");
      if (!svg || typeof svg.animate !== "function") return;

      const name = svg.getAttribute("data-ic") ?? svg.getAttribute("data-icon") ?? "";
      const g = GESTURES[name] ?? DEFAULT;

      // Sidebar nav + the ANAF·SPV pill animate more slowly (calmer).
      let dur = g.dur;
      if (ctrl.closest(".sidebar")) {
        dur = SIDEBAR_SPIN.has(name) ? 1400 : SIDEBAR_SWING.has(name) ? 1300 : 900;
      }
      if (ctrl.classList.contains("spv-sync") || ctrl.closest(".spv-sync")) {
        dur = 1500;
      }

      const origin = g.origin ?? "50% 50%";
      const frames = g.kf.map((k) => ({ ...k, transformOrigin: origin }));
      svg.animate(frames, { duration: dur, easing: g.easing, fill: "none" });
    }

    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, []);
}
