import type { Settings } from "../types";
export function withinWindow(start: string, end: string, minute: number): boolean {
  if (!start && !end) return true;
  const parse = (s: string) => /^([01]\d|2[0-3]):[0-5]\d$/.test(s) ? Number(s.slice(0,2))*60+Number(s.slice(3)) : NaN;
  if ((start && !Number.isFinite(parse(start))) || (end && !Number.isFinite(parse(end)))) return false;
  const a=parse(start), b=parse(end);
  if ((Number.isFinite(a) && a>=1440) || (Number.isFinite(b) && b>=1440)) return false;
  if (Number.isFinite(a) && !Number.isFinite(b)) return minute>=a;
  if (!Number.isFinite(a) && Number.isFinite(b)) return minute<b;
  if (!Number.isFinite(a) && !Number.isFinite(b)) return false;
  return a===b || (a<b ? minute>=a && minute<b : minute>=a || minute<b);
}
export function policyState(settings: Settings, serverTime: string, offset=0) {
  const date = new Date(Date.parse(serverTime)+offset*1000);
  const minute=date.getUTCHours()*60+date.getUTCMinutes();
  return { capture: withinWindow(settings.captureStart,settings.captureEnd,minute),
    war: settings.warModeEnabled && withinWindow(settings.warModeStart,settings.warModeEnd,minute) };
}
