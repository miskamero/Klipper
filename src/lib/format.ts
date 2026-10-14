export function time(seconds: number | null): string {
	if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "—";
	const value = Math.floor(seconds);
	const hours = Math.floor(value / 3600);
	return `${hours ? `${hours}:` : ""}${String(Math.floor(value / 60) % 60).padStart(2, "0")}:${String(value % 60).padStart(2, "0")}`;
}
export function bytes(value: number): string {
	if (!Number.isFinite(value) || value < 0) return "—";
	if (value < 1024) return `${value} B`;
	const unit = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
	return `${(value / 1024 ** unit).toFixed(1)} ${["B", "KiB", "MiB", "GiB", "TiB"][unit]}`;
}
export function playbackError(code?: number): string {
	switch (code) {
		case 1:
			return "Playback was interrupted. Reopen the recording to try again.";
		case 2:
			return "The recording could not be read. It may have moved or become unavailable.";
		case 3:
			return "The playback engine could not decode this recording. Its codec or profile may be unsupported, or the file may be damaged.";
		default:
			return "This recording is not supported by the system playback engine. HEVC support depends on Windows codecs, GPU drivers and WebView2. You can still inspect its streams below.";
	}
}
