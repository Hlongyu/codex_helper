const axisNumber = new Intl.NumberFormat("zh-CN", {
  notation: "compact",
  maximumFractionDigits: 1,
});

export function formatTrendAxisValue(value: number) {
  return axisNumber.format(value);
}

export function formatTrendXAxisLabel(label: string, granularity: string) {
  if (granularity === "hour" || granularity === "month") return label;
  const [, month, day] = label.split("-");
  return month && day ? `${month}-${day}` : label;
}

function niceTrendMax(value: number) {
  if (value <= 0) return 1;
  const base = 10 ** Math.floor(Math.log10(value));
  const fraction = value / base;
  return (
    (fraction <= 1 ? 1 : fraction <= 2 ? 2 : fraction <= 5 ? 5 : 10) * base
  );
}

function tickIndexes(count: number, plotWidth: number) {
  const limit = Math.max(2, Math.min(6, Math.floor(plotWidth / 84) + 1));
  if (count <= limit) return Array.from({ length: count }, (_, index) => index);
  return Array.from({ length: limit }, (_, index) =>
    Math.round((index * (count - 1)) / (limit - 1)),
  );
}

/** Coordinates use the container's pixels, so labels and strokes never scale. */
export function buildTrendGeometry(
  values: readonly number[],
  width: number,
  height: number,
) {
  const maximum = Math.max(4, niceTrendMax(Math.max(...values, 0)));
  const ticks = [1, 0.75, 0.5, 0.25, 0].map((ratio) => ({
    value: maximum * ratio,
    label: formatTrendAxisValue(maximum * ratio),
    ratio,
  }));
  // Compact labels are short; reserve enough room for both digits and Chinese units.
  const left = Math.max(
    38,
    ...ticks.map(
      ({ label }) =>
        Array.from(label).reduce(
          (sum, char) => sum + (char.charCodeAt(0) > 255 ? 12 : 7),
          0,
        ) + 14,
    ),
  );
  const top = 12;
  const bottom = height - 28;
  const right = width - 12;
  const plotWidth = Math.max(1, right - left);
  const plotHeight = bottom - top;
  const points = values.map((value, index) => ({
    value,
    x:
      left +
      (values.length <= 1
        ? plotWidth / 2
        : (index / (values.length - 1)) * plotWidth),
    y: bottom - (value / maximum) * plotHeight,
  }));
  return {
    left,
    right,
    top,
    bottom,
    points,
    ticks: ticks.map((tick) => ({
      ...tick,
      y: bottom - tick.ratio * plotHeight,
    })),
    xTickIndexes: tickIndexes(values.length, plotWidth),
  };
}
