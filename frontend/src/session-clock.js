// Only the session display is extrapolated; persisted aggregates stay unchanged.
export function sessionView(report, receivedAt, now) {
  const base = Math.max(0, report?.current?.session_ms || 0);
  const serverNow = Date.parse(report?.server_now);
  const observedAt = Date.parse(report?.observed_at);
  const elapsed = Math.max(0, now - receivedAt);
  const age = serverNow + elapsed - observedAt;
  const stale = Boolean(report?.stale) || !Number.isFinite(age) || age > 30_000 || age < -5_000;
  const running = Boolean(report?.current) && report?.state === "active" && !stale;
  return {
    stale,
    seconds: Math.floor((base + (running ? Math.max(0, age) : 0)) / 1000),
  };
}
