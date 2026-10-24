import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import Ajv2020 from "ajv/dist/2020.js";
import addFormats from "ajv-formats";
const readJson = async (file) =>
  JSON.parse(
    await readFile(new URL(`../../docs/${file}`, import.meta.url), "utf8"),
  );
const schema = await readJson("ingestion-v1.schema.json");
const active = await readJson("examples/active-batch.json");
const locked = await readJson("examples/locked-heartbeat.json");
const ajv = new Ajv2020({ allErrors: true, strict: true });
addFormats(ajv);
const validate = ajv.compile(schema);

test("published examples follow the draft schema, size and time semantics", () => {
  for (const batch of [active, locked]) {
    assert.equal(validate(batch), true, JSON.stringify(validate.errors));
    assert.ok(Buffer.byteLength(JSON.stringify(batch)) <= 16384);
    const observed = Date.parse(batch.snapshot.observed_at);
    assert.ok(observed <= Date.parse(batch.sent_at));
    if (batch.snapshot.state === "active") {
      assert.ok(Date.parse(batch.snapshot.since) <= observed);
      assert.ok(
        batch.snapshot.session_ms <=
          observed - Date.parse(batch.snapshot.since) + 1000,
      );
    }
    for (const interval of batch.intervals) {
      const start = Date.parse(interval.started_at),
        end = Date.parse(interval.ended_at);
      assert.ok(start < end && end <= Date.parse(batch.sent_at));
      assert.ok(Math.abs(end - start - interval.duration_ms) <= 1000);
    }
  }
});

test("schema rejects invented identity, private window data and unexpected fields", () => {
  for (const change of [
    (batch) => (batch.user_id = "untrusted"),
    (batch) => (batch.token = "untrusted"),
    (batch) => (batch.snapshot.application.window_title = "private"),
    (batch) => (batch.device.username = "untrusted"),
    (batch) => (batch.intervals[0].duration_ms = 0),
    (batch) => (batch.intervals[0].duration_ms = 60001),
    (batch) => (batch.intervals[0].duration_ms = 1.5),
    (batch) => (batch.schema_version = 2),
    (batch) => (batch.sequence = -1),
    (batch) => (batch.sent_at = "2026-10-25T17:06:24.000+08:00"),
    (batch) => (batch.sent_at = "not-a-date"),
    (batch) => (batch.intervals = Array(21).fill(batch.intervals[0])),
  ]) {
    const batch = structuredClone(active);
    change(batch);
    assert.equal(validate(batch), false);
  }
});

test("inactive snapshots cannot retain stale app or session fields", () => {
  for (const state of ["idle", "locked", "paused", "no_window"]) {
    const batch = structuredClone(locked);
    batch.snapshot.state = state;
    assert.equal(validate(batch), true);
    batch.snapshot.session_ms = 1000;
    assert.equal(validate(batch), false);
  }
});
