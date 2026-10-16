// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Library from "./Library";
import * as service from "../services/library";
import { thumbnails } from "../services/thumbnails";
vi.mock("../services/thumbnails", () => ({ thumbnails: { request: vi.fn() } }));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true, invoke: vi.fn() }));
vi.mock("../services/library", async (original) => ({
	...(await original<typeof service>()),
	libraryStatus: vi.fn(),
	queryLibrary: vi.fn(),
	addLibraryFolder: vi.fn(),
	rescanLibraryFolder: vi.fn(),
	removeLibraryFolder: vi.fn(),
	cancelLibraryScan: vi.fn(),
}));

const root = {
	id: 7,
	path: "/synthetic/recordings",
	scanState: "complete",
	error: null,
	lastScan: null,
	recordings: 43,
};
const status: service.LibraryStatus = {
	roots: [root],
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
};
const item: service.LibraryRecording = {
	id: 91,
	name: "Synthetic recording.mp4",
	path: "/synthetic/recordings/clip.mp4",
	size: 1024,
	modifiedMs: null,
	status: "ready",
	availability: "available",
	duration: 12,
	videoCodec: "hevc",
	width: 1920,
	height: 1080,
	frameRate: 60,
	audioCount: 4,
	error: null,
};

beforeEach(() => {
	localStorage.clear();
	vi.mocked(service.libraryStatus).mockResolvedValue(status);
	vi.mocked(service.queryLibrary).mockResolvedValue({ items: [item], total: 43, limit: 40 });
});
afterEach(() => {
	cleanup();
	vi.resetAllMocks();
	vi.unstubAllGlobals();
});

it("cancels old-page thumbnail work immediately while the next page is still loading", async () => {
	const cancel = vi.fn();
	vi.mocked(thumbnails.request).mockReturnValue(cancel);
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			constructor(private callback: (entries: { isIntersecting: boolean }[]) => void) {}
			observe() {
				this.callback([{ isIntersecting: true }]);
			}
			disconnect() {}
		},
	);
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	await waitFor(() => expect(thumbnails.request).toHaveBeenCalledOnce());
	vi.mocked(service.queryLibrary).mockReturnValue(new Promise(() => {}));
	fireEvent.click(screen.getByRole("button", { name: "Next" }));
	await waitFor(() => expect(cancel).toHaveBeenCalledOnce());
	expect(screen.getByRole("button", { name: "Play " + item.name })).toBeTruthy();
});

it("opens a persistent library ID and retains filters and scroll across workspaces", async () => {
	const open = vi.fn();
	const { rerender } = render(<Library active selecting={false} onOpen={open} />);
	fireEvent.click(await screen.findByRole("button", { name: "Play " + item.name }));
	expect(open).toHaveBeenCalledWith(91);
	fireEvent.change(screen.getByLabelText("Sort recordings"), { target: { value: "size" } });
	fireEvent.click(screen.getByRole("button", { name: /^Filters/ }));
	fireEvent.change(screen.getByLabelText("Folder source"), { target: { value: "7" } });
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ sort: "size", rootId: 7 }),
		),
	);
	const list = screen.getByLabelText("Library recordings");
	list.scrollTop = 144;
	fireEvent.scroll(list);
	rerender(<Library active={false} selecting={false} onOpen={open} />);
	rerender(<Library active selecting={false} onOpen={open} />);
	expect(screen.getByLabelText("Library recordings")).toBe(list);
	expect(list.scrollTop).toBe(144);
	expect((screen.getByLabelText("Sort recordings") as HTMLSelectElement).value).toBe("size");
	expect((screen.getByLabelText("Folder source") as HTMLSelectElement).value).toBe("7");
});

it("debounces search and sends bounded pagination, sorting and filters to Rust", async () => {
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	fireEvent.change(screen.getByLabelText("Search recordings"), { target: { value: "game" } });
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ search: "game", offset: 0 }),
		),
	);
	fireEvent.click(screen.getByRole("button", { name: /^Filters/ }));
	fireEvent.change(screen.getByLabelText("Video codec"), { target: { value: "hevc" } });
	fireEvent.change(screen.getByLabelText("Indexing status"), { target: { value: "available" } });
	fireEvent.click(screen.getByRole("button", { name: "Next" }));
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ codec: "hevc", filter: "available", offset: 40 }),
		),
	);
});

it("adds, rescans and removes folders without accepting a frontend path", async () => {
	vi.mocked(service.addLibraryFolder).mockResolvedValue(root);
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
	await waitFor(() => expect(service.addLibraryFolder).toHaveBeenCalledWith());
	fireEvent.click(screen.getByText("Folders"));
	fireEvent.click(screen.getByRole("button", { name: "Rescan " + root.path }));
	await waitFor(() => expect(service.rescanLibraryFolder).toHaveBeenCalledWith(7));
	await waitFor(() =>
		expect(
			screen.getByRole("button", { name: "Remove " + root.path }).hasAttribute("disabled"),
		).toBe(false),
	);
	fireEvent.click(screen.getByRole("button", { name: "Remove " + root.path }));
	await waitFor(() => expect(service.removeLibraryFolder).toHaveBeenCalledWith(7));
});

it("shows scan issues and cancellation, and disables unavailable recordings", async () => {
	vi.mocked(service.libraryStatus).mockResolvedValue({
		...status,
		progress: {
			...status.progress,
			phase: "analyzing",
			issueCount: 1,
			issues: ["Synthetic access failure"],
		},
	});
	vi.mocked(service.queryLibrary).mockResolvedValue({
		items: [{ ...item, availability: "missing" }],
		total: 1,
		limit: 40,
	});
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	expect(
		(await screen.findByRole("button", { name: "Play " + item.name })).hasAttribute("disabled"),
	).toBe(true);
	expect(screen.getByText("Synthetic access failure")).toBeTruthy();
	fireEvent.click(screen.getByRole("button", { name: "Cancel scan" }));
	await waitFor(() => expect(service.cancelLibraryScan).toHaveBeenCalledOnce());
});

it("ignores an obsolete query result and cleans up polling on unmount", async () => {
	let finish!: (result: service.LibraryPage) => void;
	vi.mocked(service.queryLibrary).mockReturnValueOnce(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	const { unmount } = render(<Library active selecting={false} onOpen={vi.fn()} />);
	fireEvent.change(screen.getByLabelText("Sort recordings"), { target: { value: "name" } });
	await screen.findByRole("button", { name: "Play " + item.name });
	await act(async () => finish({ items: [], total: 0, limit: 40 }));
	expect(screen.getByRole("button", { name: "Play " + item.name })).toBeTruthy();
	unmount();
	const calls = vi.mocked(service.queryLibrary).mock.calls.length;
	await act(async () => Promise.resolve());
	expect(service.queryLibrary).toHaveBeenCalledTimes(calls);
});

it("leaves the library unchanged when folder selection is cancelled and shows real failures", async () => {
	vi.mocked(service.addLibraryFolder)
		.mockResolvedValueOnce(null)
		.mockRejectedValueOnce("Synthetic picker failure");
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
	await waitFor(() =>
		expect(screen.getByRole("button", { name: "Add folder" }).hasAttribute("disabled")).toBe(
			false,
		),
	);
	expect(screen.queryByRole("alert")).toBeNull();
	fireEvent.click(screen.getByRole("button", { name: "Add folder" }));
	expect(await screen.findByRole("alert")).toHaveProperty(
		"textContent",
		"Synthetic picker failure",
	);
});

it("defaults to grid, remembers list across workspaces and remounts, and tolerates invalid preferences", async () => {
	localStorage.setItem("klipper.libraryView", "invalid");
	const { rerender, unmount } = render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	expect(screen.getByRole("button", { name: "Grid view" }).getAttribute("aria-pressed")).toBe(
		"true",
	);
	fireEvent.click(screen.getByRole("button", { name: "List view" }));
	expect(screen.getByLabelText("Library recordings").dataset.view).toBe("list");
	rerender(<Library active={false} selecting={false} onOpen={vi.fn()} />);
	rerender(<Library active selecting={false} onOpen={vi.fn()} />);
	expect(screen.getByRole("button", { name: "List view" }).getAttribute("aria-pressed")).toBe(
		"true",
	);
	expect(localStorage.getItem("klipper.libraryView")).toBe("list");
	unmount();
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	expect(screen.getByLabelText("Library recordings").dataset.view).toBe("list");
});

it("keeps secondary controls collapsed and returns keyboard focus when dismissed with Escape", async () => {
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	expect(screen.queryByRole("combobox", { name: "Folder source" })).toBeNull();
	const filters = screen.getByRole("button", { name: /^Filters/ });
	fireEvent.click(filters);
	const codec = screen.getByRole("combobox", { name: "Video codec" });
	codec.focus();
	fireEvent.keyDown(codec, { key: "Escape" });
	expect(document.activeElement).toBe(filters);
	expect(filters.getAttribute("aria-expanded")).toBe("false");
	const folders = screen.getByRole("button", { name: /^Folders/ });
	fireEvent.click(folders);
	const rescan = screen.getByRole("button", { name: "Rescan " + root.path });
	rescan.focus();
	fireEvent.keyDown(rescan, { key: "Escape" });
	expect(document.activeElement).toBe(folders);
	expect(folders.getAttribute("aria-expanded")).toBe("false");
});

it("navigates recordings with arrows, Home and End, skips missing files and restores opened-item focus", async () => {
	vi.mocked(service.queryLibrary).mockResolvedValue({
		items: [
			item,
			{ ...item, id: 92, name: "Missing.mp4", availability: "missing" },
			{ ...item, id: 93, name: "Third.mp4" },
		],
		total: 3,
		limit: 40,
	});
	const open = vi.fn();
	const { rerender } = render(<Library active selecting={false} onOpen={open} />);
	const first = await screen.findByRole("button", { name: "Play " + item.name });
	const third = screen.getByRole("button", { name: "Play Third.mp4" });
	first.focus();
	fireEvent.keyDown(first, { key: "ArrowRight" });
	expect(document.activeElement).toBe(third);
	fireEvent.keyDown(third, { key: "Home" });
	expect(document.activeElement).toBe(first);
	fireEvent.keyDown(first, { key: "End" });
	expect(document.activeElement).toBe(third);
	fireEvent.click(third);
	expect(open).toHaveBeenCalledWith(93);
	expect(third.getAttribute("aria-current")).toBe("true");
	rerender(<Library active={false} selecting={false} onOpen={open} />);
	document.body.focus();
	rerender(<Library active selecting={false} onOpen={open} />);
	expect(document.activeElement).toBe(third);
});

it("retains full names and paths as inspectable values without printing paths on cards", async () => {
	const name = "Synthetic_" + "long_recording_name_".repeat(12) + ".mp4";
	vi.mocked(service.queryLibrary).mockResolvedValue({
		items: [{ ...item, name }],
		total: 1,
		limit: 40,
	});
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	const recording = await screen.findByRole("button", { name: "Play " + name });
	expect(recording.title).toContain(name + "\n" + item.path);
	expect(recording.textContent).not.toContain(item.path);
	expect(recording.textContent).toContain("00:12");
});

it("does not restart visible thumbnail work when switching views and cancels when leaving Library", async () => {
	const cancel = vi.fn();
	vi.mocked(thumbnails.request).mockReturnValue(cancel);
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			constructor(private callback: (entries: { isIntersecting: boolean }[]) => void) {}
			observe() {
				this.callback([{ isIntersecting: true }]);
			}
			disconnect() {}
		},
	);
	const { rerender } = render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	await waitFor(() => expect(thumbnails.request).toHaveBeenCalledOnce());
	fireEvent.click(screen.getByRole("button", { name: "List view" }));
	expect(thumbnails.request).toHaveBeenCalledOnce();
	expect(cancel).not.toHaveBeenCalled();
	rerender(<Library active={false} selecting={false} onOpen={vi.fn()} />);
	expect(cancel).toHaveBeenCalledOnce();
});

it("hides routine successful scans and makes completed scan warnings dismissible", async () => {
	vi.mocked(service.libraryStatus).mockResolvedValue({
		...status,
		progress: { ...status.progress, phase: "complete" },
	});
	const { unmount } = render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	expect(screen.queryByText("Scan complete")).toBeNull();
	unmount();
	vi.mocked(service.libraryStatus).mockResolvedValue({
		...status,
		progress: {
			...status.progress,
			phase: "partial",
			issueCount: 1,
			issues: ["Synthetic failure"],
		},
	});
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	fireEvent.click(screen.getByRole("button", { name: "Dismiss scan status" }));
	expect(screen.queryByText("Scan partial")).toBeNull();
});

it("moves vertically by grid columns and by one row in list view without changing filters", async () => {
	vi.mocked(service.queryLibrary).mockResolvedValue({
		items: Array.from({ length: 4 }, (_, index) => ({
			...item,
			id: index + 1,
			name: "Synthetic " + index + ".mp4",
		})),
		total: 4,
		limit: 40,
	});
	const computedStyle = globalThis.getComputedStyle;
	vi.stubGlobal("getComputedStyle", (element: Element) => {
		const style = computedStyle(element);
		if (element.tagName === "UL") style.gridTemplateColumns = "240px 240px";
		return style;
	});
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	const first = await screen.findByRole("button", { name: "Play Synthetic 0.mp4" });
	const second = screen.getByRole("button", { name: "Play Synthetic 1.mp4" });
	const third = screen.getByRole("button", { name: "Play Synthetic 2.mp4" });
	first.focus();
	fireEvent.keyDown(first, { key: "ArrowDown" });
	expect(document.activeElement).toBe(third);
	fireEvent.keyDown(third, { key: "ArrowUp" });
	expect(document.activeElement).toBe(first);
	fireEvent.click(screen.getByRole("button", { name: "List view" }));
	first.focus();
	fireEvent.keyDown(first, { key: "ArrowDown" });
	expect(document.activeElement).toBe(second);
});

it("shows and clears active filters without resetting the chosen sort or view", async () => {
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByRole("button", { name: "Play " + item.name });
	fireEvent.click(screen.getByRole("button", { name: "List view" }));
	fireEvent.change(screen.getByLabelText("Sort recordings"), { target: { value: "duration" } });
	fireEvent.click(screen.getByRole("button", { name: /^Filters/ }));
	fireEvent.change(screen.getByLabelText("Video codec"), { target: { value: "hevc" } });
	fireEvent.change(screen.getByLabelText("Folder source"), { target: { value: "7" } });
	fireEvent.click(screen.getByRole("button", { name: /^Filters/ }));
	expect(screen.getByLabelText("Active filters").textContent).toContain("Folder: recordings");
	fireEvent.click(screen.getByRole("button", { name: "Clear filters" }));
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ rootId: null, codec: "all", sort: "duration", offset: 0 }),
		),
	);
	expect(screen.getByLabelText("Library recordings").dataset.view).toBe("list");
	expect(screen.queryByLabelText("Active filters")).toBeNull();
});

it("recovers from an empty filtered result with a useful reset action", async () => {
	vi.mocked(service.queryLibrary).mockResolvedValue({ items: [], total: 0, limit: 40 });
	render(<Library active selecting={false} onOpen={vi.fn()} />);
	await screen.findByText("No matching recordings");
	fireEvent.change(screen.getByLabelText("Search recordings"), { target: { value: "no match" } });
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ search: "no match" }),
		),
	);
	fireEvent.click(screen.getByRole("button", { name: "Clear search and filters" }));
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ search: "", offset: 0 }),
		),
	);
});
