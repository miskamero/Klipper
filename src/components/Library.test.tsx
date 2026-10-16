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
	fireEvent.change(screen.getByLabelText("Folder source"), { target: { value: "7" } });
	await waitFor(() =>
		expect(service.queryLibrary).toHaveBeenLastCalledWith(
			expect.objectContaining({ sort: "size", rootId: 7 }),
		),
	);
	const list = screen.getByLabelText("Library recordings");
	list.scrollTop = 144;
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
