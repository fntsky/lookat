import { mkdir, copyFile } from "node:fs/promises";
const destination = new URL("../public/protocol/", import.meta.url);
await mkdir(destination, { recursive: true });
for (const [source, name] of [
  ["ingestion-v1.md", "ingestion-v1.md"],
  ["ingestion-v1.schema.json", "ingestion-v1.schema.json"],
  ["examples/active-batch.json", "active-batch.json"],
  ["examples/locked-heartbeat.json", "locked-heartbeat.json"],
])
  await copyFile(
    new URL(`../../docs/${source}`, import.meta.url),
    new URL(name, destination),
  );
