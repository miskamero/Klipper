import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "./media";

type Result = (url: string, error: string) => void;
interface Preview {
	url: string;
	size: number;
	users: number;
}
interface Job {
	id: number;
	key: string;
	token: string;
	done: Result;
	cancelled: boolean;
	stage: "lookup" | "extract";
	running: boolean;
	preview?: Preview;
}
const buffer = (result: ArrayBuffer | number[]) =>
	result instanceof ArrayBuffer ? result : Uint8Array.from(result).buffer;

// Four cheap disk lookups bypass the single extraction slot. Only visible rows subscribe.
// Retain small image URLs across navigation, not source images or an unbounded decoded library.
export class ThumbnailQueue {
	private visible = new Set<Job>();
	private waiting: Job[] = [];
	private extracting: Job | null = null;
	private lookups = 0;
	private images = new Map<string, Preview>();
	private imageBytes = 0;
	private subscribers = new Set<Job>();

	constructor(
		private maxEntries = 120,
		private maxBytes = 8 * 1024 * 1024,
	) {}

	request(id: number, key: string, done: Result): () => void {
		const job: Job = {
			id,
			key,
			token: crypto.randomUUID(),
			done,
			cancelled: false,
			stage: "lookup",
			running: false,
		};
		const cached = this.images.get(key);
		this.subscribers.add(job);
		if (cached) {
			this.images.delete(key);
			this.images.set(key, cached);
			cached.users++;
			job.preview = cached;
			queueMicrotask(() => {
				if (!job.cancelled) done(cached.url, "");
			});
		} else {
			this.visible.add(job);
			this.pump();
		}
		return () => {
			this.subscribers.delete(job);
			job.cancelled = true;
			this.visible.delete(job);
			this.waiting = this.waiting.filter((item) => item !== job);
			if (this.extracting === job) {
				void invoke("library_thumbnail_cancel", { requestId: job.token }).catch(() => {});
			}
			if (job.preview) {
				job.preview.users--;
				if (!job.preview.users && this.images.get(key) !== job.preview)
					URL.revokeObjectURL(job.preview.url);
				job.preview = undefined;
			}
			this.trim();
		};
	}

	clear() {
		if (this.extracting) {
			void invoke("library_thumbnail_cancel", { requestId: this.extracting.token }).catch(
				() => {},
			);
		}
		for (const job of this.subscribers) {
			job.cancelled = true;
			job.preview = undefined;
		}
		this.subscribers.clear();
		this.visible.clear();
		this.waiting = [];
		for (const preview of this.images.values()) URL.revokeObjectURL(preview.url);
		this.images.clear();
		this.imageBytes = 0;
	}

	private trim() {
		for (const [key, preview] of this.images) {
			if (this.images.size <= this.maxEntries && this.imageBytes <= this.maxBytes) break;
			if (preview.users) continue;
			this.images.delete(key);
			this.imageBytes -= preview.size;
			URL.revokeObjectURL(preview.url);
		}
	}

	private complete(job: Job, bytes: ArrayBuffer | null, error = "") {
		this.visible.delete(job);
		if (job.cancelled) return;
		if (!bytes) {
			job.done("", error);
			return;
		}
		let preview = this.images.get(job.key);
		if (!preview) {
			preview = {
				url: URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" })),
				size: bytes.byteLength,
				users: 0,
			};
			this.images.set(job.key, preview);
			this.imageBytes += preview.size;
		}
		preview.users++;
		job.preview = preview;
		this.trim();
		job.done(preview.url, "");
	}

	private pump() {
		for (const job of this.visible) {
			if (this.lookups >= 4) break;
			if (job.stage !== "lookup" || job.running) continue;
			job.running = true;
			this.lookups++;
			void this.lookup(job);
		}
		for (const job of this.visible) {
			if (this.waiting.length >= 8) break;
			if (job.stage === "extract" && !job.running && !this.waiting.includes(job))
				this.waiting.push(job);
		}
		if (!this.extracting) {
			const job = this.waiting.shift();
			if (job) {
				this.extracting = job;
				job.running = true;
				void this.extract(job);
			}
		}
	}

	private async lookup(job: Job) {
		try {
			const result = buffer(
				await invoke<ArrayBuffer | number[]>("library_thumbnail", {
					fileId: job.id,
					requestId: job.token,
					cachedOnly: true,
				}),
			);
			if (job.cancelled) return;
			if (result.byteLength) this.complete(job, result);
			else job.stage = "extract";
		} catch (error) {
			this.complete(job, null, errorMessage(error));
		} finally {
			job.running = false;
			this.lookups--;
			this.pump();
		}
	}

	private async extract(job: Job) {
		try {
			const result = buffer(
				await invoke<ArrayBuffer | number[]>("library_thumbnail", {
					fileId: job.id,
					requestId: job.token,
					cachedOnly: false,
				}),
			);
			if (job.cancelled) return;
			if (result.byteLength) this.complete(job, result);
			else {
				// A cancelled native owner may still be reaping its child.
				await new Promise((resolve) => setTimeout(resolve, 150));
			}
		} catch (error) {
			this.complete(job, null, errorMessage(error));
		} finally {
			job.running = false;
			this.extracting = null;
			this.pump();
		}
	}
}

export const thumbnails = new ThumbnailQueue();
