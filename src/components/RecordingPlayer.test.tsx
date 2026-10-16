// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { getPlaybackBackend } from "../services/playback";
import type { MediaInfo } from "../services/media";
import RecordingPlayer from "./RecordingPlayer";

vi.mock("../services/playback", () => ({ getPlaybackBackend: vi.fn() }));
vi.mock("../services/media", () => ({
	playbackUrl: () => "asset://synthetic",
	errorMessage: (error: unknown) => (error instanceof Error ? error.message : String(error)),
}));
vi.mock("./native/NativePlayer", () => ({
	default: ({ id }: { id: number }) => <div>Native recording {id}</div>,
}));
const recording = { id: 7, name: "Synthetic HEVC.mp4", path: "/synthetic/hevc.mp4" };
const info: MediaInfo = {
	container: "mp4",
	videoCodec: "hevc",
	width: 96,
	height: 64,
	frameRate: 30,
	duration: 1,
	fileSize: 1024,
	bitrate: null,
	audio: [],
};
afterEach(() => {
	cleanup();
	vi.resetAllMocks();
});

it("chooses native playback from Rust without an experimental URL", async () => {
	vi.mocked(getPlaybackBackend).mockResolvedValue({ engine: "native", message: null });
	render(<RecordingPlayer recording={recording} info={info} selecting={false} />);
	await screen.findByText("Native recording 7");
	expect(document.querySelector("video")).toBeNull();
});

it("shows missing native dependencies and retries without silently using HTML for HEVC", async () => {
	vi.mocked(getPlaybackBackend)
		.mockResolvedValueOnce({
			engine: "unavailable",
			message: "Place libmpv-2.dll in the local development directory.",
		})
		.mockResolvedValueOnce({ engine: "native", message: null });
	render(<RecordingPlayer recording={recording} info={info} selecting={false} />);
	await screen.findByRole("alert");
	expect(document.querySelector("video")).toBeNull();
	expect(screen.queryByText("Try system playback")).toBeNull();
	fireEvent.click(screen.getByRole("button", { name: "Retry native playback" }));
	await screen.findByText("Native recording 7");
});

it("retains system playback on supported fallback environments with an HEVC limitation notice", async () => {
	vi.mocked(getPlaybackBackend).mockResolvedValue({ engine: "html", message: null });
	render(<RecordingPlayer recording={recording} info={info} selecting={false} />);
	await screen.findByLabelText(recording.name);
	expect(screen.getByText(/System playback: HEVC support depends/)).toBeTruthy();
});

it("requires an explicit choice for non-HEVC fallback after a native availability failure", async () => {
	vi.mocked(getPlaybackBackend).mockResolvedValue({
		engine: "unavailable",
		message: "Missing DLL",
	});
	render(
		<RecordingPlayer
			recording={recording}
			info={{ ...info, videoCodec: "h264" }}
			selecting={false}
		/>,
	);
	await screen.findByRole("alert");
	expect(document.querySelector("video")).toBeNull();
	fireEvent.click(screen.getByRole("button", { name: "Try system playback" }));
	await screen.findByLabelText(recording.name);
});

it("reports backend IPC failures rather than guessing that native playback is unavailable", async () => {
	vi.mocked(getPlaybackBackend).mockRejectedValue(new Error("Disconnected"));
	render(<RecordingPlayer recording={recording} info={info} selecting={false} />);
	await screen.findByText(/Playback backend could not be checked: Disconnected/);
	expect(document.querySelector("video")).toBeNull();
});
