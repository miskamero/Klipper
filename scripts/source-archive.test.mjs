import assert from "node:assert/strict";
import { test } from "node:test";
import { spawnSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rm, symlink, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import {
	assertSourceUnchanged,
	regularSource,
	sourceEligible,
	sourceState,
} from "./source-archive.mjs";

function git(directory, args) {
	const result = spawnSync("git", args, { cwd: directory, encoding: "utf8", windowsHide: true });
	assert.equal(result.status, 0, result.stderr);
}

async function fixture(t) {
	const directory = await mkdtemp(join(tmpdir(), "klipper-source-"));
	t.after(() => rm(directory, { recursive: true, force: true })); // Unique test-owned directory.
	git(directory, ["init", "--quiet"]);
	return directory;
}

async function put(directory, name, content = "synthetic") {
	await mkdir(dirname(join(directory, name)), { recursive: true });
	await writeFile(join(directory, name), content);
}

test("source selection excludes ignored, sensitive and unrelated data, even when tracked", async (t) => {
	const directory = await fixture(t);
	await put(directory, ".gitignore", "src/ignored.ts\nsrc/ignored-data/\n");
	const required = [
		"src/App.tsx",
		"src-tauri/src/library/migrations/001.sql",
		"scripts/build.mjs",
		"packaging/compliance.json",
		"package-lock.json",
	];
	for (const name of required) await put(directory, name);
	const privateFiles = [
		"src/.env",
		"src/.env.local",
		"src/debug.log",
		"src/personal.mp4",
		"src/private/account.json",
		"src/fixtures/catalog.json",
		"src/node_modules/package/index.js",
		"src/ignored.ts",
		"src/ignored-data/config.json",
		"local/private.ts",
		"unrelated.ts",
	];
	for (const name of privateFiles) await put(directory, name, "not for redistribution");
	// Only the disposable synthetic repository is staged; the Klipper index is untouched.
	git(directory, ["add", "-f", "src/ignored.ts", "src/.env"]);
	const state = await sourceState(directory);
	assert.deepEqual(Object.keys(state), [".gitignore", ...required].sort());
	const archive = join(directory, "source.zip");
	const tar =
		process.platform === "win32" ? join(process.env.SystemRoot, "System32/tar.exe") : "tar";
	const result = spawnSync(tar, ["-a", "-cf", archive, "--", ...Object.keys(state)], {
		cwd: directory,
	});
	assert.equal(result.status, 0, result.stderr?.toString());
	const listing = spawnSync(tar, ["-tf", archive], { encoding: "utf8" });
	assert.equal(listing.status, 0);
	assert.deepEqual(listing.stdout.trim().split(/\r?\n/).sort(), Object.keys(state));
	for (const name of privateFiles) assert.ok(!listing.stdout.split(/\r?\n/).includes(name));
});

test("dirty tracked and untracked build inputs cannot change unnoticed", async (t) => {
	const directory = await fixture(t);
	await put(directory, "src/App.tsx", "initial");
	git(directory, ["add", "src/App.tsx"]);
	await put(directory, "src/App.tsx", "intentional dirty version");
	await put(directory, "scripts/new-build.mjs");
	const state = await sourceState(directory);
	assert.ok(state["scripts/new-build.mjs"]);
	await assertSourceUnchanged(directory, state);
	await put(directory, "src/App.tsx", "changed during compilation");
	await assert.rejects(() => assertSourceUnchanged(directory, state), /changed during packaging/);
	await assert.rejects(() => assertSourceUnchanged(directory, {}), /changed during packaging/);
	const current = await sourceState(directory);
	await put(directory, "src/added.ts");
	await assert.rejects(
		() => assertSourceUnchanged(directory, current),
		/changed during packaging/,
	);
	const withAdded = await sourceState(directory);
	await rm(join(directory, "scripts/new-build.mjs"));
	await assert.rejects(
		() => assertSourceUnchanged(directory, withAdded),
		/changed during packaging/,
	);
});

test("source paths reject escapes, links and directory junctions", async (t) => {
	const directory = await fixture(t);
	for (const name of [
		"src/../secret.ts",
		"../outside.ts",
		"src\\outside.ts",
		"C:/private.ts",
		"src/.env.ts",
	])
		assert.equal(sourceEligible(name), false);
	await put(directory, "outside/value.ts");
	await mkdir(join(directory, "src"));
	await symlink(
		join(directory, "outside"),
		join(directory, "src/linked"),
		process.platform === "win32" ? "junction" : "dir",
	);
	await assert.rejects(() => regularSource(directory, "src/linked/value.ts"), /Linked source/);
	assert.equal(await readFile(join(directory, "outside/value.ts"), "utf8"), "synthetic");
});

test("private Vite environment inputs cannot leak indirectly into a compiled release", async (t) => {
	const directory = await fixture(t);
	await put(directory, ".env.production", "VITE_PRIVATE=synthetic-secret");
	await assert.rejects(() => sourceState(directory), /private local .env/);
});
