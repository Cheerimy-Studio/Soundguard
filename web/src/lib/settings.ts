import type { Device, Settings } from "../types";

export function effectiveSettings(base: Settings, device: Device): Settings {
  const overrides = device.settings;
  return {
    ...base,
    thresholdDb: overrides?.thresholdDb ?? base.thresholdDb,
    alertCooldownSecs: overrides?.alertCooldownSecs ?? base.alertCooldownSecs,
    offlineTimeoutSecs: overrides?.offlineTimeoutSecs ?? base.offlineTimeoutSecs,
    autoRecordEvidence: overrides?.autoRecordEvidence ?? base.autoRecordEvidence,
  };
}
