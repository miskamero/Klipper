import { invoke, isTauri } from "@tauri-apps/api/core";

export interface PlaybackBackend {
	engine: "native" | "html" | "unavailable";
	message: string | null;
}

export function getPlaybackBackend(): Promise<PlaybackBackend> {
	if (!isTauri()) return Promise.resolve({ engine: "html", message: null });
	return invoke("playback_backend");
}
