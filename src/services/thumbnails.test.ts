import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ThumbnailQueue } from "./thumbnails";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const image = () => new Uint8Array([255, 216, 255, 217]).buffer;
const tick = async () => {
	for (let i = 0; i < 12; i++) await Promise.resolve();
};
beforeEach(() => {
	let next = 0;
	vi.stubGlobal("URL", {
		createObjectURL: vi.fn(() => "blob:preview-" + ++next),
		revokeObjectURL: vi.fn(),
	});
});
afterEach(() => {
	vi.resetAllMocks();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

it("serializes extraction, cancels obsolete work and discards its late completion", async () => {
	const completions: ((bytes: ArrayBuffer) => void)[] = [];
	vi.mocked(invoke).mockImplementation(async (command, args) => {
		if (command === "library_thumbnail_cancel") return;
		if ((args as { cachedOnly: boolean }).cachedOnly) return new ArrayBuffer(0);
		return new Promise((resolve) => completions.push(resolve));
	});
	const queue = new ThumbnailQueue();
	const first = vi.fn(),
		second = vi.fn(),
		third = vi.fn();
	const cancel = queue.request(1, "one", first);
	const cancelSecond = queue.request(2, "two", second);
	queue.request(3, "three", third);
	await tick();
	expect(completions).toHaveLength(1);
	cancelSecond();
	cancel();
	expect(invoke).toHaveBeenCalledWith("library_thumbnail_cancel", {
		requestId: expect.any(String),
	});
	completions[0](image());
	await tick();
	expect(completions).toHaveLength(2);
	expect(first).not.toHaveBeenCalled();
	expect(second).not.toHaveBeenCalled();
	completions[1](image());
	await tick();
	expect(third).toHaveBeenCalledWith("blob:preview-1", "");
});

it("reset revokes retained and subscribed previews and ignores old in-flight responses", async () => {
	let finish!: (bytes: ArrayBuffer) => void;
	vi.mocked(invoke).mockImplementation(async (_command, args) => {
		if ((args as { fileId: number }).fileId === 2)
			return new Promise((resolve) => {
				finish = resolve;
			});
		return image();
	});
	const queue = new ThumbnailQueue();
	const remove = queue.request(1, "old", vi.fn());
	await tick();
	const late = vi.fn();
	queue.request(2, "pending", late);
	await tick();
	queue.clear();
	expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:preview-1");
	remove();
	finish(image());
	await tick();
	expect(late).not.toHaveBeenCalled();
	const fresh = vi.fn();
	queue.request(1, "old", fresh);
	await tick();
	expect(fresh).toHaveBeenCalledWith("blob:preview-2", "");
});

it("serves disk hits while an unrelated cold extraction is blocked", async () => {
	let finish!: (bytes: ArrayBuffer) => void;
	vi.mocked(invoke).mockImplementation(async (_command, args) => {
		const job = args as { fileId: number; cachedOnly: boolean };
		if (job.cachedOnly) return job.fileId === 1 ? new ArrayBuffer(0) : image();
		return new Promise((resolve) => {
			finish = resolve;
		});
	});
	const queue = new ThumbnailQueue();
	const cold = vi.fn(),
		warm = vi.fn();
	queue.request(1, "cold", cold);
	await tick();
	queue.request(2, "warm", warm);
	await tick();
	expect(warm).toHaveBeenCalledWith("blob:preview-1", "");
	expect(cold).not.toHaveBeenCalled();
	finish(image());
	await tick();
	expect(cold).toHaveBeenCalledOnce();
});

it("reuses the same memory URL on revisit without IPC, transfer or URL recreation", async () => {
	vi.mocked(invoke).mockResolvedValue(image());
	const queue = new ThumbnailQueue();
	const done = vi.fn();
	const release = queue.request(1, "revision-1", done);
	await tick();
	release();
	queue.request(1, "revision-1", done);
	await tick();
	expect(invoke).toHaveBeenCalledTimes(1);
	expect(URL.createObjectURL).toHaveBeenCalledOnce();
	expect(URL.revokeObjectURL).not.toHaveBeenCalled();
	expect(done.mock.calls).toEqual([
		["blob:preview-1", ""],
		["blob:preview-1", ""],
	]);
	// A new app instance has no memory entries and falls back to native disk lookup.
	const restarted = new ThumbnailQueue();
	restarted.request(1, "revision-1", vi.fn());
	await tick();
	expect(invoke).toHaveBeenLastCalledWith(
		"library_thumbnail",
		expect.objectContaining({ cachedOnly: true }),
	);
});

it("invalidates memory reuse when a recording or preview revision changes", async () => {
	vi.mocked(invoke).mockResolvedValue(image());
	const queue = new ThumbnailQueue();
	const done = vi.fn();
	queue.request(1, "old-fingerprint", done)();
	await tick();
	const release = queue.request(1, "old-fingerprint", done);
	await tick();
	release();
	queue.request(1, "new-fingerprint", done);
	await tick();
	expect(done.mock.calls.map(([url]) => url)).toEqual(["blob:preview-1", "blob:preview-2"]);
	expect(URL.createObjectURL).toHaveBeenCalledTimes(2);
});

it("bounds retained JPEG bytes and entry count and never revokes a visible image", async () => {
	for (const [entries, bytes] of [
		[2, 100],
		[100, 8],
	]) {
		vi.mocked(invoke).mockResolvedValue(image());
		const queue = new ThumbnailQueue(entries, bytes);
		const pinned = vi.fn(),
			second = vi.fn();
		const releasePinned = queue.request(1, "one", pinned);
		await tick();
		const releaseSecond = queue.request(2, "two", second);
		await tick();
		releaseSecond();
		const releaseThird = queue.request(3, "three", vi.fn());
		await tick();
		releaseThird();
		expect(URL.revokeObjectURL).toHaveBeenCalledWith(second.mock.calls[0][0]);
		expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(pinned.mock.calls[0][0]);
		releasePinned();
	}
});

it("bounds disk lookup concurrency and the extraction queue with many visible requests", async () => {
	const lookups: ((bytes: ArrayBuffer) => void)[] = [];
	const extracts: ((bytes: ArrayBuffer) => void)[] = [];
	vi.mocked(invoke).mockImplementation(
		async (_command, args) =>
			new Promise((resolve) =>
				((args as { cachedOnly: boolean }).cachedOnly ? lookups : extracts).push(resolve),
			),
	);
	const queue = new ThumbnailQueue();
	Array.from({ length: 40 }, (_, i) => queue.request(i, String(i), vi.fn()));
	expect(lookups).toHaveLength(4);
	for (let i = 0; i < 40; i++) {
		lookups[i](new ArrayBuffer(0));
		await tick();
	}
	expect(extracts).toHaveLength(1);
	expect(Reflect.get(queue, "waiting").length).toBeLessThanOrEqual(8);
});

it("retries a deferred extraction without spinning or showing cancellation as an error", async () => {
	vi.useFakeTimers();
	let extraction = 0;
	vi.mocked(invoke).mockImplementation(async (_command, args) =>
		(args as { cachedOnly: boolean }).cachedOnly || extraction++ === 0
			? new ArrayBuffer(0)
			: image(),
	);
	const queue = new ThumbnailQueue(),
		done = vi.fn();
	queue.request(1, "one", done);
	await tick();
	expect(done).not.toHaveBeenCalled();
	expect(extraction).toBe(1);
	await vi.advanceTimersByTimeAsync(150);
	expect(done).toHaveBeenCalledWith("blob:preview-1", "");
});

it("ignores cancelled disk lookups and reports genuine failures without caching them forever", async () => {
	let finish!: (bytes: ArrayBuffer) => void;
	vi.mocked(invoke).mockReturnValueOnce(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	const queue = new ThumbnailQueue(),
		done = vi.fn();
	queue.request(1, "one", done)();
	finish(image());
	await tick();
	expect(done).not.toHaveBeenCalled();
	expect(URL.createObjectURL).not.toHaveBeenCalled();
	vi.mocked(invoke).mockRejectedValue("Synthetic file unavailable");
	queue.request(1, "one", done);
	await tick();
	expect(done).toHaveBeenCalledWith("", "Synthetic file unavailable");
});
