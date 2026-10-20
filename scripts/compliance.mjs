import { createHash } from "node:crypto";
import {
	copyFile,
	lstat,
	mkdir,
	mkdtemp,
	open,
	readFile,
	readdir,
	rename,
	rmdir,
	rm,
	writeFile,
} from "node:fs/promises";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";
import { checksum, install, verify } from "./native-deps.mjs";
import {
	assertPublicDistribution as assertReviewed,
	dependencyFiles,
	nativeRoot,
	root,
	run,
	verifyDependencies,
} from "./windows-deps.mjs";
import { toolsDirectory, verifyInstallerTools } from "./installer-tools.mjs";

export { assertReviewed };

export const bundleRoot = join(root, "native-deps/compliance-bundle");
const policyPath = join(root, "packaging/compliance.json");
const bindingPaths = [
	"packaging/compliance.json",
	"packaging/THIRD-PARTY-NOTICES.txt",
	"scripts/libmpv.json",
	"scripts/windows-native.json",
	"scripts/installer-tools.json",
	"scripts/installer-tools.mjs",
	"scripts/compliance.mjs",
	"package.json",
	"package-lock.json",
	".npmrc",
	"src-tauri/Cargo.toml",
	"src-tauri/Cargo.lock",
];
const legalName = /^(licen[cs]e|copying|copyright|notice|unlicense)([._-]|$)/i;
const reviewedLicenseIds = new Set([
	"0BSD",
	"MIT",
	"Apache-2.0",
	"BSD-2-Clause",
	"BSD-3-Clause",
	"ISC",
	"MPL-2.0",
	"Unlicense",
	"CC0-1.0",
	"MIT-0",
	"Zlib",
	"Unicode-3.0",
]);
const tar = join(process.env.SystemRoot || "C:/Windows", "System32/tar.exe");

export function contained(base, name) {
	if (!name || isAbsolute(name) || name.includes("\\") || name.includes(":"))
		throw new Error("Unsafe compliance path");
	const result = resolve(base, name);
	const rel = relative(base, result);
	if (!rel || rel === ".." || rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`))
		throw new Error("Unsafe compliance path");
	return result;
}

async function json(path) {
	return JSON.parse(await readFile(path, "utf8"));
}

async function save(path, value) {
	await mkdir(dirname(path), { recursive: true });
	await writeFile(path, JSON.stringify(value, null, "\t") + "\n");
}

async function files(directory) {
	const result = [];
	async function visit(current, prefix = "") {
		if ((await lstat(current)).isSymbolicLink()) throw new Error("Linked compliance directory");
		for (const entry of await readdir(current, { withFileTypes: true })) {
			const name = prefix + entry.name;
			if (entry.isSymbolicLink()) throw new Error("Linked compliance file");
			if (entry.isDirectory()) await visit(join(current, entry.name), `${name}/`);
			else if (entry.isFile()) result.push(name);
			else throw new Error("Unsupported compliance file");
		}
	}
	await visit(directory);
	return result.sort();
}

async function copy(source, destination) {
	if (!(await lstat(source)).isFile()) throw new Error("Expected a regular compliance file");
	await install(source, destination, await checksum(source));
}

async function download(url, destination, expected, algorithm = "sha256") {
	async function check() {
		const actual = createHash(algorithm)
			.update(await readFile(destination))
			.digest("base64");
		if (actual !== expected)
			throw new Error(`Source archive integrity mismatch: ${basename(destination)}`);
	}
	try {
		await check();
		return;
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
	if (!url.startsWith("https://")) throw new Error("Source downloads require HTTPS");
	const response = await fetch(url, { signal: AbortSignal.timeout(180_000) });
	if (!response.ok || !response.body || !response.url.startsWith("https://"))
		throw new Error(`Source download failed: HTTP ${response.status}`);
	await mkdir(dirname(destination), { recursive: true });
	let bytes = 0;
	const limit = new Transform({
		transform(chunk, encoding, callback) {
			bytes += chunk.length;
			callback(bytes > 128 * 1024 * 1024 ? new Error("Source exceeds 128 MiB") : null, chunk);
		},
	});
	const output = await open(destination, "wx");
	try {
		await pipeline(Readable.fromWeb(response.body), limit, output.createWriteStream());
		await check();
	} catch (error) {
		// Only this invocation's exclusive, owned partial download is removed.
		await rm(destination, { force: true });
		throw error;
	}
}

export function registryPackages(metadata) {
	const ids = new Set([metadata.resolve.root]);
	for (let changed = true; changed;) {
		changed = false;
		for (const node of metadata.resolve.nodes) {
			if (!ids.has(node.id)) continue;
			for (const dep of node.deps) {
				if (dep.dep_kinds.some((kind) => kind.kind !== "dev") && !ids.has(dep.pkg)) {
					ids.add(dep.pkg);
					changed = true;
				}
			}
		}
	}
	return metadata.packages.filter((pkg) => ids.has(pkg.id) && pkg.source);
}

async function collectRust(working, temporary, inventory) {
	const metadata = JSON.parse(
		run("cargo", [
			"metadata",
			"--manifest-path",
			"src-tauri/Cargo.toml",
			"--locked",
			"--offline",
			"--filter-platform",
			"x86_64-pc-windows-msvc",
			"--format-version",
			"1",
		]),
	);
	const lock = (await readFile(join(root, "src-tauri/Cargo.lock"), "utf8")).replaceAll(
		"\r\n",
		"\n",
	);
	const packages = registryPackages(metadata);
	for (const pkg of packages) {
		if (
			!pkg.license ||
			pkg.license
				.split(/\bOR\b|\bAND\b|[/()]/)
				.map((id) => id.trim())
				.filter(Boolean)
				.some((id) => !reviewedLicenseIds.has(id))
		)
			throw new Error(`Unreviewed Rust license expression: ${pkg.name}`);
		if (!pkg.source.startsWith("registry+https://github.com/rust-lang/crates.io-index"))
			throw new Error("Unreviewed Rust source registry");
		const entry = lock
			.split("[[package]]")
			.find(
				(block) =>
					block.includes(`name = "${pkg.name}"\n`) &&
					block.includes(`version = "${pkg.version}"\n`),
			);
		const hash = entry?.match(/checksum = "([a-f0-9]{64})"/)?.[1];
		const source = dirname(pkg.manifest_path);
		const registry = dirname(source);
		const archive = `${pkg.name}-${pkg.version}.crate`;
		const cached = join(registry, "../../cache", basename(registry), archive);
		if (!hash) throw new Error(`Rust lock/source mismatch: ${pkg.name}`);
		await verify(cached, hash);
		// Compare the compiled registry files with the checksum-verified original crate.
		// Newer Cargo caches do not necessarily contain .cargo-checksum.json.
		const extracted = join(temporary, `crate-${pkg.name}-${pkg.version}`);
		await mkdir(extracted);
		run(tar, ["-xf", cached, "-C", extracted]);
		const original = join(extracted, `${pkg.name}-${pkg.version}`);
		for (const name of await files(original)) {
			await verify(contained(source, name), await checksum(join(original, name)));
			if (
				pkg.license === "MPL-2.0" &&
				!legalName.test(basename(name)) &&
				(await readFile(join(original, name), "utf8")).includes(
					'This Source Code Form is "Incompatible With Secondary Licenses"',
				)
			)
				throw new Error(`MPL secondary-license exclusion requires review: ${pkg.name}`);
		}
		await copy(cached, join(working, "sources/rust", archive));
		const notices = [];
		for (const name of await files(source)) {
			if (legalName.test(basename(name))) {
				await copy(
					join(source, name),
					join(working, "licenses/rust", `${pkg.name}-${pkg.version}`, name),
				);
				notices.push(name);
			}
		}
		inventory.rust.push({
			name: pkg.name,
			version: pkg.version,
			license: pkg.license,
			repository: pkg.repository,
			source: `https://static.crates.io/crates/${pkg.name}/${archive}`,
			sha256: hash,
			archive: `sources/rust/${archive}`,
			notices,
		});
		if (inventory.rust.length % 50 === 0)
			console.log(
				`Verified ${inventory.rust.length}/${packages.length} Rust source archives and installed files.`,
			);
	}
	const sysroot = run("rustc", ["--print", "sysroot"]).trim();
	const docs = join(sysroot, "share/doc/rust");
	for (const name of [
		"COPYRIGHT.html",
		"COPYRIGHT-library.html",
		...(await files(join(docs, "licenses"))).map((name) => `licenses/${name}`),
	])
		await copy(join(docs, name), join(working, "licenses/rust-toolchain", name));
	inventory.rustToolchain = run("rustc", ["-vV"]).trim();
}

export function frontendPackages(lock) {
	const result = new Set();
	const pending = [["", Object.keys(lock.packages[""].dependencies || {})]];
	while (pending.length) {
		const [parent, names] = pending.pop();
		for (const name of names) {
			let prefix = parent;
			let found;
			while (true) {
				const candidate = `${prefix ? `${prefix}/` : ""}node_modules/${name}`;
				if (lock.packages[candidate]) {
					found = candidate;
					break;
				}
				if (!prefix) throw new Error(`Unresolved frontend dependency: ${name}`);
				prefix = prefix.includes("/") ? prefix.slice(0, prefix.lastIndexOf("/")) : "";
			}
			if (result.has(found)) continue;
			result.add(found);
			pending.push([found, Object.keys(lock.packages[found].dependencies || {})]);
		}
	}
	return [...result].sort();
}

async function collectNpm(working, temporary, inventory) {
	const lock = await json(join(root, "package-lock.json"));
	const runtime = frontendPackages(lock);
	const names = [
		...new Set([...runtime, "node_modules/vite", "node_modules/vite/node_modules/rolldown"]),
	];
	for (const name of names) {
		const pkg = lock.packages[name];
		const [algorithm, expected] = pkg.integrity.split("-");
		if (algorithm !== "sha512" || !pkg.resolved.startsWith("https://registry.npmjs.org/"))
			throw new Error("Unreviewed npm source/integrity");
		const id = `${name.split("node_modules/").at(-1).replaceAll("/", "-")}-${pkg.version}`;
		const archive = join(root, "native-deps/compliance-inputs", `${id}.tgz`);
		await download(pkg.resolved, archive, expected, algorithm);
		const extracted = join(temporary, id);
		await mkdir(extracted);
		run(tar, ["-xf", archive, "-C", extracted]);
		const packageRoot = join(extracted, "package");
		const notices = [];
		for (const file of await files(packageRoot)) {
			await verify(join(root, name, file), await checksum(join(packageRoot, file)));
			if (legalName.test(basename(file))) {
				await copy(join(packageRoot, file), join(working, "licenses/npm", id, file));
				notices.push(file);
			}
		}
		if (!notices.length) throw new Error(`Missing npm license: ${id}`);
		await copy(archive, join(working, "sources/npm", `${id}.tgz`));
		inventory.npm.push({
			role: runtime.includes(name) ? "frontend-runtime" : "build-helper-notices",
			name: (await json(join(packageRoot, "package.json"))).name,
			version: pkg.version,
			license: pkg.license,
			source: pkg.resolved,
			integrity: pkg.integrity,
			archive: `sources/npm/${id}.tgz`,
			notices,
		});
	}
}

async function bindings() {
	const result = {};
	for (const path of bindingPaths) result[path] = await checksum(join(root, path));
	for (const [source, , hash] of dependencyFiles) {
		await verify(join(nativeRoot, source), hash);
		result[`native/${source}`] = hash;
	}
	return result;
}

export async function prepareCompliance() {
	const policy = await json(policyPath);
	// Authenticate native tools before any version/configuration command executes.
	const ffmpegVersion = await verifyDependencies();
	await verifyInstallerTools();
	await mkdir(join(root, "native-deps"), { recursive: true });
	const temporary = await mkdtemp(join(root, "native-deps/compliance-work-"));
	if (dirname(temporary) !== resolve(root, "native-deps"))
		throw new Error("Unsafe work directory");
	const working = join(temporary, "bundle");
	await mkdir(working);
	try {
		const inventory = {
			schemaVersion: 1,
			review: policy.review,
			nativeSources: policy.sources,
			rust: [],
			npm: [],
			installerInputs: policy.installerInputs,
		};
		inventory.mediaBuild = {
			ffmpegVersion,
			configuration: ffmpegVersion
				.split("\n")
				.find((line) => line.startsWith("configuration:")),
			enabledSwitches: [...ffmpegVersion.matchAll(/--enable-([a-z0-9_-]+)/g)].map(
				(match) => match[1],
			),
			completeness:
				"Configuration identifies enabled features/libraries, not exact static dependency versions or all transitive components. Those sources, notices and builder inputs remain public-release blockers.",
			mpvBuilderRecipes:
				"sources/upstream-evidence/mpv-builder-1f8d2e7.zip contains packages/mpv.cmake, packages/ffmpeg.cmake, dependency recipes, patches, toolchain definitions and workflow. Dependency revisions are not fully locked.",
		};
		for (const pin of policy.sources) {
			const archive = join(root, "native-deps/compliance-inputs", pin.file);
			await download(pin.url, archive, Buffer.from(pin.sha256, "hex").toString("base64"));
			await copy(archive, join(working, "sources/upstream-evidence", pin.file));
			if (!pin.notices.length) continue;
			run(tar, [
				"-xf",
				archive,
				"-C",
				temporary,
				...pin.notices.map((name) => `${pin.archiveRoot}/${name}`),
			]);
			for (const name of pin.notices)
				await copy(
					join(temporary, pin.archiveRoot, name),
					join(working, "licenses", pin.id, name),
				);
		}
		await collectRust(working, temporary, inventory);
		await collectNpm(working, temporary, inventory);
		const nsis = join(toolsDirectory, "NSIS");
		for (const [file, hash] of policy.installerInputs) await verify(join(nsis, file), hash);
		await copy(join(nsis, "COPYING"), join(working, "licenses/nsis/BINARY-COPYING.txt"));
		await copy(
			join(root, "packaging/THIRD-PARTY-NOTICES.txt"),
			join(working, "THIRD-PARTY-NOTICES.txt"),
		);
		await save(join(working, "inventory.json"), inventory);
		const manifest = {
			schemaVersion: 1,
			bindings: await bindings(),
			rustToolchain: inventory.rustToolchain,
			files: {},
		};
		for (const name of await files(working))
			manifest.files[name] = await checksum(join(working, name));
		await refreshBundle(working, bundleRoot, manifest);
		console.log(
			`Collected ${inventory.rust.length} Rust crates, ${inventory.npm.length} frontend packages and ${policy.sources.length} upstream source archives. Public-release blockers remain explicit.`,
		);
	} finally {
		// mkdtemp is created directly inside native-deps, not derived from provider/user paths.
		await rm(temporary, { recursive: true, force: true });
	}
}

export async function refreshBundle(working, destination, manifest) {
	await save(join(working, "manifest.json"), manifest);
	await verifyBundle(working, manifest);
	let previous;
	try {
		await lstat(destination);
		// Ownership requires both the previous inventory and unchanged file contents.
		previous = await json(join(destination, "manifest.json"));
		await verifyBundle(destination, previous);
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
		try {
			await lstat(destination);
			throw new Error("Unowned compliance bundle; review before refreshing", {
				cause: error,
			});
		} catch (error) {
			if (error.code !== "ENOENT") throw error;
		}
	}
	if (!previous) {
		await rename(working, destination);
		return;
	}
	const backup = await mkdtemp(join(dirname(destination), "compliance-previous-"));
	if (dirname(backup) !== resolve(dirname(destination)))
		throw new Error("Unsafe backup directory");
	const old = join(backup, "bundle");
	await rename(destination, old);
	try {
		await rename(working, destination);
	} catch (error) {
		await rename(old, destination);
		await rmdir(backup);
		throw error;
	}
	// Do not recursively delete even the old bundle: remove only proven owned files.
	// An interrupted cleanup leaves a separately named backup, never an incomplete new bundle.
	const directories = new Set();
	for (const name of [...Object.keys(previous.files), "manifest.json"]) {
		const path = contained(old, name);
		if (name !== "manifest.json") await verify(path, previous.files[name]);
		await rm(path);
		for (let parent = dirname(path); parent !== old; parent = dirname(parent))
			directories.add(parent);
	}
	for (const directory of [...directories].sort((a, b) => b.length - a.length))
		await rmdir(directory);
	try {
		await rmdir(old);
		await rmdir(backup);
	} catch (error) {
		if (error.code !== "ENOTEMPTY") throw error;
		console.warn(
			"Preserved unowned empty directories in the previous compliance bundle:",
			backup,
		);
	}
}

export async function verifyBundle(directory, manifest) {
	if (manifest.schemaVersion !== 1 || !manifest.files || !Object.keys(manifest.files).length)
		throw new Error("Missing compliance file inventory");
	const actual = (await files(directory)).filter((name) => name !== "manifest.json");
	if (JSON.stringify(actual) !== JSON.stringify(Object.keys(manifest.files).sort()))
		throw new Error("Unexpected or missing compliance artifacts");
	for (const [name, hash] of Object.entries(manifest.files))
		await verify(contained(directory, name), hash);
}

export async function checkCompliance({ publicRelease = true } = {}) {
	const policy = await json(policyPath);
	if (publicRelease) assertReviewed(policy.review);
	const manifest = await json(join(bundleRoot, "manifest.json"));
	if (JSON.stringify(manifest.bindings) !== JSON.stringify(await bindings()))
		throw new Error("Compliance inputs changed; rerun npm run dist:licenses");
	if (manifest.rustToolchain !== run("rustc", ["-vV"]).trim())
		throw new Error("Rust toolchain changed; recollect its notices");
	await verifyInstallerTools();
	await verifyBundle(bundleRoot, manifest);
	return manifest;
}

export async function stageCompliance(destination, options) {
	const manifest = await checkCompliance(options);
	for (const name of [...Object.keys(manifest.files), "manifest.json"]) {
		const target = contained(destination, name);
		await mkdir(dirname(target), { recursive: true });
		await copyFile(contained(bundleRoot, name), target, 1);
	}
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
	const command = process.argv[2];
	const args = process.argv.slice(3);
	const action =
		command === "prepare" && !args.length
			? prepareCompliance
			: command === "check" && args.every((arg) => arg === "--internal-test")
				? () => checkCompliance({ publicRelease: !args.includes("--internal-test") })
				: null;
	if (!action)
		throw new Error("Usage: node scripts/compliance.mjs prepare|check [--internal-test]");
	action().catch((error) => {
		console.error(error.message);
		process.exitCode = 1;
	});
}
