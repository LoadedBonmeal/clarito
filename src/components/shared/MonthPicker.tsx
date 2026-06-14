import { Ic } from "./Ic";

/**
 * MonthPicker — fiscal-period selector styled as the same calendar card/header as
 * the dashboard day-grid (`.cal-head`/`.cal-nav`/`.cal-title`), but with a 3×4 grid
 * of months instead of days. Used by the Salarizare and Declarații period popups so
 * every period popup looks like one family. Picks a whole month (a fiscal period).
 *
 * Render it where a `.pop` would go (inside a `position:relative` anchor); it renders
 * the `.pop.show` card itself and positions to the button's bottom-right.
 */
export function MonthPicker({
  year,
  month,
  monthsFull,
  prevYearLabel,
  nextYearLabel,
  onPrevYear,
  onNextYear,
  onPick,
}: {
  year: number;
  /** currently-selected month, 1–12 */
  month: number;
  /** 12 localized full month names (Ianuarie…Decembrie); cells show the first 3 chars */
  monthsFull: string[];
  prevYearLabel: string;
  nextYearLabel: string;
  onPrevYear: () => void;
  onNextYear: () => void;
  onPick: (month: number) => void;
}) {
  return (
    <div className="pop show cal-mpop" onMouseDown={(e) => e.stopPropagation()}>
      <div className="cal-head">
        <button className="cal-nav" aria-label={prevYearLabel} onClick={onPrevYear}>
          <Ic name="chevL" />
        </button>
        <div className="cal-title num">{year}</div>
        <button className="cal-nav" aria-label={nextYearLabel} onClick={onNextYear}>
          <Ic name="chevR" />
        </button>
      </div>
      <div className="cal-mgrid">
        {monthsFull.map((m, i) => (
          <button
            key={m}
            className={`cal-mon${month === i + 1 ? " sel" : ""}`}
            title={`${m} ${year}`}
            onClick={() => onPick(i + 1)}
          >
            {m.slice(0, 3)}
          </button>
        ))}
      </div>
    </div>
  );
}
