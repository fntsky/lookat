import test from "node:test";
import assert from "node:assert/strict";
import { sessionView } from "../src/session-clock.js";

const report = (updates = {}) => ({
  state: "active", stale: false,
  current: { id: "editor", session_ms: 60_000 },
  observed_at: "2026-10-25T10:00:00.000Z",
  server_now: "2026-10-25T10:00:05.000Z",
  total_ms: 15_000,
  ...updates,
});

test("session advances each second including snapshot age without altering totals", () => {
  const snapshot = report();
  assert.deepEqual(sessionView(snapshot, 1000, 1000), { seconds: 65, stale: false });
  assert.equal(sessionView(snapshot, 1000, 2000).seconds, 66);
  assert.equal(snapshot.total_ms, 15_000);
  assert.equal(snapshot.current.session_ms, 60_000);
});

test("stale, inactive and missing snapshots never keep counting", () => {
  assert.deepEqual(sessionView(report(), 1000, 27_000), { seconds: 60, stale: true });
  assert.equal(sessionView(report({ stale: true }), 1000, 2000).seconds, 60);
  for (const state of ["locked", "idle", "paused", "no_window"]) {
    assert.equal(sessionView(report({ state, current: null }), 1000, 2000).seconds, 0);
  }
  assert.equal(sessionView(null, 1000, 2000).seconds, 0);
  assert.equal(sessionView(report({ server_now: undefined }), 1000, 2000).stale, true);
  assert.equal(sessionView(report({ observed_at: "2026-10-25T10:01:00.000Z" }), 1000, 2000).stale, true);
});

test("new application or corrected snapshot replaces the previous baseline", () => {
  assert.equal(sessionView(report(), 0, 10_000).seconds, 75);
  const switched = report({ current: { id: "terminal", session_ms: 500 }, observed_at: "2026-10-25T10:00:05.000Z" });
  assert.equal(sessionView(switched, 10_000, 11_000).seconds, 1);
  assert.equal(sessionView(switched, 10_000, 9_000).seconds, 0);
});
