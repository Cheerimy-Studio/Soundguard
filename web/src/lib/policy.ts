import type { Settings } from "../types";
export function withinWindow(start: string, end: string, minute: number): boolean {
  if (!start && !end) return true;
  const parse = (s: string) => /^\d{2}:\d{2}$/.test(s) ? Number(s.slice(0,2))*60+Number(s.slice(3)) : NaN;
  const a=parse(start), b=parse(end);
  if (!Number.isFinite(a) || !Number.isFinite(b) || a>=1440 || b>=1440) return false;
  return a===b || (a<b ? minute>=a && minute<b : minute>=a || minute<b);
}
export function policyState(settings: Settings, serverTime: string, offset=0) {
  const date = new Date(Date.parse(serverTime)+offset*1000);
  const minute=date.getUTCHours()*60+date.getUTCMinutes();
  return { capture: withinWindow(settings.captureStart,settings.captureEnd,minute),
    war: settings.warModeEnabled && withinWindow(settings.warModeStart,settings.warModeEnd,minute) };
}
