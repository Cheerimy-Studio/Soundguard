import { useCallback, useEffect, useMemo, useState } from "react";
import { graphic } from "echarts";
import type { EChartsOption } from "echarts";
import {
  Activity,
  AlertTriangle,
  CalendarDays,
  Database,
  Download,
  Gauge,
  LoaderCircle,
  RefreshCw,
  TrendingUp,
} from "lucide-react";
import { api } from "../api";
import { EChart } from "../components/EChart";
import { downloadBlob, formatDb, formatFullDateTime } from "../lib/format";
import type { ReadingsResponse } from "../types";

type RangeKey = "24h" | "7d" | "30d";

const RANGES: { key: RangeKey; label: string; hours: number; maxPoints: number }[] = [
  { key: "24h", label: "最近 24 小时", hours: 24, maxPoints: 900 },
  { key: "7d", label: "最近 7 天", hours: 24 * 7, maxPoints: 1600 },
  { key: "30d", label: "最近 30 天", hours: 24 * 30, maxPoints: 2200 },
];

interface HistoryPageProps {
  threshold: number;
  deviceId: string;
  onNotify: (message: string, tone?: "success" | "error" | "info") => void;
}

export function HistoryPage({ threshold, deviceId, onNotify }: HistoryPageProps) {
  const [range, setRange] = useState<RangeKey>("24h");
  const [data, setData] = useState<ReadingsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [exporting, setExporting] = useState(false);
  const [updatedAt, setUpdatedAt] = useState<Date | null>(null);

  const activeRange = RANGES.find((item) => item.key === range) ?? RANGES[0];

  const load = useCallback(
    async (key: RangeKey) => {
      const current = RANGES.find((item) => item.key === key) ?? RANGES[0];
      setLoading(true);
      setError("");
      try {
        const to = new Date();
        const from = new Date(to.getTime() - current.hours * 3600 * 1000);
        const result = await api.readings(from, to, deviceId, current.maxPoints);
        setData(result);
        setUpdatedAt(new Date());
      } catch (err) {
        setError(err instanceof Error ? err.message : "加载历史数据失败");
      } finally {
        setLoading(false);
      }
    },
    [deviceId],
  );

  useEffect(() => {
    void load(range);
  }, [load, range]);

  const chartOption = useMemo<EChartsOption>(() => {
    const readings = data?.readings ?? [];
    const avgPoints = readings.map((reading) => [
      new Date(reading.recordedAt).getTime(),
      reading.avgDb,
    ]);
    const peakPoints = readings.map((reading) => [
      new Date(reading.recordedAt).getTime(),
      reading.peakDb,
    ]);
    const maxValue = Math.max(
      threshold + 15,
      ...readings.map((reading) => reading.peakDb + 8),
      85,
    );
    return {
      animation: false,
      color: ["#0d7a82", "#b9822c"],
      legend: {
        top: 0,
        right: 4,
        itemWidth: 16,
        itemHeight: 3,
        textStyle: { color: "#617578", fontSize: 11 },
        data: ["30 秒均值", "瞬时峰值"],
      },
      grid: { left: 16, right: 24, top: 42, bottom: 66, containLabel: true },
      tooltip: {
        trigger: "axis",
        backgroundColor: "rgba(255, 255, 255, .98)",
        borderColor: "#dfe7e8",
        textStyle: { color: "#17292b", fontSize: 12 },
        axisPointer: { lineStyle: { color: "#c5d0d2", type: "dashed" } },
        formatter: (params: unknown) => {
          const list = params as Array<{ seriesName: string; value: [number, number]; color: string }>;
          if (!list.length) return "";
          const time = new Date(list[0].value[0]).toLocaleString("zh-CN", { hour12: false });
          const rows = list
            .map(
              (item) =>
                `<div style="display:flex;justify-content:space-between;gap:24px;margin-top:5px"><span style="color:#617578">${item.seriesName}</span><strong style="color:${item.color}">${Number(item.value[1]).toFixed(1)} dB</strong></div>`,
            )
            .join("");
          return `<div style="font-size:11px;color:#8b9b9d;margin-bottom:6px">${time}</div>${rows}`;
        },
      },
      xAxis: {
        type: "time",
        axisLine: { lineStyle: { color: "#dfe7e8" } },
        axisTick: { show: false },
        axisLabel: {
          color: "#8b9b9d",
          fontSize: 10,
          hideOverlap: true,
          formatter: (value: number) => {
            const date = new Date(value);
            if (activeRange.hours <= 24) {
              return date.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit", hour12: false });
            }
            return date.toLocaleDateString("zh-CN", { month: "2-digit", day: "2-digit" });
          },
        },
        splitLine: { show: false },
      },
      yAxis: {
        type: "value",
        min: 30,
        max: Math.ceil(maxValue / 10) * 10,
        splitNumber: 5,
        axisLine: { show: false },
        axisTick: { show: false },
        axisLabel: { color: "#8b9b9d", fontSize: 10, formatter: "{value}" },
        splitLine: { lineStyle: { color: "#edf1f2", type: "dashed" } },
      },
      dataZoom: [
        { type: "inside", throttle: 60 },
        {
          type: "slider",
          height: 18,
          bottom: 18,
          borderColor: "#dfe7e8",
          backgroundColor: "#f4f7f8",
          fillerColor: "rgba(14,124,134,.1)",
          handleStyle: { color: "#0d7a82", borderColor: "#096068" },
          textStyle: { color: "#8b9b9d", fontSize: 9 },
          dataBackground: {
            lineStyle: { color: "#c7d3d4" },
            areaStyle: { color: "rgba(14,124,134,.06)" },
          },
          selectedDataBackground: {
            lineStyle: { color: "#0d7a82" },
            areaStyle: { color: "rgba(14,124,134,.14)" },
          },
        },
      ],
      series: [
        {
          name: "30 秒均值",
          type: "line",
          smooth: 0.2,
          symbol: "none",
          lineStyle: { width: 2.1, color: "#0d7a82" },
          areaStyle: {
            color: new graphic.LinearGradient(0, 0, 0, 1, [
              { offset: 0, color: "rgba(13,122,130,.18)" },
              { offset: 1, color: "rgba(13,122,130,0)" },
            ]),
          },
          data: avgPoints,
          markLine: {
            silent: true,
            symbol: "none",
            lineStyle: { color: "#bd4438", type: "dashed", width: 1.2, opacity: 0.8 },
            label: {
              color: "#a13a30",
              fontSize: 10,
              formatter: `阈值 ${threshold.toFixed(0)} dB`,
              position: "insideMiddleTop",
            },
            data: [{ yAxis: threshold }],
          },
        },
        {
          name: "瞬时峰值",
          type: "line",
          smooth: 0.15,
          symbol: "none",
          lineStyle: { width: 1.3, color: "rgba(185,130,44,.8)", type: "dashed" },
          data: peakPoints,
        },
      ],
    };
  }, [activeRange.hours, data, threshold]);

  const stats = data?.stats;
  const readings = data?.readings ?? [];
  const tableRows = [...readings].reverse().slice(0, 24);

  async function handleExport() {
    if (exporting) return;
    setExporting(true);
    try {
      const to = new Date();
      const from = new Date(to.getTime() - activeRange.hours * 3600 * 1000);
      const blob = await api.exportReadings(from, to, deviceId);
      downloadBlob(
        blob,
        `soundguard-${range}-${to.toISOString().slice(0, 10)}.csv`,
      );
      onNotify("CSV 数据已导出", "success");
    } catch (err) {
      onNotify(err instanceof Error ? err.message : "导出失败", "error");
    } finally {
      setExporting(false);
    }
  }

  return (
    <div className="history-page">
      <section className="panel history-toolbar">
        <div className="range-tabs">
          <div className="range-tabs-label">
            <CalendarDays size={15} />
            统计区间
          </div>
          {RANGES.map((item) => (
            <button
              key={item.key}
              type="button"
              className={range === item.key ? "is-active" : ""}
              onClick={() => setRange(item.key)}
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="toolbar-actions">
          <button type="button" className="ghost-button small" onClick={() => void load(range)} disabled={loading}>
            {loading ? <LoaderCircle size={14} className="spin" /> : <RefreshCw size={14} />}
            刷新
          </button>
          <button type="button" className="primary-button small" onClick={() => void handleExport()} disabled={exporting}>
            {exporting ? <LoaderCircle size={14} className="spin" /> : <Download size={14} />}
            导出 CSV
          </button>
        </div>
      </section>

      <div className="history-stats">
        <div className="panel mini-stat">
          <div className="mini-stat-icon cyan"><Activity size={18} /></div>
          <div>
            <span>区间平均</span>
            <strong>{formatDb(stats?.avgDb)} <em>dB</em></strong>
          </div>
        </div>
        <div className="panel mini-stat">
          <div className="mini-stat-icon amber"><TrendingUp size={18} /></div>
          <div>
            <span>最高峰值</span>
            <strong>{formatDb(stats?.peakDb)} <em>dB</em></strong>
          </div>
        </div>
        <div className="panel mini-stat">
          <div className="mini-stat-icon rose"><AlertTriangle size={18} /></div>
          <div>
            <span>超限采样</span>
            <strong>{stats?.overCount ?? 0} <em>次</em></strong>
          </div>
        </div>
        <div className="panel mini-stat">
          <div className="mini-stat-icon emerald"><Database size={18} /></div>
          <div>
            <span>数据点</span>
            <strong>{stats?.sampleCount ?? 0} <em>个</em></strong>
          </div>
        </div>
      </div>

      <section className="panel history-chart-panel">
        <div className="panel-header">
          <div>
            <div className="eyebrow">历史声环境</div>
            <h3>{activeRange.label}分贝波动</h3>
          </div>
          <div className="history-meta">
            {updatedAt && <span>更新于 {updatedAt.toLocaleTimeString("zh-CN", { hour12: false })}</span>}
          </div>
        </div>
        {loading && !data ? (
          <div className="chart-loading">
            <LoaderCircle size={22} className="spin" />
            正在读取声学历史…
          </div>
        ) : error ? (
          <div className="chart-loading error">
            <AlertTriangle size={20} />
            <span>{error}</span>
            <button type="button" className="ghost-button small" onClick={() => void load(range)}>
              重试
            </button>
          </div>
        ) : (
          <EChart option={chartOption} className="history-chart" ariaLabel="历史分贝变化曲线" />
        )}
        <div className="chart-foot">
          <span>
            <CalendarDays size={13} />
            单次最多查询 90 天，超出范围请分段导出
          </span>
          <span>{readings.length} 个绘图点 · 每 30 秒聚合</span>
        </div>
      </section>

      <section className="panel data-table-panel">
        <div className="panel-header">
          <div>
            <div className="eyebrow">原始数据</div>
            <h3>最近采集记录</h3>
          </div>
          <span className="table-count">显示最新 {tableRows.length} 条</span>
        </div>
        <div className="data-table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th>采集时间</th>
                <th>30 秒均值</th>
                <th>峰值</th>
                <th>最低</th>
                <th>状态</th>
              </tr>
            </thead>
            <tbody>
              {tableRows.map((reading) => (
                <tr key={`${reading.recordedAt}-${reading.avgDb}`}>
                  <td>{formatFullDateTime(reading.recordedAt)}</td>
                  <td>
                    <span className="db-value" style={{ color: reading.overThreshold ? "#bd4438" : "#0d7a82" }}>
                      {formatDb(reading.avgDb)} dB
                    </span>
                  </td>
                  <td>{formatDb(reading.peakDb)} dB</td>
                  <td>{formatDb(reading.minDb)} dB</td>
                  <td>
                    <span className={`table-status ${reading.overThreshold ? "is-over" : "is-normal"}`}>
                      <span />
                      {reading.overThreshold ? "超限" : "正常"}
                    </span>
                  </td>
                </tr>
              ))}
              {tableRows.length === 0 && (
                <tr>
                  <td colSpan={5} className="table-empty">
                    当前区间暂无采集数据
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
