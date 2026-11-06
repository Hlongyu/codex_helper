import { useEffect, useId, useRef, useState } from "react";
import type { RouteUsageBucket, TrendMetric } from "../types";
import { Button, EmptyState, Tooltip } from "./ui";
import { buildTrendGeometry, formatTrendXAxisLabel } from "./trendChart";

export function UsageTrendChart({
  buckets,
  compact = false,
  granularity,
  metric,
}: {
  buckets: RouteUsageBucket[];
  compact?: boolean;
  granularity: string;
  metric: TrendMetric;
}) {
  const container = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const gradientId = `trend-${useId().replace(/:/g, "")}`;
  const height = compact ? 176 : 220;
  const values = buckets.map((bucket) =>
    metric === "tokens" ? bucket.total_tokens : bucket.request_count,
  );
  const { left, right, top, bottom, points, ticks, xTickIndexes } =
    buildTrendGeometry(values, width, height);
  const activePoint = activeIndex === null ? undefined : points[activeIndex];
  const metricLabel = metric === "tokens" ? "总 Token" : "成功请求";
  const fullNumber = (value: number) =>
    Math.round(value).toLocaleString("zh-CN");
  const line = points.map((point) => `${point.x},${point.y}`).join(" ");
  const area =
    points.length > 1
      ? `${points[0].x},${bottom} ${line} ${points[points.length - 1].x},${bottom}`
      : "";

  useEffect(() => {
    const element = container.current;
    if (!element) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => setActiveIndex(null), [metric, granularity]);

  return (
    <div
      ref={container}
      className="trend-line-chart"
      style={{ height }}
      data-sparse={points.length <= 2}
      onPointerLeave={() => setActiveIndex(null)}
    >
      {values.every((value) => value === 0) ? (
        <EmptyState
          title="暂无趋势数据"
          description="当前时间范围内的调用会汇总显示在这里。"
        />
      ) : (
        <>
          <svg
            className="trend-svg"
            viewBox={`0 0 ${width} ${height}`}
            role="group"
            aria-label={`${metricLabel}趋势`}
          >
            <defs>
              <linearGradient id={gradientId} x1="0" x2="0" y1="0" y2="1">
                <stop
                  offset="0%"
                  stopColor="var(--chart-primary)"
                  stopOpacity="0.14"
                />
                <stop
                  offset="100%"
                  stopColor="var(--chart-primary)"
                  stopOpacity="0.015"
                />
              </linearGradient>
            </defs>
            {ticks.map((tick) => (
              <g key={tick.ratio}>
                <line
                  className={
                    tick.ratio === 0 ? "trend-baseline" : "trend-grid-line"
                  }
                  x1={left}
                  x2={right}
                  y1={tick.y}
                  y2={tick.y}
                />
                <text
                  className="trend-y-tick"
                  x={left - 12}
                  y={tick.y}
                  textAnchor="end"
                  dominantBaseline="middle"
                >
                  {tick.label}
                </text>
              </g>
            ))}
            {area && (
              <polygon
                className="trend-area"
                fill={`url(#${gradientId})`}
                points={area}
              />
            )}
            {points.length > 1 && (
              <polyline className="trend-line" points={line} />
            )}
            {activePoint && (
              <line
                className="trend-crosshair"
                x1={activePoint.x}
                x2={activePoint.x}
                y1={top}
                y2={bottom}
              />
            )}
            {points.map((point, index) => (
              <circle
                key={buckets[index].label}
                className={`trend-marker${activeIndex === index ? " active" : ""}`}
                cx={point.x}
                cy={point.y}
                r={activeIndex === index ? 4 : 3}
                aria-hidden="true"
              />
            ))}
            {xTickIndexes.map((index) => (
              <text
                className="trend-x-tick"
                key={buckets[index].label}
                textAnchor={
                  index === 0
                    ? "start"
                    : index === points.length - 1
                      ? "end"
                      : "middle"
                }
                x={points[index].x}
                y={height - 6}
              >
                {formatTrendXAxisLabel(buckets[index].label, granularity)}
              </text>
            ))}
          </svg>
          {points.map((point, index) => {
            const bucket = buckets[index];
            const label = formatTrendXAxisLabel(bucket.label, granularity);
            return (
              <Tooltip
                key={bucket.label}
                open={activeIndex === index}
                onOpenChange={(open) =>
                  setActiveIndex((active) =>
                    open ? index : active === index ? null : active,
                  )
                }
                content={
                  <span className="trend-tooltip-content">
                    <strong>{bucket.label}</strong>
                    <span>
                      总 Token <b>{fullNumber(bucket.total_tokens)}</b>
                    </span>
                    <span>
                      成功请求 <b>{fullNumber(bucket.request_count)} 次</b>
                    </span>
                  </span>
                }
              >
                <Button
                  className="trend-point"
                  style={{ left: point.x, top: point.y }}
                  aria-label={`${label}，${metricLabel} ${fullNumber(point.value)}`}
                  onFocus={() => setActiveIndex(index)}
                  onBlur={() => setActiveIndex(null)}
                  onPointerEnter={() => setActiveIndex(index)}
                  onPointerLeave={() => setActiveIndex(null)}
                />
              </Tooltip>
            );
          })}
        </>
      )}
    </div>
  );
}
