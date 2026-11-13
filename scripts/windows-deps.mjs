import { createWriteStream } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { Readable, Transform } from "node:stream";
import { spawnSync } from "node:child_process";
import { install, verify } from "./native-deps.mjs";
import { inspectFile, unexpectedImports } from "./windows-pe.mjs";

export const root = fileURLToPath(new URL("../", import.meta.url));
export const nativeRoot = join(root, "native-deps/windows-x64-klinoff-rc1");
export const pins = JSON.parse(await readFile(new URL("./windows-native.json", import.meta.url)));
export const mpv = JSON.parse(await readFile(new URL("./libmpv.json", import.meta.url)));
const redistributionReview = JSON.parse(
	await readFile(new URL("../packaging/compliance.json", import.meta.url)),
).review;

export function run(executable, args, options = {}) {
	const result = spawnSync(executable, args, {
		cwd: root,
		windowsHide: true,
		shell: false,
		stdio: ["ignore", "pipe", "pipe"],
		encoding: "utf8",
		timeout: 120_000,
		maxBuffer: 8 * 1024 * 1024,
		...options,
	});
	if (result.error) throw result.error;
	if (result.status !== 0)
		throw new Error(
			`${basename(executable)} failed (${result.status}): ${result.stderr || "see build output"}`,
		);
	return result.stdout;
}

export async function runVerified(executable, hash, args, options = {}) {
	await verify(executable, hash);
	return run(executable, args, {
		...options,
		cwd: resolve(executable, ".."),
		env: { ...process.env, PATH: join(process.env.SystemRoot || "C:/Windows", "System32") },
	});
}

export function assertPublicDistribution(review = redistributionReview) {
	if (review?.approved !== true || !Array.isArray(review.blockers) || review.blockers.length) {
		throw new Error(
			`Public distribution is blocked:\n${review?.blockers?.join("\n") || "Missing approval/evidence"}\nUse --internal-test only for private validation artifacts.`,
		);
	}
}

export const dependencyFiles = [
	...Object.entries(mpv.runtimeFiles).map(([file, hash]) => [
		`mpv/${basename(file)}`,
		basename(file),
		hash,
	]),
	...Object.entries(pins.ffmpeg.runtimeFiles).map(([file, hash]) => [
		`ffmpeg/${file}`,
		`bin/${file}`,
		hash,
	]),
	["mpv/vulkan-1.dll", "vulkan-1.dll", pins.vulkan.dllSha256],
	["vulkan/VulkanRT-License.txt", "licenses/VulkanRT-License.txt", pins.vulkan.licenseSha256],
];

export async function verifyDependencies() {
	for (const [file, , hash] of dependencyFiles) await verify(join(nativeRoot, file), hash);
	for (const [file, destination] of dependencyFiles.filter(([file]) =>
		/\.(exe|dll)$/.test(file),
	)) {
		const image = await inspectFile(join(nativeRoot, file));
		const neighbors = dependencyFiles
			.filter(
				([, other]) =>
					other.slice(0, other.lastIndexOf("/") + 1) ===
					destination.slice(0, destination.lastIndexOf("/") + 1),
			)
			.map(([, other]) => basename(other));
		const missing = unexpectedImports(image.imports, neighbors);
		if (missing.length)
			throw new Error(`Unresolved runtime dependencies in ${file}: ${missing.join(", ")}`);
	}
	const version = await runVerified(
		join(nativeRoot, "ffmpeg/ffmpeg.exe"),
		pins.ffmpeg.ffmpegSha256,
		["-version"],
	);
	if (
		!version.includes(pins.ffmpeg.version) ||
		!version.includes("--enable-gpl") ||
		!version.includes("--enable-version3") ||
		version.includes("--enable-nonfree")
	) {
		throw new Error("Unsupported FFmpeg build/version/license configuration");
	}
	const probe = await runVerified(
		join(nativeRoot, "ffmpeg/ffprobe.exe"),
		pins.ffmpeg.ffprobeSha256,
		["-version"],
	);
	if (!probe.split("\n")[0].includes(pins.ffmpeg.version))
		throw new Error("Unsupported FFprobe version");
	return version;
}

export async function download(pin) {
	const downloads = join(root, "native-deps/downloads");
	await mkdir(downloads, { recursive: true });
	const archive = join(downloads, `${pin.archiveSha256}.zip`);
	try {
		await stat(archive);
		await verify(archive, pin.archiveSha256);
		return archive;
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
	const temporary = await mkdtemp(join(downloads, "fetch-"));
	try {
		const response = await fetch(pin.archiveUrl, { signal: AbortSignal.timeout(300_000) });
		if (!response.ok || !response.body || !response.url.startsWith("https://"))
			throw new Error(`Dependency download failed: HTTP ${response.status}`);
		let bytes = 0;
		const limit = new Transform({
			transform(chunk, encoding, callback) {
				bytes += chunk.length;
				callback(
					bytes > 160 * 1024 * 1024
						? new Error("Dependency archive exceeds 160 MiB")
						: null,
					chunk,
				);
			},
		});
		const partial = join(temporary, "download.zip");
		await pipeline(
			Readable.fromWeb(response.body),
			limit,
			createWriteStream(partial, { flags: "wx" }),
		);
		await install(partial, archive, pin.archiveSha256);
		return archive;
	} finally {
		await rm(temporary, { recursive: true, force: true });
	}
}

export async function prepareMedia(packageDirectory) {
	await mkdir(join(root, "native-deps/downloads"), { recursive: true });
	for (const pin of [pins.ffmpeg, mpv]) {
		const archive = join(root, "native-deps/downloads", `${pin.archiveSha256}.zip`);
		if (packageDirectory) {
			await install(
				join(resolve(packageDirectory), pin.archiveFile),
				archive,
				pin.archiveSha256,
			);
			await install(
				join(resolve(packageDirectory), pin.sourceFile),
				join(root, "native-deps/compliance-inputs", pin.sourceFile),
				pin.sourceSha256,
			);
		}
		await verify(archive, pin.archiveSha256).catch(() => {
			throw new Error(
				`Provide the corrected packages: npm run native:setup -- <package-directory> (${pin.archiveFile})`,
			);
		});
		const temporary = await mkdtemp(join(root, "native-deps/extract-"));
		try {
			run(join(process.env.SystemRoot, "System32/tar.exe"), [
				"-xf",
				archive,
				"-C",
				temporary,
				...Object.keys(pin.runtimeFiles),
			]);
			for (const [member, hash] of Object.entries(pin.runtimeFiles))
				await install(
					join(temporary, member),
					join(nativeRoot, pin.directory, basename(member)),
					hash,
				);
		} finally {
			await rm(temporary, { recursive: true, force: true });
		}
	}
	for (const [name, pin, members] of [
		[
			"vulkan",
			pins.vulkan,
			[
				["x64/vulkan-1.dll", "vulkan-1.dll", pins.vulkan.dllSha256],
				["VulkanRT-License.txt", "VulkanRT-License.txt", pins.vulkan.licenseSha256],
			],
		],
	]) {
		const archive = await download(pin);
		const temporary = await mkdtemp(join(root, "native-deps/extract-"));
		try {
			// Extract only exact pinned archive members, never an arbitrary archive tree.
			run(join(process.env.SystemRoot, "System32/tar.exe"), [
				"-xf",
				archive,
				"-C",
				temporary,
				...members.map(([member]) => `${pin.archiveRoot}/${member}`),
			]);
			for (const [member, destination, hash] of members)
				await install(
					join(temporary, pin.archiveRoot, member),
					join(nativeRoot, name, destination),
					hash,
				);
		} finally {
			await rm(temporary, { recursive: true, force: true });
		}
	}
	await install(
		join(nativeRoot, "vulkan/vulkan-1.dll"),
		join(nativeRoot, "mpv/vulkan-1.dll"),
		pins.vulkan.dllSha256,
	);
	await verifyDependencies();
	console.log(
		`Verified KlinoffBuilds FFmpeg/FFprobe ${pins.ffmpeg.version} and libmpv ${mpv.version}: ${nativeRoot}`,
	);
}

async function prepare(packageDirectory) {
	await prepareMedia(packageDirectory);
	await (await import("./installer-tools.mjs")).prepareInstallerTools();
	console.log(
		"Verified pinned media dependencies and project-local NSIS tools. No system installation performed.",
	);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	const command = process.argv[2];
	const action =
		command === "prepare"
			? () => prepare(process.argv[3])
			: command === "check"
				? async () => {
						await verifyDependencies();
						await (await import("./installer-tools.mjs")).verifyInstallerTools();
					}
				: null;
	if (process.platform !== "win32" || process.arch !== "x64" || !action) {
		console.error("Usage (Windows x64): node scripts/windows-deps.mjs prepare|check");
		process.exitCode = 1;
	} else
		action().catch((error) => {
			console.error(error.message);
			process.exitCode = 1;
		});
}
