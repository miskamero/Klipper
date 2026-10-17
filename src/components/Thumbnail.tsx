import { useEffect, useRef, useState, type RefObject } from "react";
import { isTauri } from "@tauri-apps/api/core";
import type { LibraryRecording } from "../services/library";
import { thumbnails } from "../services/thumbnails";
import styles from "./Thumbnail.module.scss";

export default function Thumbnail({
	recording,
	active,
	root,
}: {
	recording: LibraryRecording;
	active: boolean;
	root: RefObject<HTMLDivElement | null>;
}) {
	const frame = useRef<HTMLSpanElement>(null);
	const [preview, setPreview] = useState<{ key: string; url: string; error: string } | null>(
		null,
	);
	const key = JSON.stringify([
		recording.previewRevision ?? "ffmpeg-320x180-q5-v1",
		recording.id,
		recording.size,
		recording.revision,
		recording.status,
		recording.availability,
	]);
	const available = recording.availability === "available";

	useEffect(() => {
		if (
			!active ||
			!available ||
			!isTauri() ||
			!frame.current ||
			typeof IntersectionObserver === "undefined"
		)
			return;
		let cancel: (() => void) | undefined;
		let disposed = false;
		const clear = () => {
			cancel?.();
			cancel = undefined;
		};
		const observer = new IntersectionObserver(
			([entry]) => {
				if (entry.isIntersecting) {
					if (cancel) return;
					setPreview(null);
					cancel = thumbnails.request(recording.id, key, (url, error) => {
						if (disposed) return;
						setPreview({ key, url, error });
					});
				} else {
					clear();
					setPreview(null);
				}
			},
			{ root: root.current, threshold: 0.01 },
		);
		observer.observe(frame.current);
		return () => {
			disposed = true;
			observer.disconnect();
			clear();
			setPreview(null);
		};
	}, [active, available, key, recording.id, root]);

	const current = active && preview?.key === key ? preview : null;
	const error = !available ? "Recording is unavailable" : current?.error;
	return (
		<span
			ref={frame}
			className={styles.frame}
			data-thumbnail-state={current?.url ? "ready" : error ? "error" : "pending"}
			title={error || undefined}
		>
			{current?.url && !error ? (
				<img
					src={current.url}
					alt=""
					width={320}
					height={180}
					onError={() =>
						setPreview({ key, url: "", error: "Preview image could not be displayed" })
					}
				/>
			) : (
				<span
					className={styles.placeholder}
					role="img"
					aria-label={error ? "Preview unavailable: " + error : "Preview loading"}
				>
					<svg
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						strokeWidth="1.5"
						aria-hidden="true"
					>
						<rect x="3" y="5" width="18" height="14" rx="2" />
						{error ? <path d="m8 8 8 8m0-8-8 8" /> : <path d="m10 9 5 3-5 3Z" />}
					</svg>
				</span>
			)}
		</span>
	);
}
