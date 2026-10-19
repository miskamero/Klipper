export type Control =
	| { kind: "pause" | "mute"; value: boolean }
	| { kind: "seek" | "speed" | "volume"; value: number }
	| { kind: "previewRange"; value: [number, number] }
	| { kind: "clearPreview" }
	| { kind: "frameStep"; value: boolean };
export interface Snapshot {
	id: number;
	position: number;
	duration: number;
	paused: boolean;
	speed: number;
	volume: number;
	muted: boolean;
	hwdec: string;
	dropped: number;
	error: string;
	fullscreen: boolean;
	pointer?: { x: number; y: number } | null;
	seeking?: boolean;
}
