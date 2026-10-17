import * as echarts from "echarts";
import { useEffect, useRef } from "react";

interface ChartProps {
  option: echarts.EChartsOption;
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
