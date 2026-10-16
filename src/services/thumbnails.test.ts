import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ThumbnailQueue } from "./thumbnails";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
afterEach(() => {
	vi.resetAllMocks();
	vi.useRealTimers();
});
const tick = async () => {
	await Promise.resolve();
	await Promise.resolve();
	await Promise.resolve();
};

it("serializes extraction and discards cancelled queued and in-flight work", async () => {
	let finish!: (bytes: ArrayBuffer) => void;
	vi.mocked(invoke).mockImplementation(async (name) =>
		name === "library_thumbnail"
			? new Promise((resolve) => {
					finish = resolve;
				})
			: undefined,
	);
	const queue = new ThumbnailQueue();
	const first = vi.fn(),
		second = vi.fn(),
		third = vi.fn();
	const cancel = queue.request(1, first);
	const cancelSecond = queue.request(2, second);
	queue.request(3, third);
	expect(
		vi.mocked(invoke).mock.calls.filter(([name]) => name === "library_thumbnail"),
	).toHaveLength(1);
	cancelSecond();
	cancel();
	expect(invoke).toHaveBeenCalledWith(
		"library_thumbnail_cancel",
		expect.objectContaining({ requestId: expect.any(String) }),
	);
	finish(new Uint8Array([1]).buffer);
	await tick();
	expect(invoke).toHaveBeenCalledWith(
		"library_thumbnail",
		expect.objectContaining({ fileId: 3 }),
	);
	expect(invoke).not.toHaveBeenCalledWith(
		"library_thumbnail",
		expect.objectContaining({ fileId: 2 }),
	);
	expect(first).not.toHaveBeenCalled();
	finish(new Uint8Array([2]).buffer);
	await tick();
	expect(third).toHaveBeenCalledOnce();
});

it("keeps only eight queued jobs, replenishes visible demand, and reports genuine errors", async () => {
	let finish!: (bytes: ArrayBuffer) => void;
	vi.mocked(invoke).mockImplementation(
		async () =>
			new Promise((resolve) => {
				finish = resolve;
			}),
	);
	const queue = new ThumbnailQueue();
	const callbacks = Array.from({ length: 40 }, () => vi.fn());
	const cancels = callbacks.map((callback, index) => queue.request(index + 1, callback));
	// Inspect the explicit queue bound as well as actual native admission.
	expect(Reflect.get(queue, "waiting").length).toBeLessThanOrEqual(8);
	vi.mocked(invoke).mockRejectedValue(new Error("FFmpeg is missing"));
	finish(new ArrayBuffer(1));
	await tick();
	await vi.waitFor(() => expect(callbacks[39]).toHaveBeenCalledWith(null, "FFmpeg is missing"));
	cancels.forEach((cancel) => cancel());
});

it("retries a deferred native slot without spinning or displaying cancellation as an error", async () => {
	vi.useFakeTimers();
	vi.mocked(invoke)
		.mockResolvedValueOnce(new ArrayBuffer(0))
		.mockResolvedValueOnce([255, 216, 255, 217]);
	const queue = new ThumbnailQueue();
	const done = vi.fn();
	queue.request(1, done);
	await tick();
	expect(done).not.toHaveBeenCalled();
	expect(invoke).toHaveBeenCalledTimes(1);
	await vi.advanceTimersByTimeAsync(150);
	expect(done).toHaveBeenCalledWith(expect.any(ArrayBuffer), "");
});
