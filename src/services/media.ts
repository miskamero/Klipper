import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";

export interface Recording {
	id: number;
	name: string;
	path: string;
}
export interface AudioStream {
	index: number;
	codec: string | null;
	channels: number | null;
	channelLayout: string | null;
	tags: Record<string, string>;
}
export interface MediaInfo {
	container: string | null;
	videoCodec: string | null;
	width: number | null;
	height: number | null;
	frameRate: number | null;
	duration: number | null;
	fileSize: number;
	bitrate: number | null;
	audio: AudioStream[];
}

export async function selectRecording(): Promise<Recording | null> {
	if (!isTauri())
		throw new Error("Open the desktop app with npm run desktop to select a local recording.");
	return invoke("select_recording");
}
// null means superseded or shutting down; genuine inspection failures still reject.
export function analyzeRecording(id: number): Promise<MediaInfo | null> {
	return invoke("analyze_recording", { id });
}
export function activateRecording(id: number): Promise<Recording> {
	return invoke("activate_recording", { id });
}
export function deactivateRecording(id: number): Promise<void> {
	return invoke("deactivate_recording", { id });
}
export function playbackUrl(recording: Recording): string {
	return convertFileSrc(recording.path);
}
export function errorMessage(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}
