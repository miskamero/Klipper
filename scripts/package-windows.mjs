import { mkdir, mkdtemp, readFile, readdir, copyFile, writeFile } from "node:fs/promises";
import { join, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { checksum } from "./native-deps.mjs";
import { inspectFile } from "./windows-pe.mjs";
import { checkCompliance, stageCompliance } from "./compliance.mjs";
import { sourceState, assertSourceUnchanged } from "./source-archive.mjs";
import { verifyInstallerTools } from "./installer-tools.mjs";
import {
	root,
	nativeRoot,
	pins,
	mpv,
	dependencyFiles,
	verifyDependencies,
	assertPublicDistribution,
	run,
	runVerified,
} from "./windows-deps.mjs";

// Deliberately finite: an unexpected non-system import must stop packaging.
const systemDlls = new Set(
	`advapi32 avicap32 avrt bcrypt bcryptprimitives cabinet cfgmgr32 clbcatq combase comctl32 comdlg32 crypt32 cryptbase cryptsp d2d1 d3d11 d3d12 d3dcompiler_47 dbghelp dnsapi dwmapi dwrite dxgi dxva2 gdi32 gdiplus imm32 iphlpapi kernel32 kernelbase mf mfplat mfreadwrite mfuuid mpr msimg32 msvcrt ncrypt normaliz ntdll ole32 oleacc oleaut32 opengl32 powrprof propsys psapi rpcrt4 secur32 setupapi shcore shell32 shlwapi sspicli ucrtbase user32 userenv usp10 uxtheme version winhttp wininet winmm winspool wintrust wldap32 ws2_32 wtsapi32`
		.split(" ")
		.map((name) => `${name}.dll`),
);

export function unexpectedImports(imports, bundledNames) {
	const bundled = new Set(bundledNames.map((name) => name.toLowerCase()));
	return imports.filter(
		(name) =>
			!systemDlls.has(name) &&
			!/^api-ms-win-(core|crt|security)-[a-z0-9-]+\.dll$/.test(name) &&
			!/^ext-ms-win-[a-z0-9-]+\.dll$/.test(name) &&
			!bundled.has(name),
	);
}

export function validateVersions(packageVersion, tauriVersion, cargoText) {
	const cargoVersion = cargoText.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
	if (
		!/^\d+\.\d+\.\d+$/.test(packageVersion) ||
		packageVersion !== tauriVersion ||
		packageVersion !== cargoVersion
	)
		throw new Error("package.json, Tauri and Cargo package versions must agree");
	return packageVersion;
}

export function validateNpmConfig(text) {
	if (
		text
			.split(/\r?\n/)
			.some(
				(line) =>
					line.trim() &&
					!line.trim().startsWith("#") &&
					line.trim() !== "install-strategy=nested",
			)
	)
		throw new Error(
			"Unreviewed .npmrc settings; do not package credentials or private registry configuration",
		);
}

async function copy(source, destination) {
	await mkdir(resolve(destination, ".."), { recursive: true });
	await copyFile(source, destination, 1); // COPYFILE_EXCL: never overwrite an existing artifact.
}

async function auditBinaries(directory) {
	const files = [
		"klipper.exe",
		"libmpv-2.dll",
		"vulkan-1.dll",
		"bin/ffmpeg.exe",
		"bin/ffprobe.exe",
	];
	const result = {};
	for (const file of files) {
		const inspected = await inspectFile(join(directory, file));
		const missing = unexpectedImports(
			inspected.imports,
			file.startsWith("bin/") ? [] : ["libmpv-2.dll", "vulkan-1.dll"],
		);
		if (missing.length)
			throw new Error(`Unresolved native dependencies in ${file}: ${missing.join(", ")}`);
		result[file] = { sha256: await checksum(join(directory, file)), ...inspected };
	}
	return result;
}

async function sourceSnapshot(destination, state) {
	validateNpmConfig(await readFile(join(root, ".npmrc"), "utf8"));
	run(join(process.env.SystemRoot, "System32/tar.exe"), [
		"-a",
		"-cf",
		destination,
		"--",
		...Object.keys(state),
	]);
	await assertSourceUnchanged(root, state);
}

async function main(args) {
	if (process.platform !== "win32" || process.arch !== "x64")
		throw new Error("Windows x64 packaging requires a Windows x64 host and the MSVC toolchain");
	if (args.some((arg) => arg !== "--internal-test"))
		throw new Error("Usage: npm run dist:windows -- [--internal-test]");
	const internal = args.includes("--internal-test");
	if (!internal) assertPublicDistribution();
	const source = await sourceState(root);
	for (const name of [
		"package.json",
		"package-lock.json",
		"LICENSE",
		".npmrc",
		"src-tauri/Cargo.toml",
		"src-tauri/Cargo.lock",
		"src-tauri/tauri.conf.json",
	])
		if (!source[name]) throw new Error(`Required build input is missing or ignored: ${name}`);
	await checkCompliance({ publicRelease: !internal });
	const pkg = JSON.parse(await readFile(join(root, "package.json")));
	const config = JSON.parse(await readFile(join(root, "src-tauri/tauri.conf.json")));
	const version = validateVersions(
		pkg.version,
		config.version,
		await readFile(join(root, "src-tauri/Cargo.toml"), "utf8"),
	);
	const ffmpegVersion = await verifyDependencies();
	const outputRoot = join(root, "release/windows-x64");
	await mkdir(outputRoot, { recursive: true });
	const output = await mkdtemp(
		join(outputRoot, `${version}-${internal ? "internal-test" : "release"}-`),
	);
	const appName = `Klipper-${version}-windows-x64${internal ? "-internal-test" : ""}`;
	const productName = internal ? "Klipper Internal Test" : config.productName;
	const app = join(output, appName);
	await mkdir(app);
	for (const [source, destination] of dependencyFiles)
		await copy(join(nativeRoot, source), join(app, destination));
	await copy(join(root, "LICENSE"), join(app, "LICENSE.txt"));
	await copy(
		join(root, "packaging/THIRD-PARTY-NOTICES.txt"),
		join(app, "THIRD-PARTY-NOTICES.txt"),
	);
	// Source and legal resources must be staged before Tauri creates the installer.
	await stageCompliance(join(app, "compliance"), { publicRelease: !internal });
	await mkdir(join(app, "sources"));
	const applicationSource = join(app, "sources/application-source.zip");
	await sourceSnapshot(applicationSource, source);
	await writeFile(
		join(app, "sources/source-files.json"),
		JSON.stringify(
			Object.fromEntries(Object.entries(source).map(([name, { sha256 }]) => [name, sha256])),
			null,
			"\t",
		) + "\n",
	);
	await writeFile(join(app, "FFmpeg-version.txt"), ffmpegVersion);
	await writeFile(
		join(app, "FFmpeg-buildconf.txt"),
		(await runVerified(
			join(nativeRoot, "ffmpeg/ffmpeg.exe"),
			pins.ffmpeg.ffmpegSha256,
			["-hide_banner", "-buildconf"],
			{
				stdio: ["ignore", "pipe", "pipe"],
			},
		)) || ffmpegVersion,
	);
	await writeFile(
		join(app, "dependency-provenance.json"),
		JSON.stringify({ mpv, ...pins }, null, 4),
	);
	await writeFile(
		join(app, "READ-ME-FIRST.txt"),
		`Klipper ${version} Windows x64\n${internal ? "INTERNAL TEST BUILD - NOT APPROVED FOR PUBLIC DISTRIBUTION.\n" : ""}Extract the entire directory, then run klipper.exe. Windows 10/11 x64, WebView2 and supported graphics drivers are required. The ZIP does not install WebView2; the NSIS installer offers its installation (Internet required if absent).\nNo developer tools or PATH media programs are required. Library/settings/cache use the normal Windows application-data directories. Closing Klipper cancels unfinished exports. Uninstalling does not remove recordings. This alpha is unsigned; Windows may display SmartScreen warnings.\nSee THIRD-PARTY-NOTICES.txt for unresolved public-release obligations.\n`,
	);
	// Official Tauri resource map, shared verbatim by installer and ZIP layouts.
	const resources = {};
	async function resourcesFrom(path, prefix = "") {
		for (const entry of await readdir(path, { withFileTypes: true })) {
			const destination = prefix + entry.name;
			if (entry.isDirectory()) await resourcesFrom(join(path, entry.name), `${destination}/`);
			else resources[join(path, entry.name)] = destination;
		}
	}
	await resourcesFrom(app);
	const overlay = join(output, "tauri-distribution.json");
	await writeFile(
		overlay,
		JSON.stringify(
			{
				...(internal
					? {
							productName,
							identifier: `${config.identifier}.packaging-test`,
							app: {
								windows: config.app.windows.map((window) =>
									window.label === "main"
										? {
												...window,
												title: "Klipper - Packaged alpha validation",
											}
										: window,
								),
							},
						}
					: {}),
				bundle: {
					active: true,
					useLocalToolsDir: true,
					targets: ["nsis"],
					publisher: "Klipper contributors",
					license: "GPL-3.0-only",
					resources,
					windows: {
						webviewInstallMode: { type: "downloadBootstrapper", silent: true },
						nsis: {
							installMode: "currentUser",
							displayLanguageSelector: false,
							languages: ["English"],
							compression: "lzma",
						},
					},
				},
			},
			null,
			4,
		),
	);
	await assertSourceUnchanged(root, source);
	await verifyInstallerTools();
	const cli = join(root, "node_modules/@tauri-apps/cli/tauri.js");
	run(
		process.execPath,
		[
			cli,
			"build",
			"--target",
			"x86_64-pc-windows-msvc",
			"--bundles",
			"nsis",
			"--config",
			overlay,
			"--",
			"--locked",
		],
		{
			timeout: 1_800_000,
			stdio: "inherit",
			// Keep Tauri's local tool cache and build output in the verified project location.
			env: { ...process.env, CARGO_TARGET_DIR: join(root, "src-tauri/target") },
		},
	);
	await assertSourceUnchanged(root, source);
	await verifyInstallerTools();
	const compiled = join(root, "src-tauri/target/x86_64-pc-windows-msvc/release");
	await copy(join(compiled, "klipper.exe"), join(app, "klipper.exe"));
	const binaries = await auditBinaries(app);
	await writeFile(join(output, "native-import-audit.json"), JSON.stringify(binaries, null, 4));
	const installerName = `${productName}_${version}_x64-setup.exe`;
	await copy(join(compiled, "bundle/nsis", installerName), join(output, `${appName}-setup.exe`));
	const zip = join(output, `${appName}.zip`);
	run(
		join(process.env.SystemRoot, "System32/tar.exe"),
		["-a", "-cf", zip, "-C", output, appName],
		{ timeout: 300_000 },
	);
	const sourceArchive = join(output, `${appName}-application-source.zip`);
	await copy(applicationSource, sourceArchive);
	const artifacts = [
		`${appName}.zip`,
		`${appName}-setup.exe`,
		`${appName}-application-source.zip`,
	];
	await writeFile(
		join(output, "SHA256SUMS.txt"),
		(
			await Promise.all(
				artifacts.map(async (file) => `${await checksum(join(output, file))}  ${file}`),
			)
		).join("\n") + "\n",
	);
	console.log(
		`\n${internal ? "INTERNAL TEST ONLY" : "Distribution"}: ${relative(root, output)}\n${artifacts.join("\n")}`,
	);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	main(process.argv.slice(2)).catch((error) => {
		console.error(error.message);
		process.exitCode = 1;
	});
}
