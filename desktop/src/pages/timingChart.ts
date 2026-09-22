/** Monotone cubic interpolation through equally spaced daily timing values. */
export function timingChartPath(values: readonly number[], maxValue: number): string {
  if (values.length === 0) return "";
  const scale = Math.max(0.01, maxValue);
  const y = values.map((value) => 100 - (value / scale) * 90);
  const step = 100 / Math.max(1, y.length - 1);
  const slopes = y.slice(1).map((value, i) => value - y[i]);
  const tangents = y.map((_, i) => {
    if (i === 0) return slopes[0] ?? 0;
    if (i === y.length - 1) return slopes[i - 1];
    const before = slopes[i - 1], after = slopes[i];
    // A zero tangent at extrema and flat intervals prevents invented peaks,
    // negative durations and bumps during days without activity.
    return before * after > 0 ? 2 / (1 / before + 1 / after) : 0;
  });
  const point = (x: number, value: number) => `${x.toFixed(4)},${value.toFixed(4)}`;
  let path = `M${point(0, y[0])}`;
  for (let i = 0; i < y.length - 1; i++) {
    path += ` C${point((i + 1 / 3) * step, y[i] + tangents[i] / 3)} ${point((i + 2 / 3) * step, y[i + 1] - tangents[i + 1] / 3)} ${point((i + 1) * step, y[i + 1])}`;
  }
  return path;
}
