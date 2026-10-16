import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "./media";

type Result = (bytes: ArrayBuffer | null, error: string) => void;
interface Job {
	id: number;
	token: string;
	done: Result;
	cancelled: boolean;
}

// One native request in flight and eight queued. Only intersecting rows subscribe.
// IPC is serialized even while React unmounts/replaces pages.
export class ThumbnailQueue {
	private visible = new Set<Job>();
	private waiting: Job[] = [];
	private active: Job | null = null;
	private running = false;

	request(id: number, done: Result): () => void {
		const job: Job = { id, token: crypto.randomUUID(), done, cancelled: false };
		this.visible.add(job);
		void this.run();
		return () => {
			job.cancelled = true;
			this.visible.delete(job);
			this.waiting = this.waiting.filter((item) => item !== job);
			if (this.active === job) {
				void invoke("library_thumbnail_cancel", { requestId: job.token }).catch(() => {});
			}
		};
	}

	private fill() {
		for (const job of this.visible) {
			if (this.waiting.length >= 8) break;
			if (job !== this.active && !this.waiting.includes(job)) this.waiting.push(job);
		}
	}

	private async run() {
		if (this.running) return;
		this.running = true;
		try {
			for (;;) {
				this.fill();
				const job = this.waiting.shift();
				if (!job) break;
				this.active = job;
				try {
					const result = await invoke<ArrayBuffer | number[]>("library_thumbnail", {
						fileId: job.id,
						requestId: job.token,
					});
					if (job.cancelled) continue;
					const bytes =
						result instanceof ArrayBuffer ? result : Uint8Array.from(result).buffer;
					if (!bytes.byteLength) {
						// The previous native owner may still be finishing cleanup.
						this.visible.delete(job);
						this.visible.add(job);
						await new Promise((resolve) => setTimeout(resolve, 150));
						continue;
					}
					this.visible.delete(job);
					job.done(bytes, "");
				} catch (error) {
					this.visible.delete(job);
					if (!job.cancelled) job.done(null, errorMessage(error));
				} finally {
					this.active = null;
				}
			}
		} finally {
			this.running = false;
		}
	}
}

export const thumbnails = new ThumbnailQueue();
