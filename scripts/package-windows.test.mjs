import { test } from "node:test";
import assert from "node:assert/strict";
import { inspectPe } from "./windows-pe.mjs";
import { unexpectedImports, validateNpmConfig, validateVersions } from "./package-windows.mjs";
import { assertPublicDistribution, dependencyFiles } from "./windows-deps.mjs";

function syntheticPe() {
	const bytes = Buffer.alloc(2048);
	bytes.write("MZ");
	bytes.writeUInt32LE(128, 60);
	bytes.writeUInt32LE(0x4550, 128);
	bytes.writeUInt16LE(0x8664, 132);
	bytes.writeUInt16LE(1, 134);
	bytes.writeUInt16LE(240, 148);
	bytes.writeUInt16LE(0x20b, 152);
	bytes.writeUInt32LE(0x1000, 272);
	bytes.writeUInt32LE(40, 276);
	bytes.writeUInt32LE(0x1000, 404);
	bytes.writeUInt32LE(1024, 408);
	bytes.writeUInt32LE(512, 412);
	bytes.writeUInt32LE(0x1080, 524);
	bytes.write("KERNEL32.dll\0", 640);
	return bytes;
}

test("reads x64 PE import names without executing the image", () => {
	assert.deepEqual(inspectPe(syntheticPe()), { machine: "x64", imports: ["kernel32.dll"] });
	const invalid = syntheticPe();
	invalid.writeUInt16LE(0x14c, 132);
	assert.throws(() => inspectPe(invalid), /Windows x64/);
	assert.throws(() => inspectPe(Buffer.from("not an executable")), /malformed/);
});

test("rejects unresolved runtime DLLs instead of assuming the developer's installed runtime", () => {
	assert.deepEqual(
		unexpectedImports(["kernel32.dll", "vulkan-1.dll", "vcruntime140.dll"], ["vulkan-1.dll"]),
		["vcruntime140.dll"],
	);
	assert.deepEqual(unexpectedImports(["api-ms-win-crt-stdio-l1-1-0.dll"], []), []);
	assert.deepEqual(unexpectedImports(["vulkan-1.dll"], []), ["vulkan-1.dll"]);
});

test("rejects inconsistent release versions and preserves the explicit redistribution gate", () => {
	assert.equal(validateVersions("0.1.0", "0.1.0", '[package]\nversion = "0.1.0"\n'), "0.1.0");
	assert.throws(() => validateVersions("0.1.0", "0.2.0", 'version = "0.1.0"'), /must agree/);
	assert.throws(assertPublicDistribution, /Public distribution is blocked/);
});

test("runtime resource list contains only fixed, checksum-pinned tools and licenses", () => {
	assert.deepEqual(
		dependencyFiles.map(([, destination]) => destination),
		[
			"libmpv-2.dll",
			"bin/ffmpeg.exe",
			"bin/ffprobe.exe",
			"vulkan-1.dll",
			"licenses/FFmpeg-GPL.txt",
			"licenses/VulkanRT-License.txt",
		],
	);
	for (const [source, destination, hash] of dependencyFiles) {
		assert.match(hash, /^[a-f0-9]{64}$/);
		assert.ok(!source.includes("..") && !destination.includes(".."));
	}
});

test("source distribution includes the build's npm strategy without accepting private registry secrets", () => {
	assert.doesNotThrow(() => validateNpmConfig("# Build requirement\ninstall-strategy=nested\n"));
	assert.throws(
		() => validateNpmConfig("//registry.example/:_authToken=synthetic-secret"),
		/credentials/,
	);
});
