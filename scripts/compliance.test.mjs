import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { checksum } from "./native-deps.mjs";
import {
	assertReviewed,
	contained,
	frontendPackages,
	registryPackages,
	verifyBundle,
	refreshBundle,
} from "./compliance.mjs";

test("approval alone cannot clear unresolved corresponding-source obligations", async () => {
	assert.throws(
		() => assertReviewed({ approved: true, blockers: ["Unresolved static sources"] }),
		/blocked/,
	);
	assert.throws(() => assertReviewed({ approved: true }), /blocked/);
	assert.throws(() => assertReviewed({ approved: false, blockers: [] }), /blocked/);
	assert.doesNotThrow(() => assertReviewed({ approved: true, blockers: [] }));
	const policy = JSON.parse(
		await readFile(new URL("../packaging/compliance.json", import.meta.url)),
	);
	assert.throws(() => assertReviewed(policy.review), /libmpv.*exact revisions/s);
	assert.equal(policy.sources.length, 6);
	assert.equal(policy.review.approved, false);
	assert.match(policy.obligations.permissiveComponents, /MIT does not require/);
	assert.match(policy.verificationPolicy.reproducibility, /not a release requirement/);
	for (const source of policy.sources) {
		assert.match(source.url, /^https:\/\/codeload\.github\.com\/.+\/zip\/[a-f0-9]{40}$/);
		assert.match(source.sha256, /^[a-f0-9]{64}$/);
	}
});

test("compliance refresh replaces only inventory-owned unchanged files and is idempotent", async (t) => {
	const directory = await mkdtemp(join(tmpdir(), "klipper-refresh-"));
	t.after(() => rm(directory, { recursive: true, force: true }));
	const destination = join(directory, "bundle");
	async function next(name, content) {
		const working = await mkdtemp(join(directory, "new-"));
		await writeFile(join(working, name), content);
		return [
			working,
			{ schemaVersion: 1, files: { [name]: await checksum(join(working, name)) } },
		];
	}
	async function publish(name, content) {
		const [working, manifest] = await next(name, content);
		await refreshBundle(working, destination, manifest);
		await verifyBundle(destination, manifest);
	}
	await publish("old-license.txt", "old version");
	await publish("new-license.txt", "new version");
	await assert.rejects(() => readFile(join(destination, "old-license.txt")), { code: "ENOENT" });
	await publish("new-license.txt", "new version");
	await writeFile(join(destination, "personal-notes.txt"), "preserve me");
	let [working, manifest] = await next("third.txt", "next version");
	await assert.rejects(() => refreshBundle(working, destination, manifest), /Unexpected/);
	assert.equal(await readFile(join(destination, "personal-notes.txt"), "utf8"), "preserve me");
	await rm(join(destination, "personal-notes.txt"));
	await writeFile(join(destination, "new-license.txt"), "user modified");
	[working, manifest] = await next("third.txt", "next version");
	await assert.rejects(() => refreshBundle(working, destination, manifest), /SHA-256 mismatch/);
	assert.equal(await readFile(join(destination, "new-license.txt"), "utf8"), "user modified");
});

test("Rust source closure includes transitive build/runtime dependencies, excluding dev-only crates", () => {
	const edge = (pkg, kind = null) => ({ pkg, dep_kinds: [{ kind }] });
	const result = registryPackages({
		resolve: {
			root: "app",
			nodes: [
				{ id: "app", deps: [edge("runtime"), edge("build", "build"), edge("test", "dev")] },
				{ id: "build", deps: [edge("helper")] },
				{ id: "runtime", deps: [] },
				{ id: "test", deps: [] },
				{ id: "helper", deps: [] },
			],
		},
		packages: ["app", "runtime", "build", "test", "helper"].map((id) => ({
			id,
			source: id !== "app" ? "registry" : null,
		})),
	});
	assert.deepEqual(
		result.map((pkg) => pkg.id),
		["runtime", "build", "helper"],
	);
});

test("frontend source closure resolves nested dependencies and rejects missing lock entries", () => {
	const lock = {
		packages: {
			"": { dependencies: { react: "1", "react-dom": "1" } },
			"node_modules/react": {},
			"node_modules/react-dom": { dependencies: { react: "1", scheduler: "1" } },
			"node_modules/react-dom/node_modules/scheduler": {},
			"node_modules/test-only": {},
		},
	};
	assert.deepEqual(frontendPackages(lock), [
		"node_modules/react",
		"node_modules/react-dom",
		"node_modules/react-dom/node_modules/scheduler",
	]);
	delete lock.packages["node_modules/react-dom/node_modules/scheduler"];
	assert.throws(() => frontendPackages(lock), /Unresolved frontend dependency/);
});

test("compliance bundle rejects missing, altered, extra and escaping artifacts", async () => {
	const directory = await mkdtemp(join(tmpdir(), "klipper-compliance-"));
	try {
		await mkdir(join(directory, "licenses"));
		const license = join(directory, "licenses/NOTICE.txt");
		await writeFile(license, "Synthetic notice\n");
		const manifest = {
			schemaVersion: 1,
			files: { "licenses/NOTICE.txt": await checksum(license) },
		};
		await verifyBundle(directory, manifest);
		await writeFile(license, "changed");
		await assert.rejects(() => verifyBundle(directory, manifest), /SHA-256 mismatch/);
		await rm(license);
		await assert.rejects(() => verifyBundle(directory, manifest), /missing compliance/);
		await writeFile(license, "Synthetic notice\n");
		await writeFile(join(directory, "unexpected.txt"), "Not allowlisted");
		await assert.rejects(() => verifyBundle(directory, manifest), /Unexpected/);
		for (const path of ["../outside", "C:/outside", "nested\\outside", "", "/outside"])
			assert.throws(() => contained(directory, path), /Unsafe/);
		await assert.rejects(
			() => verifyBundle(directory, { schemaVersion: 1, files: {} }),
			/Missing compliance/,
		);
	} finally {
		// This is a unique test-owned child of the platform temporary directory.
		await rm(directory, { recursive: true, force: true });
	}
});
