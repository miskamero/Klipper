// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import EditorPanel from "./EditorPanel";
import {
	clampRange,
	parseTime,
	preciseTime,
	type EditProject,
	type EditorBridge,
} from "../../services/editor";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const project: EditProject = {
	output: { container: "mp4", videoCodec: "h264", audioCodec: "aac", crf: 18 },
	version: 1,
	token: 1,
	recordingId: 7,
	revision: "synthetic",
	start: 0,
	end: 4,
	info: {
		duration: 4,
		origin: 0,
		videoIndex: 0,
		codec: "hevc",
		width: 160,
		height: 96,
		timeBase: "1/15360",
		audio: [1, 2, 3, 4].map((index) => ({
			index,
			codec: "aac",
			channels: 1,
			sampleRate: 48000,
		})),
	},
};
afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
	vi.clearAllMocks();
});
function setup() {
	vi.mocked(invoke).mockImplementation(async (name, args) => {
		if (name === "editor_frame") return new ArrayBuffer(0);
		if (name === "editor_range")
			return { ...project, ...(args as { selection: object }).selection };
		if (name === "editor_destination") return "C:/synthetic/new.mp4";
		if (name === "editor_export") return 21;
		if (name === "export_encoders") return { nvenc: true, detail: "Synthetic capability" };
	});
	const command = vi.fn();
	const bridge = { current: { command } as EditorBridge };
	const state = vi.fn();
	const open = vi.fn();
	render(<EditorPanel project={project} bridge={bridge} onState={state} onQueued={open} />);
	return { command, bridge, state, open };
}
it("parses precise inputs and clamps nonempty ranges without assuming a frame rate", () => {
	expect(parseTime("01:02:03.125")).toBe(3723.125);
	expect(parseTime("3.123456")).toBe(3.123456);
	expect(parseTime("01:99:00")).toBeNull();
	expect(preciseTime(3723.125)).toBe("01:02:03.125");
	expect(clampRange(5, 4, 4)).toEqual({ start: 3.999, end: 4 });
});
it("aligns typed boundaries natively before preview and exports only to a selected destination", async () => {
	const { command, state, open } = setup();
	expect(screen.getByRole("button", { name: "Set start here" })).toBeTruthy();
	expect(screen.getByRole("button", { name: "Set end here" })).toBeTruthy();
	expect((screen.getByText("Export new video") as HTMLButtonElement).disabled).toBe(true);
	fireEvent.change(screen.getByLabelText("Trim start time"), { target: { value: "0.25" } });
	fireEvent.blur(screen.getByLabelText("Trim start time"));
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("editor_range", {
			token: 1,
			selection: { start: 0.25, end: 4 },
		}),
	);
	await waitFor(() => expect(command).toHaveBeenCalledWith({ kind: "seek", value: 0.25 }));
	expect(state).toHaveBeenCalledWith({ dirty: true, busy: false });
	expect(screen.queryByText("Preview selection")).toBeNull();
	expect(command).toHaveBeenCalledWith({ kind: "previewRange", value: [0.25, 4] });
	fireEvent.click(screen.getByText("Save as…"));
	await screen.findByText(/Destination: C:/);
	fireEvent.click(screen.getByText("Export new video"));
	await waitFor(() =>
		expect(invoke).toHaveBeenCalledWith("editor_export", { token: 1, encoder: "auto" }),
	);
	expect(open).toHaveBeenCalledWith(21);
	expect(vi.mocked(invoke).mock.calls.some(([name]) => name === "editor_status")).toBe(false);
});
it("supports keyboard handles, separate frame controls and native playhead feedback", async () => {
	const { command, bridge } = setup();
	fireEvent.keyDown(screen.getByRole("slider", { name: "Trim start" }), {
		key: "ArrowRight",
		shiftKey: true,
	});
	fireEvent.keyUp(screen.getByRole("slider", { name: "Trim start" }), { key: "ArrowRight" });
	await waitFor(() => expect(command).toHaveBeenCalledWith({ kind: "seek", value: 1 }));
	fireEvent.click(screen.getByText("Frame →"));
	expect(command).toHaveBeenCalledWith({ kind: "frameStep", value: true });
	act(() =>
		bridge.current.report?.({
			id: 7,
			position: 2.125,
			duration: 4,
			speed: 1,
			volume: 100,
			muted: false,
			paused: true,
			hwdec: "d3d11va",
			dropped: 0,
			error: "",
			fullscreen: false,
		}),
	);
	expect((screen.getByLabelText("Timeline playhead") as HTMLInputElement).value).toBe("2.125");
});
it("bounds storyboard extraction to eight samples and stops requesting after unmount", async () => {
	setup();
	await waitFor(() =>
		expect(
			vi.mocked(invoke).mock.calls.filter(([name]) => name === "editor_frame"),
		).toHaveLength(8),
	);
	cleanup();
	await Promise.resolve();
	expect(vi.mocked(invoke).mock.calls.filter(([name]) => name === "editor_frame")).toHaveLength(
		8,
	);
});
it("keeps genuine export failures visible without presenting a success action", async () => {
	setup();
	fireEvent.click(screen.getByText("Save as…"));
	await screen.findByText(/Destination:/);
	vi.mocked(invoke).mockRejectedValueOnce("Output filename is reserved");
	fireEvent.click(screen.getByText("Export new video"));
	await screen.findByText("Output filename is reserved");
	expect(screen.queryByText("Open exported recording")).toBeNull();
});
