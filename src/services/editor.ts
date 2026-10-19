import { invoke } from "@tauri-apps/api/core";
import type { Control, Snapshot } from "../components/native/nativePlayback";

export interface EditProject {
	output: { container: "mp4"; videoCodec: "h264"; audioCodec: "aac"; crf: number };
	version: 1;
	token: number;
	recordingId: number;
	revision: string;
	start: number;
	end: number;
	info: {
		duration: number;
		origin: number;
		videoIndex: number;
		codec: string;
		width: number;
		height: number;
		timeBase: string;
		audio: { index: number; codec: string; channels: number; sampleRate: number }[];
	};
}
// One native player; editor controls use its existing serial command pump.
export interface EditorBridge {
	snapshot?: Snapshot;
	command?: (control: Control) => void;
	fullscreen?: (value: boolean) => Promise<boolean>;
	report?: (snapshot: Snapshot) => void;
}
export const beginEdit = (id: number) => invoke<EditProject>("editor_begin", { id });
export const closeEdit = (token: number, cancel = false) =>
	invoke("editor_close", { token, ...(cancel ? { cancel: true } : {}) });
export const resolveRange = (token: number, start: number, end: number) =>
	invoke<EditProject>("editor_range", { token, selection: { start, end } });

export function clampRange(start: number, end: number, duration: number) {
	const minimum = Math.min(0.001, duration);
	return {
		start: Math.max(0, Math.min(start, end - minimum, duration - minimum)),
		end: Math.min(duration, Math.max(end, start + minimum, minimum)),
	};
}
export function preciseTime(seconds: number) {
	const ms = Math.round(Math.max(0, seconds) * 1000);
	return `${Math.floor(ms / 3600000)
		.toString()
		.padStart(2, "0")}:${Math.floor((ms % 3600000) / 60000)
		.toString()
		.padStart(2, "0")}:${Math.floor((ms % 60000) / 1000)
		.toString()
		.padStart(2, "0")}.${(ms % 1000).toString().padStart(3, "0")}`;
}
export function parseTime(value: string): number | null {
	if (/^\d+(\.\d{1,6})?$/.test(value)) return Number(value);
	const match = /^(\d+):([0-5]\d):([0-5]\d(?:\.\d{1,6})?)$/.exec(value);
	return match ? Number(match[1]) * 3600 + Number(match[2]) * 60 + Number(match[3]) : null;
}
