import { describe, expect, it } from "vitest";
import { bytes, playbackError, time } from "./format";

describe("media display boundaries", () => {
	it("handles unavailable durations and long recordings", () => {
		expect(time(NaN)).toBe("—");
		expect(time(Infinity)).toBe("—");
		expect(time(null)).toBe("—");
		expect(time(-1)).toBe("—");
		expect(time(0)).toBe("00:00");
		expect(time(3661.9)).toBe("1:01:01");
	});
	it("formats byte units without invalid numbers", () => {
		expect(bytes(0)).toBe("0 B");
		expect(bytes(1024 ** 3)).toBe("1.0 GiB");
		expect(bytes(NaN)).toBe("—");
	});
	it("distinguishes access failures from unsupported codecs", () => {
		expect(playbackError(2)).toContain("read");
		expect(playbackError(3)).toContain("decode");
		expect(playbackError(4)).toContain("HEVC");
	});
});
