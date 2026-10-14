import { expect, it } from "vitest";
import { audioLayout, containerLabel } from "./mediaLabels";

it("does not claim a specific format from ambiguous demuxer names", () => {
	expect(containerLabel("mov,mp4,m4a,3gp,3g2,mj2")).toBe("QuickTime / MPEG-4 family");
	expect(containerLabel("matroska,webm")).toBe("Matroska / WebM family");
	expect(containerLabel("mp4")).toBe("MPEG-4 (MP4)");
	expect(containerLabel(null)).toBe("Unknown");
});
it("keeps channel counts when layout is absent or unusual", () => {
	expect(audioLayout(2, "stereo")).toBe("Stereo");
	expect(audioLayout(6, "5.1")).toBe("6 ch (5.1)");
	expect(audioLayout(4, null)).toBe("4 ch");
});
