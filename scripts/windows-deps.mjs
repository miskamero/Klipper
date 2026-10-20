import { createWriteStream } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, stat } from "node:fs/promises";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { pipeline } from "node:stream/promises";
import { Readable, Transform } from "node:stream";
import { spawnSync } from "node:child_process";
import { install, verify } from "./native-deps.mjs";
import { inspectFile } from "./windows-pe.mjs";

export const root = fileURLToPath(new URL("../", import.meta.url));
export const nativeRoot = join(root, "native-deps/windows-x64");
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
	return run(executable, args, options);
}

export function assertPublicDistribution(review = redistributionReview) {
	if (review?.approved !== true || !Array.isArray(review.blockers) || review.blockers.length) {
		throw new Error(
			`Public distribution is blocked:\n${review?.blockers?.join("\n") || "Missing approval/evidence"}\nUse --internal-test only for private validation artifacts.`,
		);
	}
}

export const dependencyFiles = [
	["libmpv-2.dll", "libmpv-2.dll", mpv.dllSha256],
	["ffmpeg/ffmpeg.exe", "bin/ffmpeg.exe", pins.ffmpeg.ffmpegSha256],
	["ffmpeg/ffprobe.exe", "bin/ffprobe.exe", pins.ffmpeg.ffprobeSha256],
	["vulkan/vulkan-1.dll", "vulkan-1.dll", pins.vulkan.dllSha256],
	["ffmpeg/LICENSE", "licenses/FFmpeg-GPL.txt", pins.ffmpeg.licenseSha256],
	["vulkan/VulkanRT-License.txt", "licenses/VulkanRT-License.txt", pins.vulkan.licenseSha256],
];

export async function verifyDependencies() {
	for (const [file, , hash] of dependencyFiles) await verify(join(nativeRoot, file), hash);
	for (const [file] of dependencyFiles.filter(([file]) => /\.(exe|dll)$/.test(file)))
		await inspectFile(join(nativeRoot, file));
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

async function prepare() {
	await verify(join(nativeRoot, "libmpv-2.dll"), mpv.dllSha256).catch(() => {
		throw new Error(
			"First install the pinned libmpv using npm run native:setup -- <extracted-libmpv-2.dll>. See scripts/libmpv.json for the verified archive.",
		);
	});
	for (const [name, pin, members] of [
		[
			"ffmpeg",
			pins.ffmpeg,
			[
				["bin/ffmpeg.exe", "ffmpeg.exe", pins.ffmpeg.ffmpegSha256],
				["bin/ffprobe.exe", "ffprobe.exe", pins.ffmpeg.ffprobeSha256],
				["LICENSE", "LICENSE", pins.ffmpeg.licenseSha256],
			],
		],
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
	await verifyDependencies();
	await (await import("./installer-tools.mjs")).prepareInstallerTools();
	console.log(
		"Verified pinned media dependencies and project-local NSIS tools. No system installation performed.",
	);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	const command = process.argv[2];
	const action =
		command === "prepare"
			? prepare
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
