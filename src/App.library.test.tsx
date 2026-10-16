// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import App from "./App";
import * as library from "./services/library";
import * as media from "./services/media";

vi.mock("./services/library", async (original) => ({
	...(await original<typeof library>()),
	libraryStatus: vi.fn(),
	queryLibrary: vi.fn(),
	openLibraryRecording: vi.fn(),
}));
vi.mock("./services/media", () => ({
	selectRecording: vi.fn(),
	analyzeRecording: vi.fn(),
	playbackUrl: (recording: media.Recording) => "asset://synthetic-" + recording.id,
	errorMessage: String,
}));

const info: media.MediaInfo = {
	container: "mp4",
	videoCodec: "hevc",
	width: 1920,
	height: 1080,
	frameRate: 60,
	duration: 12,
	fileSize: 1024,
	bitrate: null,
	audio: [],
};
const recording = { id: 8, name: "Synthetic clip.mp4", path: "/synthetic/clip.mp4" };
beforeEach(() => {
	vi.mocked(library.libraryStatus).mockResolvedValue({
		roots: [],
		progress: {
			rootId: null,
			phase: "",
			discovered: 0,
			analyzed: 0,
			reused: 0,
			skippedLinks: 0,
			issueCount: 0,
			issues: [],
		},
	});
	vi.mocked(library.queryLibrary).mockResolvedValue({
		total: 1,
		limit: 40,
		items: [
			{
				...recording,
				id: 501,
				size: 1024,
				modifiedMs: null,
				status: "ready",
				availability: "available",
				duration: 12,
				videoCodec: "hevc",
				width: 1920,
				height: 1080,
				frameRate: 60,
				audioCount: 0,
				error: null,
			},
		],
	});
});
afterEach(() => {
	cleanup();
	vi.resetAllMocks();
	localStorage.clear();
});

it("starts in Library, opens through authorization and reuses cached analysis", async () => {
	vi.mocked(library.openLibraryRecording).mockResolvedValue({ recording, info });
	render(<App />);
	expect(screen.getByRole("button", { name: "Library" }).getAttribute("aria-current")).toBe(
		"page",
	);
	fireEvent.click(await screen.findByRole("button", { name: "Play " + recording.name }));
	expect(await screen.findByLabelText(recording.name)).toBeTruthy();
	expect(library.openLibraryRecording).toHaveBeenCalledWith(501);
	expect(media.analyzeRecording).not.toHaveBeenCalled();
	expect(screen.getByRole("button", { name: "Player" }).getAttribute("aria-current")).toBe(
		"page",
	);
	fireEvent.click(screen.getByRole("button", { name: "Library" }));
	expect(screen.queryByLabelText(recording.name)).toBeNull();
	expect(await screen.findByRole("button", { name: "Play " + recording.name })).toBeTruthy();
});

it("analyzes an uncached file using its new temporary selection ID", async () => {
	vi.mocked(library.openLibraryRecording).mockResolvedValue({ recording, info: null });
	vi.mocked(media.analyzeRecording).mockResolvedValue(info);
	render(<App />);
	fireEvent.click(await screen.findByRole("button", { name: "Play " + recording.name }));
	await screen.findByLabelText(recording.name);
	expect(media.analyzeRecording).toHaveBeenCalledWith(8);
});

it("does not let obsolete direct-file inspection overwrite a cached library selection", async () => {
	let finish!: (result: media.MediaInfo) => void;
	vi.mocked(media.selectRecording).mockResolvedValue({
		id: 1,
		name: "Old.mp4",
		path: "/synthetic/old.mp4",
	});
	vi.mocked(media.analyzeRecording).mockReturnValue(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	vi.mocked(library.openLibraryRecording).mockResolvedValue({ recording, info });
	render(<App />);
	fireEvent.click(screen.getByRole("button", { name: "Open recording" }));
	await screen.findByLabelText("Old.mp4");
	fireEvent.click(screen.getByRole("button", { name: "Library" }));
	fireEvent.click(await screen.findByRole("button", { name: "Play " + recording.name }));
	await screen.findByLabelText(recording.name);
	await act(async () => finish({ ...info, videoCodec: "h264" }));
	expect(screen.queryByText("H264")).toBeNull();
});

it("keeps Library usable after opening a missing recording fails", async () => {
	vi.mocked(library.openLibraryRecording).mockRejectedValue("Synthetic file is unavailable");
	render(<App />);
	fireEvent.click(await screen.findByRole("button", { name: "Play " + recording.name }));
	expect(await screen.findByRole("alert")).toHaveProperty(
		"textContent",
		"Synthetic file is unavailable",
	);
	await waitFor(() =>
		expect(
			screen.getByRole("button", { name: "Play " + recording.name }).hasAttribute("disabled"),
		).toBe(false),
	);
	expect(screen.getByRole("button", { name: "Library" }).getAttribute("aria-current")).toBe(
		"page",
	);
});
