import * as echarts from "echarts/core";
import type { EChartsOption } from "echarts";
import { LineChart } from "echarts/charts";
import { GridComponent, LegendComponent, TooltipComponent, DataZoomComponent, MarkLineComponent } from "echarts/components";
import { LegacyGridContainLabel, LabelLayout } from "echarts/features";
import { CanvasRenderer } from "echarts/renderers";

echarts.use([LineChart, GridComponent, LegendComponent, TooltipComponent, DataZoomComponent, MarkLineComponent, CanvasRenderer, LegacyGridContainLabel, LabelLayout]);
import { useEffect, useRef } from "react";

interface ChartProps {
  option: EChartsOption;
  className?: string;
  ariaLabel?: string;
}

export function EChart({ option, className, ariaLabel }: ChartProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<echarts.ECharts | null>(null);

  useEffect(() => {
    if (!hostRef.current) return;
    const chart = echarts.init(hostRef.current, undefined, { renderer: "canvas" });
    chartRef.current = chart;
    const observer = new ResizeObserver(() => chart.resize());
    observer.observe(hostRef.current);
    return () => {
      observer.disconnect();
      chart.dispose();
      chartRef.current = null;
    };
  }, []);

  useEffect(() => {
    chartRef.current?.setOption(option, { notMerge: true });
  }, [option]);

  return (
    <div
      ref={hostRef}
      className={className}
      role="img"
      aria-label={ariaLabel ?? "数据图表"}
    />
  );
}
