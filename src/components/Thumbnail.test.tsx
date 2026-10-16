// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Thumbnail from "./Thumbnail";
import { thumbnails } from "../services/thumbnails";
import type { LibraryRecording } from "../services/library";
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("../services/thumbnails", () => ({ thumbnails: { request: vi.fn() } }));
let intersect!: (visible: boolean) => void;
const disconnect = vi.fn(),
	cancel = vi.fn();
const source: LibraryRecording = {
	id: 1,
	name: "Synthetic.mp4",
	path: "/synthetic/clip.mp4",
	size: 4,
	modifiedMs: 1,
	revision: "v1",
	status: "ready",
	availability: "available",
	duration: 1,
	videoCodec: "hevc",
	width: 1920,
	height: 1080,
	frameRate: 60,
	audioCount: 1,
	error: null,
};
const root = { current: null };
beforeEach(() => {
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			constructor(callback: (entries: { isIntersecting: boolean }[]) => void) {
				intersect = (visible) => callback([{ isIntersecting: visible }]);
			}
			observe() {}
			disconnect = disconnect;
		},
	);
	vi.stubGlobal("URL", {
		createObjectURL: vi.fn(() => "blob:synthetic-preview"),
		revokeObjectURL: vi.fn(),
	});
	vi.mocked(thumbnails.request).mockReturnValue(cancel);
});
afterEach(() => {
	cleanup();
	vi.resetAllMocks();
	vi.unstubAllGlobals();
});
const deliver = (error = "") =>
	act(() => {
		vi.mocked(thumbnails.request).mock.calls.at(-1)![2](
			error ? "" : "blob:synthetic-preview",
			error,
		);
	});

it("requests only intersecting recordings and releases the preview lease offscreen", () => {
	render(<Thumbnail recording={source} active root={root} />);
	expect(thumbnails.request).not.toHaveBeenCalled();
	expect(screen.getByRole("img", { name: "Preview loading" })).toBeTruthy();
	act(() => intersect(true));
	expect(thumbnails.request).toHaveBeenCalledWith(1, expect.any(String), expect.any(Function));
	deliver();
	const image = document.querySelector("img")!;
	expect(image.getAttribute("src")).toBe("blob:synthetic-preview");
	expect(image.getAttribute("width")).toBe("320");
	act(() => intersect(false));
	expect(cancel).toHaveBeenCalledOnce();
	expect(document.querySelector("img")).toBeNull();
});

it("invalidates previews after a rescan revision changes and ignores obsolete completion", () => {
	const { rerender } = render(<Thumbnail recording={source} active root={root} />);
	act(() => intersect(true));
	const old = vi.mocked(thumbnails.request).mock.calls[0][2];
	deliver();
	rerender(<Thumbnail recording={{ ...source, revision: "v2" }} active root={root} />);
	expect(cancel).toHaveBeenCalled();
	expect(document.querySelector("img")).toBeNull();
	act(() => old("blob:obsolete-preview", ""));
	expect(document.querySelector("img")).toBeNull();
	act(() => intersect(true));
	expect(thumbnails.request).toHaveBeenCalledTimes(2);
});

it("cancels on navigation and unmount and shows useful extraction and image failures", () => {
	const { rerender, unmount } = render(<Thumbnail recording={source} active root={root} />);
	act(() => intersect(true));
	deliver("FFmpeg could not start");
	expect(screen.getByRole("img", { name: /Preview unavailable: FFmpeg/ })).toBeTruthy();
	rerender(<Thumbnail recording={source} active={false} root={root} />);
	expect(cancel).toHaveBeenCalled();
	rerender(<Thumbnail recording={source} active root={root} />);
	act(() => intersect(true));
	deliver();
	fireEvent.error(document.querySelector("img")!);
	expect(screen.getByRole("img", { name: /could not be displayed/ })).toBeTruthy();
	unmount();
	expect(disconnect).toHaveBeenCalled();
});

it("does not extract missing files", () => {
	render(<Thumbnail recording={{ ...source, availability: "missing" }} active root={root} />);
	expect(screen.getByRole("img", { name: /Recording is unavailable/ })).toBeTruthy();
	expect(thumbnails.request).not.toHaveBeenCalled();
});
