import { invoke, isTauri } from "@tauri-apps/api/core";

export interface PlaybackBackend {
	engine: "native" | "html" | "unavailable";
	message: string | null;
}

// Session-only transport state. Restoring a context always starts paused.
export interface PlaybackPosition {
	position: number;
	speed: number;
	volume: number;
	muted: boolean;
}

export function getPlaybackBackend(): Promise<PlaybackBackend> {
	if (!isTauri()) return Promise.resolve({ engine: "html", message: null });
	return invoke("playback_backend");
}
