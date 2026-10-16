// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import useRecordingWorkspaces from "./useRecordingWorkspaces";
import * as media from "./services/media";
import { openLibraryRecording } from "./services/library";
vi.mock("./services/media", () => ({
	selectRecording: vi.fn(),
	analyzeRecording: vi.fn(),
	activateRecording: vi.fn(),
	deactivateRecording: vi.fn(),
	errorMessage: String,
}));
vi.mock("./services/library", () => ({ openLibraryRecording: vi.fn() }));
const one = { id: 1, name: "One-off.mp4", path: "/synthetic/outside.mp4" };
const two = { id: 2, name: "Library.mp4", path: "/synthetic/indexed.mp4" };
beforeEach(() => {
	vi.mocked(media.selectRecording).mockResolvedValue(one);
	vi.mocked(media.analyzeRecording).mockResolvedValue(null);
	vi.mocked(media.deactivateRecording).mockResolvedValue(undefined);
	vi.mocked(media.activateRecording).mockResolvedValue({ ...one, id: 3 });
	vi.mocked(openLibraryRecording).mockResolvedValue({ recording: two, info: null });
});
afterEach(async () => {
	cleanup();
	for (let i = 0; i < 8; i++) await Promise.resolve();
	vi.resetAllMocks();
});

it("serializes owner cleanup and restores standalone state paused even before its first progress event", async () => {
	const { result } = renderHook(useRecordingWorkspaces);
	await act(async () => result.current.open());
	await waitFor(() => expect(result.current.current?.recording.id).toBe(1));
	let finish!: () => void;
	vi.mocked(media.deactivateRecording).mockReturnValueOnce(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	act(() => result.current.navigate("library"));
	await act(async () => Promise.resolve());
	act(() => result.current.navigate("player"));
	expect(media.activateRecording).not.toHaveBeenCalled();
	await act(async () => finish());
	await waitFor(() => expect(result.current.current?.recording.id).toBe(3));
	expect(result.current.current?.resume).toEqual({
		position: 0,
		speed: 1,
		volume: 100,
		muted: false,
	});
	expect(media.activateRecording).toHaveBeenCalledWith(1);
});

it("restores the standalone transport position without retaining a second active player", async () => {
	const { result } = renderHook(useRecordingWorkspaces);
	await act(async () => result.current.open());
	await waitFor(() => expect(result.current.current?.recording.id).toBe(1));
	act(() => result.current.playbackChanged({ position: 7, speed: 1.5, volume: 30, muted: true }));
	act(() => result.current.navigate("library"));
	await act(async () => result.current.open(501));
	await waitFor(() => expect(result.current.view).toBe("libraryPlayback"));
	expect(result.current.current?.recording.id).toBe(2);
	act(() => result.current.navigate("player"));
	await waitFor(() => expect(result.current.current?.recording.id).toBe(3));
	expect(result.current.current?.resume).toEqual({
		position: 7,
		speed: 1.5,
		volume: 30,
		muted: true,
	});
	expect(media.deactivateRecording).toHaveBeenCalledWith(2);
});

it("discards superseded analysis and releases stale activations during rapid navigation", async () => {
	let oldAnalysis!: (value: media.MediaInfo | null) => void;
	vi.mocked(media.analyzeRecording).mockReturnValueOnce(
		new Promise((resolve) => {
			oldAnalysis = resolve;
		}),
	);
	const { result } = renderHook(useRecordingWorkspaces);
	await act(async () => result.current.open());
	await waitFor(() => expect(result.current.current?.analyzing).toBe(true));
	act(() => result.current.navigate("library"));
	await act(async () => Promise.resolve());
	let activate!: (value: media.Recording) => void;
	vi.mocked(media.activateRecording).mockReturnValueOnce(
		new Promise((resolve) => {
			activate = resolve;
		}),
	);
	act(() => result.current.navigate("player"));
	await waitFor(() => expect(media.activateRecording).toHaveBeenCalled());
	act(() => result.current.navigate("library"));
	await act(async () => {
		activate({ ...one, id: 3 });
		oldAnalysis(null);
	});
	await waitFor(() => expect(media.deactivateRecording).toHaveBeenCalledWith(3));
	expect(result.current.view).toBe("library");
	expect(result.current.current).toBeNull();
	vi.mocked(media.activateRecording).mockResolvedValueOnce({ ...one, id: 4 });
	act(() => result.current.navigate("player"));
	await waitFor(() => expect(result.current.current?.recording.id).toBe(4));
	expect(media.activateRecording).toHaveBeenLastCalledWith(3);
});

it("does not activate another session if cleanup fails and allows retry", async () => {
	const { result } = renderHook(useRecordingWorkspaces);
	await act(async () => result.current.open());
	await waitFor(() => expect(result.current.current?.recording.id).toBe(1));
	vi.mocked(media.deactivateRecording).mockRejectedValueOnce("Synthetic cleanup failure");
	act(() => result.current.navigate("library"));
	await waitFor(() => expect(result.current.error).toContain("cleanup failure"));
	expect(media.activateRecording).not.toHaveBeenCalled();
	act(() => result.current.navigate("player"));
	await waitFor(() => expect(result.current.current?.recording.id).toBe(3));
});

it("cleans up a picker result that arrives after unmount without starting inspection", async () => {
	let finish!: (value: media.Recording) => void;
	vi.mocked(media.selectRecording).mockReturnValueOnce(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	const { result, unmount } = renderHook(useRecordingWorkspaces);
	let open!: Promise<void>;
	act(() => {
		open = result.current.open();
	});
	unmount();
	await act(async () => {
		finish(one);
		await open;
	});
	expect(media.analyzeRecording).not.toHaveBeenCalled();
	expect(media.deactivateRecording).toHaveBeenCalledWith(1);
});
