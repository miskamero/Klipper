// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import { analyzeRecording, selectRecording, type MediaInfo } from "./services/media";

vi.mock("./services/media", () => ({
	selectRecording: vi.fn(),
	analyzeRecording: vi.fn(),
	playbackUrl: (recording: { id: number }) => `asset://synthetic-${recording.id}`,
	errorMessage: (error: unknown) => String(error),
}));

afterEach(() => {
	cleanup();
	vi.resetAllMocks();
	localStorage.clear();
});

const first = {
	id: 1,
	name: "Synthetic A.mp4",
	path: "/synthetic/a.mp4",
};

const second = {
	id: 2,
	name: "Synthetic B.mp4",
	path: "/synthetic/b.mp4",
};

const info: MediaInfo = {
	container: "mp4",
	videoCodec: "h264",
	width: 96,
	height: 64,
	frameRate: 30,
	duration: 1,
	fileSize: 1024,
	bitrate: null,
	audio: [],
};

describe("recording workflow", () => {
	it("keeps the empty state when the picker is canceled", async () => {
		vi.mocked(selectRecording).mockResolvedValue(null);

		render(<App />);

		fireEvent.click(screen.getByRole("button", { name: /Open recording/ }));

		await waitFor(() => {
			expect(
				screen.getByRole("button", { name: /Open recording/ }).hasAttribute("disabled"),
			).toBe(false);
		});

		expect(screen.getByText("No recording selected")).toBeTruthy();
		expect(analyzeRecording).not.toHaveBeenCalled();
	});

	it("preserves playback when analysis fails and permits another selection", async () => {
		vi.mocked(selectRecording).mockResolvedValueOnce(first).mockResolvedValueOnce(second);

		vi.mocked(analyzeRecording)
			.mockRejectedValueOnce("FFprobe missing")
			.mockResolvedValueOnce(info);

		render(<App />);

		fireEvent.click(screen.getByRole("button", { name: /Open recording/ }));

		expect(await screen.findByText("FFprobe missing")).toBeTruthy();
		expect(screen.getByLabelText(first.name).getAttribute("src")).toContain("synthetic-1");

		fireEvent.click(screen.getByRole("button", { name: /Open another/ }));

		expect(await screen.findByText("H264")).toBeTruthy();
		expect(screen.queryByText("FFprobe missing")).toBeNull();
		expect(screen.getByLabelText(second.name)).toBeTruthy();
	});

	it("discards analysis from the previous recording after switching", async () => {
		let finishOld: (value: MediaInfo) => void = () => {};

		vi.mocked(selectRecording).mockResolvedValueOnce(first).mockResolvedValueOnce(second);

		vi.mocked(analyzeRecording)
			.mockImplementationOnce(
				() =>
					new Promise((resolve) => {
						finishOld = resolve;
					}),
			)
			.mockResolvedValueOnce({
				...info,
				videoCodec: "hevc",
			});

		render(<App />);

		fireEvent.click(screen.getByRole("button", { name: /Open recording/ }));

		await screen.findByText("Reading container and streams…");

		fireEvent.click(screen.getByRole("button", { name: /Open another/ }));

		await screen.findByText("HEVC");

		await act(async () => finishOld(info));

		expect(screen.queryByText("H264")).toBeNull();
		expect(screen.getByText("HEVC")).toBeTruthy();
	});

	it("reports a real media element error independently of probe results", async () => {
		vi.mocked(selectRecording).mockResolvedValue(first);
		vi.mocked(analyzeRecording).mockResolvedValue(info);

		render(<App />);

		fireEvent.click(screen.getByRole("button", { name: /Open recording/ }));

		const video = await screen.findByLabelText(first.name);

		fireEvent.error(video);

		expect(screen.getByText("Playback unavailable")).toBeTruthy();
		expect(await screen.findByText("H264")).toBeTruthy();
	});
});

it("keeps the selected player mounted while details are hidden and restored", async () => {
	vi.mocked(selectRecording).mockResolvedValue(first);
	vi.mocked(analyzeRecording).mockResolvedValue(info);

	render(<App />);

	fireEvent.click(screen.getByRole("button", { name: /Open recording/ }));

	const video = await screen.findByLabelText(first.name);

	fireEvent.click(screen.getByRole("button", { name: "Hide details" }));

	expect(screen.queryByRole("complementary")).toBeNull();
	expect(screen.getByLabelText(first.name)).toBe(video);

	fireEvent.click(screen.getByRole("button", { name: "Show details" }));

	expect(screen.getByRole("complementary")).toBeTruthy();
	expect(screen.getByLabelText(first.name)).toBe(video);
});

it("resizes the inspector with the keyboard and restores its saved width", () => {
	const { unmount } = render(<App />);

	let handle = screen.getByRole("separator", {
		name: "Resize recording details",
	});

	expect(handle.getAttribute("aria-valuenow")).toBe("400");

	fireEvent.keyDown(handle, { key: "ArrowLeft" });

	expect(handle.getAttribute("aria-valuenow")).toBe("416");
	expect(localStorage.getItem("klipper.inspectorWidth")).toBe("416");

	unmount();
	render(<App />);

	handle = screen.getByRole("separator", {
		name: "Resize recording details",
	});

	expect(handle.getAttribute("aria-valuenow")).toBe("416");

	fireEvent.keyDown(handle, { key: "End" });

	expect(handle.getAttribute("aria-valuenow")).toBe("560");

	fireEvent.keyDown(handle, { key: "Home" });

	expect(handle.getAttribute("aria-valuenow")).toBe("340");
});

it("puts recording identity in the inspector and keeps exact file information available", async () => {
	vi.mocked(selectRecording).mockResolvedValue({
		...first,
		name: "Long_gameplay_title.mp4",
	});

	vi.mocked(analyzeRecording).mockResolvedValue(info);

	render(<App />);

	fireEvent.click(screen.getByRole("button", { name: "Open recording" }));

	const title = await screen.findByRole("heading", {
		name: "Long gameplay title",
	});

	expect(title.closest("aside")).toBeTruthy();

	expect(screen.getByText("Long_gameplay_title.mp4").closest("details")).toBeTruthy();

	expect(screen.getByText(first.path)).toBeTruthy();
});

it("returns the inspector to recording identity and resets expanded file details after switching", async () => {
	vi.mocked(selectRecording).mockResolvedValueOnce(first).mockResolvedValueOnce(second);

	vi.mocked(analyzeRecording).mockResolvedValue(info);

	render(<App />);

	fireEvent.click(screen.getByRole("button", { name: "Open recording" }));

	await screen.findByLabelText(first.name);

	const region = screen.getByRole("region", {
		name: "Scrollable recording details",
	});

	const oldDetails = region.querySelector("details")!;

	oldDetails.open = true;
	region.scrollTop = 300;

	fireEvent.click(
		screen.getByRole("button", {
			name: "Open another recording",
		}),
	);

	await screen.findByLabelText(second.name);

	expect(region.scrollTop).toBe(0);
	expect(region.querySelector("details")!.open).toBe(false);
});
