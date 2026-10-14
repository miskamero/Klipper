export function containerLabel(container: string | null): string {
	if (!container) return "Unknown";
	const names = container
		.toLowerCase()
		.split(",")
		.map((name) => name.trim());
	if (names.includes("mov") && names.includes("mp4")) return "QuickTime / MPEG-4 family";
	if (names.includes("matroska") && names.includes("webm")) return "Matroska / WebM family";
	const labels: Record<string, string> = {
		mp4: "MPEG-4 (MP4)",
		mov: "QuickTime (MOV)",
		matroska: "Matroska (MKV)",
		webm: "WebM",
		avi: "AVI",
		mpegts: "MPEG transport stream",
	};
	return names.length === 1 ? (labels[names[0]] ?? "Other container") : "Other container family";
}

export function audioLayout(channels: number | null, layout: string | null): string {
	if (layout === "mono" && channels === 1) return "Mono";
	if (layout === "stereo" && channels === 2) return "Stereo";
	const count = channels ? `${channels} ch` : "Channels unknown";
	return layout ? `${count} (${layout})` : count;
}
