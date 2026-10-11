import { useEffect, useState } from "react";
import { Card, CardHead, ErrorNote, Field, MoneyInput, PageHeader, Spinner, Toggle } from "../../components/ui";
import { tool } from "../../lib/api";
import { useSettings } from "../../lib/data";
import { useMutation } from "../../lib/query";
import { useSession } from "../../lib/session";
import type { Settings } from "../../lib/types";

export function SettingsScreen() {
  const { me } = useSession();
  const owner = me?.role === "owner";
  const settings = useSettings();
  const [s, setS] = useState<Settings | null>(null);
  useEffect(() => { if (settings.data) setS(settings.data); }, [settings.data]);
  const save = useMutation((key: string, value: unknown) => tool("household.update_settings", { key, value }), ["household.", "balance.", "budget."]);
  if (!s) return <Spinner />;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]) => { setS({ ...s, [key]: value }); void save.run(key as string, value); };
  const ps = s.pay_schedule;
  return (
    <>
      <PageHeader title="Settings" back="/more" />
      {!owner && <ErrorNote error="Only the owner can change settings." />}
      <fieldset disabled={!owner} className="contents">
      <Card className="divider overflow-hidden">
        <CardHead title="Paydays" />
        <div className="flex flex-col gap-3 p-4">
          <Field label="Schedule">
            <select className="field" value={ps.kind} onChange={(e) => {
              const kind = e.target.value as Settings["pay_schedule"]["kind"];
              const today = new Date().toISOString().slice(0, 10);
              set("pay_schedule", kind === "semimonthly" ? { kind, days: [1, 15] } : kind === "monthly" ? { kind, day: 1 } : { kind, anchor: today });
            }}>
              <option value="semimonthly">Twice a month</option><option value="biweekly">Every two weeks</option><option value="weekly">Weekly</option><option value="monthly">Monthly</option>
            </select>
          </Field>
          {ps.kind === "semimonthly" && (
            <div className="grid grid-cols-2 gap-3">
              <Field label="First payday"><input className="field" type="number" min={1} max={31} value={ps.days[0]} onChange={(e) => set("pay_schedule", { kind: "semimonthly", days: [Number(e.target.value), ps.days[1]] })} /></Field>
              <Field label="Second payday" hint="31 = last day of the month"><input className="field" type="number" min={1} max={31} value={ps.days[1]} onChange={(e) => set("pay_schedule", { kind: "semimonthly", days: [ps.days[0], Number(e.target.value)] })} /></Field>
            </div>
          )}
          {ps.kind === "monthly" && <Field label="Day of month"><input className="field" type="number" min={1} max={31} value={ps.day} onChange={(e) => set("pay_schedule", { kind: "monthly", day: Number(e.target.value) })} /></Field>}
          {(ps.kind === "weekly" || ps.kind === "biweekly") && <Field label="A recent payday" hint="Hearth counts forward from it."><input className="field" type="date" value={ps.anchor} onChange={(e) => set("pay_schedule", { kind: ps.kind, anchor: e.target.value })} /></Field>}
        </div>
      </Card>

      <Card className="divider overflow-hidden">
        <CardHead title="Cash" />
        <div className="flex flex-col gap-3 p-4">
          <Field label="Cash floor" hint="The low point should never go under this."><MoneyInput value={s.cash_floor_cents} onChange={(c) => c != null && set("cash_floor_cents", c)} /></Field>
          <Field label="Large charge threshold" hint="Unplanned charges above this raise an alert."><MoneyInput value={s.large_charge_threshold_cents} onChange={(c) => c != null && set("large_charge_threshold_cents", c)} /></Field>
          <Field label="Balance is stale after (days)"><input className="field" type="number" min={1} value={s.stale_balance_days} onChange={(e) => set("stale_balance_days", Number(e.target.value))} /></Field>
        </div>
      </Card>

      <Card className="divider overflow-hidden">
        <CardHead title="Pace alerts" />
        <div className="grid grid-cols-2 gap-3 p-4">
          <Field label="Flag at % of plan"><input className="field" type="number" min={100} value={s.pace_alert.ratio_pct} onChange={(e) => set("pace_alert", { ...s.pace_alert, ratio_pct: Number(e.target.value) })} /></Field>
          <Field label="Not before day"><input className="field" type="number" min={1} max={28} value={s.pace_alert.from_day} onChange={(e) => set("pace_alert", { ...s.pace_alert, from_day: Number(e.target.value) })} /></Field>
        </div>
      </Card>

      <Card className="divider overflow-hidden">
        <CardHead title="Quiet hours & nagging" />
        <div className="flex flex-col gap-3 p-4">
          <div className="grid grid-cols-2 gap-3">
            <Field label="Quiet from"><input className="field" type="time" value={s.notifications.quiet_hours.start} onChange={(e) => set("notifications", { ...s.notifications, quiet_hours: { ...s.notifications.quiet_hours, start: e.target.value } })} /></Field>
            <Field label="Until"><input className="field" type="time" value={s.notifications.quiet_hours.end} onChange={(e) => set("notifications", { ...s.notifications, quiet_hours: { ...s.notifications.quiet_hours, end: e.target.value } })} /></Field>
          </div>
          <Toggle label="Critical alerts break quiet hours" checked={s.notifications.quiet_hours_override_critical} onChange={(v) => set("notifications", { ...s.notifications, quiet_hours_override_critical: v })} />
          <div className="grid grid-cols-3 gap-3">
            <Field label="First nudge (min)"><input className="field" type="number" min={1} value={s.nag.first_after_minutes} onChange={(e) => set("nag", { ...s.nag, first_after_minutes: Number(e.target.value) })} /></Field>
            <Field label="Then every (min)"><input className="field" type="number" min={1} value={s.nag.every_minutes} onChange={(e) => set("nag", { ...s.nag, every_minutes: Number(e.target.value) })} /></Field>
            <Field label="At most"><input className="field" type="number" min={0} value={s.nag.max_nudges} onChange={(e) => set("nag", { ...s.nag, max_nudges: Number(e.target.value) })} /></Field>
          </div>
        </div>
      </Card>
      </fieldset>
      <ErrorNote error={save.error} onClose={save.clearError} />
      <p className="text-faint text-center text-[11px] font-semibold">Changes save as you make them.</p>
    </>
  );
}
