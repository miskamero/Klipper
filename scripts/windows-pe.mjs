import { readFile } from "node:fs/promises";

// Inspect PE imports without loading or executing a downloaded binary.
export function inspectPe(bytes) {
	const fail = () => {
		throw new Error("Unsupported or malformed Windows x64 PE image");
	};
	if (bytes.length < 64 || bytes.toString("ascii", 0, 2) !== "MZ") fail();
	const pe = bytes.readUInt32LE(60);
	if (pe + 264 > bytes.length || bytes.readUInt32LE(pe) !== 0x4550) fail();
	if (bytes.readUInt16LE(pe + 4) !== 0x8664) fail();
	const optional = pe + 24;
	if (bytes.readUInt16LE(optional) !== 0x20b) fail();
	const sectionCount = bytes.readUInt16LE(pe + 6);
	const sectionStart = optional + bytes.readUInt16LE(pe + 20);
	const offset = (rva, size = 1) => {
		for (let index = 0; index < sectionCount; index++) {
			const section = sectionStart + index * 40;
			if (section + 40 > bytes.length) fail();
			const virtual = bytes.readUInt32LE(section + 12);
			const length = bytes.readUInt32LE(section + 16);
			const raw = bytes.readUInt32LE(section + 20);
			if (
				rva >= virtual &&
				rva - virtual + size <= length &&
				raw + rva - virtual + size <= bytes.length
			) {
				return raw + rva - virtual;
			}
		}
		fail();
	};
	const name = (rva) => {
		const start = offset(rva);
		const end = bytes.indexOf(0, start);
		if (end < start || end - start > 255) fail();
		const value = bytes.toString("ascii", start, end).toLowerCase();
		if (!/^[a-z0-9_.-]+\.dll$/.test(value)) fail();
		return value;
	};
	const imports = new Set();
	for (const [directory, stride, field] of [
		[1, 20, 12],
		[13, 32, 4],
	]) {
		const rva = bytes.readUInt32LE(optional + 112 + directory * 8);
		const size = bytes.readUInt32LE(optional + 116 + directory * 8);
		if (!rva || !size) continue;
		for (let index = 0; index < 4096; index++) {
			const record = offset(rva + index * stride, stride);
			if (bytes.subarray(record, record + stride).every((byte) => byte === 0)) break;
			// Modern PE32+ delay imports use RVA-based descriptors.
			if (directory === 13 && bytes.readUInt32LE(record) !== 1) fail();
			imports.add(name(bytes.readUInt32LE(record + field)));
			if (index === 4095) fail();
		}
	}
	return { machine: "x64", imports: [...imports].sort() };
}

export async function inspectFile(path) {
	return inspectPe(await readFile(path));
}
