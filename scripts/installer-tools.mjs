import { lstat, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { checksum, install, verify } from "./native-deps.mjs";
import { download, root, run } from "./windows-deps.mjs";

export const installerPins = JSON.parse(
	await readFile(new URL("./installer-tools.json", import.meta.url)),
);
export const toolsDirectory = join(root, "src-tauri/target/.tauri");
const inputs = JSON.parse(
	await readFile(new URL("../packaging/compliance.json", import.meta.url)),
).installerInputs;

export async function toolFiles(directory) {
	if ((await lstat(directory)).isSymbolicLink())
		throw new Error("Linked installer tools directory");
	const result = {};
	async function visit(current) {
		for (const entry of await readdir(current, { withFileTypes: true })) {
			const path = join(current, entry.name);
			if (entry.isSymbolicLink()) throw new Error("Linked installer tool");
			if (entry.isDirectory()) await visit(path);
			else if (entry.isFile())
				result[relative(directory, path).replaceAll("\\", "/")] = await checksum(path);
			else throw new Error("Unsupported installer tool file");
		}
	}
	await visit(directory);
	return Object.fromEntries(Object.entries(result).sort(([a], [b]) => a.localeCompare(b)));
}

export async function verifyInstallerTools(base = toolsDirectory) {
	const manifest = JSON.parse(await readFile(join(base, "installer-tools.json")));
	if (
		JSON.stringify(manifest.pins) !== JSON.stringify(installerPins) ||
		JSON.stringify(manifest.files) !== JSON.stringify(await toolFiles(join(base, "NSIS")))
	)
		throw new Error("Installer tools changed; review them and rerun npm run dist:prepare");
	for (const [file, hash] of inputs) await verify(join(base, "NSIS", file), hash);
	return manifest;
}

export async function prepareInstallerTools(base = toolsDirectory, fetchArchive = download) {
	try {
		await lstat(join(base, "NSIS"));
		return await verifyInstallerTools(base); // Never overwrite an unknown existing toolset.
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
		// A missing manifest with existing tools is not permission to replace those tools.
		try {
			await lstat(join(base, "NSIS"));
			throw new Error("Unowned NSIS tools; review before replacing", { cause: error });
		} catch (error) {
			if (error.code !== "ENOENT") throw error;
		}
	}
	await mkdir(base, { recursive: true });
	const temporary = await mkdtemp(join(base, "nsis-prepare-"));
	if (dirname(temporary) !== resolve(base)) throw new Error("Unsafe installer work directory");
	try {
		const archive = await fetchArchive(installerPins.nsis);
		const plugin = await fetchArchive(installerPins.plugin);
		await verify(archive, installerPins.nsis.archiveSha256);
		await verify(plugin, installerPins.plugin.archiveSha256);
		// The complete archive is authenticated before extraction or executing any tool.
		run(join(process.env.SystemRoot, "System32/tar.exe"), ["-xf", archive, "-C", temporary]);
		const extracted = join(temporary, installerPins.nsis.archiveRoot);
		await install(
			plugin,
			join(extracted, "Plugins/x86-unicode/additional/nsis_tauri_utils.dll"),
			installerPins.plugin.archiveSha256,
		);
		for (const [file, hash] of inputs) await verify(join(extracted, file), hash);
		const manifest = { pins: installerPins, files: await toolFiles(extracted) };
		await rename(extracted, join(base, "NSIS"));
		await writeFile(
			join(base, "installer-tools.json"),
			JSON.stringify(manifest, null, "\t") + "\n",
			{ flag: "wx" },
		);
		return await verifyInstallerTools(base);
	} finally {
		await rm(temporary, { recursive: true, force: true });
	}
}
