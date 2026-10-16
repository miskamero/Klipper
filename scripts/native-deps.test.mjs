import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { checksum, install, verify } from "./native-deps.mjs";

test("installs verified bytes and permits an identical repeated setup", async (t) => {
	const directory = await mkdtemp(join(tmpdir(), "klipper-dependency-"));
	t.after(() => rm(directory, { recursive: true, force: true }));
	const source = join(directory, "synthetic-input");
	const destination = join(directory, "dependency", "synthetic-output");
	const bytes = Buffer.from("Synthetic dependency fixture; no executable content.");
	const expected = createHash("sha256").update(bytes).digest("hex");
	await writeFile(source, bytes);
	await install(source, destination, expected);
	await install(source, destination, expected);
	assert.equal(await checksum(destination), expected);
	assert.deepEqual(await readFile(source), bytes);
});

test("rejects an unverified input and does not overwrite an unknown destination", async (t) => {
	const directory = await mkdtemp(join(tmpdir(), "klipper-dependency-"));
	t.after(() => rm(directory, { recursive: true, force: true }));
	const source = join(directory, "synthetic-input");
	const destination = join(directory, "synthetic-output");
	await writeFile(source, "approved");
	await writeFile(destination, "preserve this existing file");
	await assert.rejects(install(source, destination, "invalid-checksum"), /SHA-256 mismatch/);
	await assert.rejects(install(source, destination, await checksum(source)), /SHA-256 mismatch/);
	assert.equal(await readFile(destination, "utf8"), "preserve this existing file");
	await assert.rejects(verify(join(directory, "missing"), "invalid"), { code: "ENOENT" });
});
