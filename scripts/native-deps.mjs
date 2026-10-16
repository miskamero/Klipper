import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { constants, copyFile, mkdir, readFile, stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export async function checksum(path) {
	const hash = createHash("sha256");
	for await (const bytes of createReadStream(path)) hash.update(bytes);
	return hash.digest("hex");
}

export async function verify(path, expected) {
	if ((await checksum(path)) !== expected) throw new Error(`SHA-256 mismatch: ${path}`);
}

export async function install(source, destination, expected) {
	await verify(source, expected);
	try {
		await stat(destination);
		await verify(destination, expected);
		return; // Already installed. Never replace an unknown or currently loaded DLL.
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
	await mkdir(dirname(destination), { recursive: true });
	await copyFile(source, destination, constants.COPYFILE_EXCL);
	await verify(destination, expected);
}

async function main(args) {
	const root = fileURLToPath(new URL("../", import.meta.url));
	const manifest = JSON.parse(await readFile(new URL("./libmpv.json", import.meta.url), "utf8"));
	const destination = resolve(root, "native-deps/windows-x64/libmpv-2.dll");
	if (args.length === 0) {
		await verify(destination, manifest.dllSha256);
	} else if (args.length === 2 && args[0] === "install") {
		await install(resolve(args[1]), destination, manifest.dllSha256);
	} else {
		throw new Error(
			"Usage: npm run native:check OR npm run native:setup -- <extracted-libmpv-2.dll>",
		);
	}
	console.log(`Verified libmpv ${manifest.version}: ${destination}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	main(process.argv.slice(2)).catch((error) => {
		console.error(
			`${error.message}\nSee scripts/libmpv.json for the pinned download and checksums. No automatic download is performed.`,
		);
		process.exitCode = 1;
	});
}
