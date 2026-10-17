import { useMemo, useState } from "react";
import { graphic } from "echarts";
import type { EChartsOption } from "echarts";
import {
  ArrowRight,
  Check,
  ChevronRight,
  Download,
  Pause,
  Play,
  RefreshCw,
  Volume2,
  Wifi,
  WifiOff,
} from "lucide-react";
import type { Alert, Overview, Reading } from "../types";
import type { IntercomController } from "../hooks/useIntercom";
import { EChart } from "../components/EChart";
import { IntercomCard } from "../components/Intercom";
import {
  alertKindMeta,
  deviceStatusMeta,
  formatDb,
  formatRelative,
} from "../lib/format";

interface OverviewPageProps {
  overview: Overview;
  alerts: Alert[];
  liveReadings: Reading[];
  intercom: IntercomController;
  signalingConnected: boolean;
  onControl: (monitoring: boolean) => Promise<void>;
  onAcknowledge: (id: string) => Promise<void>;
  onNavigate: (page: "overview" | "history" | "alerts" | "devices" | "settings") => void;
  onExport: () => void;
}

function toneClass(tone: string): string {
  if (tone === "online") return "is-online";
  if (tone === "paused" || tone === "warning" || tone === "waiting") return "is-warn";
  if (tone === "error" || tone === "offline") return "is-danger";
  return "";
}

export function OverviewPage({
  overview,
  alerts,
  liveReadings,
  intercom,
  signalingConnected,
  onControl,
  onAcknowledge,
  onNavigate,
  onExport,
}: OverviewPageProps) {
  const [busy, setBusy] = useState(false);
  const device = overview.device;
  const settings = overview.settings;
  const stats = overview.stats;
  const status = deviceStatusMeta(device.status);
  const threshold = settings.thresholdDb;
  const ignoringSystem = device.liveSystemPlaying && !device.liveAecEnabled;
  const current = device.liveSuspended || ignoringSystem ? null : device.currentDb;
  const average = device.liveSuspended || ignoringSystem
    ? null
    : device.liveAvgDb ?? device.currentDb;
  const peak = device.liveSuspended || ignoringSystem ? null : device.peakDb;
  const over = average !== null && average >= threshold;
  const selectedSignaling = signalingConnected;

  const chartReadings = useMemo(() => {
    const cutoff = Date.now() - 120_000;
    return liveReadings.filter(
      (reading) => new Date(reading.recordedAt).getTime() >= cutoff,
    );
  }, [liveReadings]);

  const chartOption = useMemo<EChartsOption>(() => {
    const points = chartReadings.map((reading) => [
      new Date(reading.recordedAt).getTime(),
      reading.avgDb,
    ]);
    const averageLine =
      average === null
        ? []
        : chartReadings.map((reading) => [
            new Date(reading.recordedAt).getTime(),
            average,
          ]);
    const maxValue = Math.max(
      threshold + 15,
      ...chartReadings.map((reading) => reading.peakDb + 8),
      85,
    );
    return {
      animation: false,
      grid: { left: 12, right: 18, top: 34, bottom: 24, containLabel: true },
      legend: {
        top: 0,
        right: 4,
        itemWidth: 14,
        itemHeight: 3,
        textStyle: { color: "#758399", fontSize: 11 },
        data: ["瞬时", "平均", "阈值"],
      },
      tooltip: {
        trigger: "axis",
        backgroundColor: "#ffffff",
        borderColor: "#e5e9ef",
        borderWidth: 1,
        padding: [9, 11],
        textStyle: { color: "#172033", fontSize: 12 },
        axisPointer: { lineStyle: { color: "#cbd5e1", type: "dashed" } },
        formatter: (params: unknown) => {
          const list = params as Array<{
            seriesName: string;
            value: [number, number];
            color: string;
          }>;
          if (!list.length) return "";
          const time = new Date(list[0].value[0]).toLocaleTimeString("zh-CN", {
            hour12: false,
          });
          const rows = list
            .filter((item) => item.seriesName !== "阈值")
            .map(
              (item) =>
                `<div style="display:flex;justify-content:space-between;gap:22px;margin-top:4px"><span style="color:#758399">${item.seriesName}</span><strong>${Number(item.value[1]).toFixed(1)} dB</strong></div>`,
            )
            .join("");
          return `<div style="font-size:11px;color:#9aa7b7;margin-bottom:4px">${time}</div>${rows}`;
        },
      },
      xAxis: {
        type: "time",
        axisLine: { lineStyle: { color: "#e5e9ef" } },
        axisTick: { show: false },
        axisLabel: {
          color: "#9aa7b7",
          fontSize: 10,
          hideOverlap: true,
          formatter: (value: number) =>
            new Date(value).toLocaleTimeString("zh-CN", {
              hour: "2-digit",
              minute: "2-digit",
              second: "2-digit",
              hour12: false,
            }),
        },
        splitLine: { show: false },
      },
      yAxis: {
        type: "value",
        min: 30,
        max: Math.ceil(maxValue / 10) * 10,
        splitNumber: 4,
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { color: "#9aa7b7", fontSize: 10 },
        splitLine: { lineStyle: { color: "#eef1f5" } },
      },
      series: [
        {
          name: "瞬时",
          type: "line",
          smooth: 0.25,
          symbol: "none",
          lineStyle: { width: 2, color: "#0d7a82" },
          areaStyle: {
            color: new graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(13,122,130,.16)" },
              { offset: 1, color: "rgba(13,122,130,0)" },
            ]),
          },
          data: points,
        },
        {
          name: "平均",
          type: "line",
          symbol: "none",
          lineStyle: { width: 1.4, type: "dashed", color: "#2563eb" },
          data: averageLine,
        },
        {
          name: "阈值",
          type: "line",
          symbol: "none",
          silent: true,
          lineStyle: { width: 1, type: "dashed", color: "#d97706" },
          data: chartReadings.map((reading) => [
            new Date(reading.recordedAt).getTime(),
            threshold,
          ]),
        },
      ],
    };
  }, [average, chartReadings, threshold]);

  const recentAlerts = alerts
    .filter(
      (alert) =>
        !alert.acknowledged &&
        ["noise", "offline", "tamper"].includes(alert.kind),
    )
    .slice(0, 4);

  async function toggleMonitoring() {
    if (busy) return;
    setBusy(true);
    try {
      await onControl(!device.desiredMonitoring);
    } catch {
      // Parent displays the error.
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dashboard-page">
      <section className="dash-card dash-live">
        <div className="dash-head">
          <div>
            <h1>{device.name}</h1>
            <span className={`dash-state ${toneClass(status.tone)}`}>
              <span className="dot" />
              {device.liveSuspended ? "通话中" : status.label}
            </span>
          </div>
          <button type="button" className="light-button" onClick={onExport}>
            <Download size={14} />
            导出 24h
          </button>
        </div>

        <div className="dash-db-row">
          <div className={`dash-db ${over ? "is-over" : ""}`}>
            <strong>{formatDb(current)}</strong>
            <span>dB(A)</span>
          </div>
          <div className="dash-side-meta">
            <span>平均 <b>{formatDb(average)} dB</b></span>
            <span>峰值 <b>{formatDb(peak)} dB</b></span>
            <span>阈值 <b>{threshold.toFixed(1)} dB</b></span>
          </div>
        </div>

        <div className="dash-level">
          <div
            className={`dash-level-fill ${over ? "is-over" : ""}`}
            style={{
              width: `${
                current === null
                  ? 0
                  : Math.max(0, Math.min(100, ((current - 30) / 80) * 100))
              }%`,
            }}
          />
          <span
            className="dash-level-threshold"
            style={{ left: `${Math.max(0, Math.min(100, ((threshold - 30) / 80) * 100))}%` }}
          />
        </div>
        <div className="dash-scale"><span>30</span><span>50</span><span>70</span><span>90</span><span>110</span></div>

        {device.liveSuspended ? (
          <div className="dash-notice info">
            <span>通话进行中，分贝检测已暂停</span>
            <span>结束后自动恢复</span>
          </div>
        ) : over && current !== null ? (
          <div className="dash-notice danger">
            <Volume2 size={16} />
            <span>平均 {formatDb(average)} dB，阈值 {threshold.toFixed(1)} dB，请保持安静</span>
          </div>
        ) : device.liveSystemPlaying ? (
          <div className="dash-notice info">
            <span>
              检测到电脑播放声音
              {device.liveAecEnabled ? "，已通过回声消除抑制" : "，已自动忽略"}
            </span>
          </div>
        ) : null}

        <div className="dash-metrics">
          <div><span>平均</span><strong>{formatDb(average)}<em> dB</em></strong></div>
          <div><span>峰值</span><strong>{formatDb(peak)}<em> dB</em></strong></div>
          <div><span>阈值</span><strong>{threshold.toFixed(1)}<em> dB</em></strong></div>
          <div><span>今日告警</span><strong>{overview.alertCountToday}<em> 条</em></strong></div>
        </div>

        <div className="dash-chart-box">
          <div className="dash-chart-head">
            <div>
              <h2>声压波形</h2>
              <span>每秒同步客户端数据</span>
            </div>
          </div>
          <EChart
            option={chartOption}
            className="dash-chart"
            ariaLabel="实时分贝波形"
          />
        </div>

        <div className="dash-actions">
          <button
            type="button"
            className={device.desiredMonitoring ? "light-button" : "teal-button"}
            onClick={toggleMonitoring}
            disabled={busy}
          >
            {device.desiredMonitoring ? <Pause size={15} /> : <Play size={15} />}
            {busy ? "正在下发" : device.desiredMonitoring ? "暂停采集" : "恢复采集"}
          </button>
          <button type="button" className="light-button" onClick={() => onNavigate("devices")}>
            <RefreshCw size={14} />
            设备管理
          </button>
          <button type="button" className="light-button" onClick={() => onNavigate("history")}>
            查看历史数据
            <ArrowRight size={14} />
          </button>
        </div>
      </section>

      <aside className="dash-side">
        <section className="dash-card">
          <div className="card-heading">
            <h2>24 小时统计</h2>
          </div>
          <div className="dash-stats-grid">
            <div><span>平均</span><strong>{formatDb(stats.avgDb)}<em> dB</em></strong></div>
            <div><span>峰值</span><strong>{formatDb(stats.peakDb)}<em> dB</em></strong></div>
            <div><span>超限</span><strong>{stats.overCount}<em> 次</em></strong></div>
            <div><span>样本</span><strong>{stats.sampleCount}<em> 个</em></strong></div>
          </div>
        </section>

        <IntercomCard
          controller={intercom}
          device={device}
          signalingConnected={selectedSignaling}
        />

        <section className="dash-card">
          <div className="card-heading">
            <h2>待处理告警</h2>
            <button type="button" className="text-link" onClick={() => onNavigate("alerts")}>
              全部
              <ChevronRight size={13} />
            </button>
          </div>
          {recentAlerts.length === 0 ? (
            <div className="dash-empty">
              <Check size={16} />
              当前没有待处理告警
            </div>
          ) : (
            <div className="dash-alert-list">
              {recentAlerts.map((alert) => {
                const kind = alertKindMeta(alert.kind);
                return (
                  <div className="dash-alert-row" key={alert.id}>
                    <span className={`mini-dot ${alert.severity}`} />
                    <div>
                      <strong>{alert.title}</strong>
                      <span>
                        {kind.short} · {formatRelative(alert.createdAt)}
                      </span>
                    </div>
                    {alert.db !== null && <em>{formatDb(alert.db)} dB</em>}
                    <button
                      type="button"
                      title="标记已处理"
                      onClick={() => void onAcknowledge(alert.id)}
                    >
                      <Check size={14} />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="dash-card dash-connection">
          <div className="card-heading">
            <h2>设备状态</h2>
            <span className={`tiny-status ${device.online ? "is-online" : "is-offline"}`}>
              {device.online ? <Wifi size={12} /> : <WifiOff size={12} />}
              {device.online ? "在线" : "离线"}
            </span>
          </div>
          <div className="dash-facts">
            <div><span>最后心跳</span><b>{device.lastSeen ? formatRelative(device.lastSeen) : "--"}</b></div>
            <div><span>最后上报</span><b>{device.lastReadingAt ? formatRelative(device.lastReadingAt) : "--"}</b></div>
            <div><span>客户端</span><b>{device.appVersion ?? "等待上报"}</b></div>
          </div>
        </section>
      </aside>
    </div>
  );
}
