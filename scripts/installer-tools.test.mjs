import assert from "node:assert/strict";
import { test } from "node:test";
import { appendFile, copyFile, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { checksum } from "./native-deps.mjs";
import { root, runVerified } from "./windows-deps.mjs";
import { installerPins, prepareInstallerTools, verifyInstallerTools } from "./installer-tools.mjs";

test("a modified executable is rejected before it can execute, even for version inspection", async (t) => {
	const directory = await mkdtemp(join(tmpdir(), "klipper-executable-"));
	t.after(() => rm(directory, { recursive: true, force: true }));
	const executable = join(directory, process.platform === "win32" ? "tool.exe" : "tool");
	await copyFile(process.execPath, executable);
	const pinned = await checksum(executable);
	await appendFile(executable, "synthetic modification");
	const marker = join(directory, "executed.txt");
	await assert.rejects(
		() =>
			runVerified(executable, pinned, [
				"-e",
				"require('node:fs').writeFileSync(process.argv[1], 'executed')",
				marker,
			]),
		/SHA-256 mismatch/,
	);
	await assert.rejects(() => readFile(marker), { code: "ENOENT" });
});

test(
	"fresh local NSIS provisioning is verified, repeatable and rejects changed or extra tools",
	{
		skip: process.platform !== "win32" || process.env.KLIPPER_TEST_INSTALLER_TOOLS !== "1",
	},
	async (t) => {
		const directory = await mkdtemp(join(tmpdir(), "klipper-nsis-bootstrap-"));
		t.after(() => rm(directory, { recursive: true, force: true }));
		// Opt-in integration test uses the authenticated downloads, never the user's NSIS cache.
		const cached = async (pin) =>
			join(root, "native-deps/downloads", `${pin.archiveSha256}.zip`);
		await assert.rejects(() => verifyInstallerTools(directory), { code: "ENOENT" });
		const manifest = await prepareInstallerTools(directory, cached);
		assert.deepEqual(manifest.pins, installerPins);
		assert.ok(Object.keys(manifest.files).length > 50);
		assert.deepEqual(
			await prepareInstallerTools(directory, () => {
				throw new Error("Unexpected download");
			}),
			manifest,
		);
		await writeFile(join(directory, "NSIS/unowned.txt"), "preserve");
		await assert.rejects(() => prepareInstallerTools(directory, cached), /tools changed/);
		assert.equal(await readFile(join(directory, "NSIS/unowned.txt"), "utf8"), "preserve");
		await rm(join(directory, "NSIS/unowned.txt"));
		await appendFile(join(directory, "NSIS/makensis.exe"), "tampered");
		await assert.rejects(() => verifyInstallerTools(directory), /tools changed/);
	},
);
