import { Field } from "./ui";
import { ordinal } from "../lib/dates";
import type { PaySchedule } from "../lib/types";

/** "every 2 weeks", "twice a month", "monthly on the 1st". */
export function scheduleWord(s: PaySchedule): string {
  switch (s.kind) {
    case "weekly": return "weekly";
    case "biweekly": return "every 2 weeks";
    case "semimonthly": return `on the ${ordinal(s.days[0])} and ${s.days[1] === 31 ? "last day" : ordinal(s.days[1])}`;
    case "monthly": return `monthly on the ${ordinal(s.day)}`;
  }
}

/** The pay-schedule fields shared by Settings (the household) and a category paid on its own rhythm. */
export function PayScheduleFields({ value, onChange }: { value: PaySchedule; onChange: (v: PaySchedule) => void }) {
  const ps = value;
  return (
    <>
      <Field label="Schedule">
        <select className="field" value={ps.kind} onChange={(e) => {
          const kind = e.target.value as PaySchedule["kind"];
          const today = new Date().toISOString().slice(0, 10);
          onChange(kind === "semimonthly" ? { kind, days: [1, 15] } : kind === "monthly" ? { kind, day: 1 } : { kind, anchor: today });
        }}>
          <option value="semimonthly">Twice a month</option><option value="biweekly">Every two weeks</option><option value="weekly">Weekly</option><option value="monthly">Monthly</option>
        </select>
      </Field>
      {ps.kind === "semimonthly" && (
        <div className="grid grid-cols-2 gap-3">
          <Field label="First payday"><input className="field" type="number" min={1} max={31} value={ps.days[0]} onChange={(e) => onChange({ kind: "semimonthly", days: [Number(e.target.value), ps.days[1]] })} /></Field>
          <Field label="Second payday" hint="31 = last day of the month"><input className="field" type="number" min={1} max={31} value={ps.days[1]} onChange={(e) => onChange({ kind: "semimonthly", days: [ps.days[0], Number(e.target.value)] })} /></Field>
        </div>
      )}
      {ps.kind === "monthly" && <Field label="Day of month"><input className="field" type="number" min={1} max={31} value={ps.day} onChange={(e) => onChange({ kind: "monthly", day: Number(e.target.value) })} /></Field>}
      {(ps.kind === "weekly" || ps.kind === "biweekly") && <Field label="A recent payday" hint="Hearth counts forward from it."><input className="field" type="date" value={ps.anchor} onChange={(e) => onChange({ kind: ps.kind, anchor: e.target.value })} /></Field>}
    </>
  );
}
