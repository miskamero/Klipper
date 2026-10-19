import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { preciseTime } from "../../services/editor";
import styles from "./Editor.module.scss";

function Storyboard({ token, duration }: { token: number; duration: number }) {
	const [frames, setFrames] = useState<string[]>([]);
	useEffect(() => {
		let disposed = false;
		const urls: string[] = [];
		async function load() {
			for (let index = 0; index < 8 && !disposed; index++) {
				let bytes: ArrayBuffer | number[] | null = null;
				// A range seek may temporarily own the editor slot. One request at a time.
				for (let retry = 0; retry < 60 && !disposed; retry++) {
					try {
						bytes = await invoke<ArrayBuffer | number[]>("editor_frame", {
							token,
							seconds: ((index + 0.5) / 8) * duration,
						});
						break;
					} catch (error) {
						if (!String(error).includes("busy")) break;
						await new Promise((resolve) => setTimeout(resolve, 200));
					}
				}
				if (disposed) return;
				const buffer =
					bytes instanceof ArrayBuffer ? bytes : Uint8Array.from(bytes ?? []).buffer;
				urls[index] = buffer.byteLength
					? URL.createObjectURL(new Blob([buffer], { type: "image/jpeg" }))
					: "";
				setFrames([...urls]);
			}
		}
		void load();
		return () => {
			disposed = true;
			urls.forEach((url) => url && URL.revokeObjectURL(url));
		};
	}, [token, duration]);
	return (
		<div className={styles.storyboard} aria-label="Sampled video storyboard">
			{Array.from({ length: 8 }, (_, index) => (
				<div key={index}>
					{frames[index] ? (
						<img src={frames[index]} alt="" />
					) : (
						<span aria-hidden="true">·</span>
					)}
				</div>
			))}
		</div>
	);
}

export default function TrimTimeline({
	token,
	duration,
	start,
	end,
	position,
	disabled,
	onRange,
	onCommit,
	onSeek,
}: {
	token: number;
	duration: number;
	start: number;
	end: number;
	position: number;
	disabled: boolean;
	onRange: (start: number, end: number) => void;
	onCommit: () => void;
	onSeek: (seconds: number) => void;
}) {
	const track = useRef<HTMLDivElement>(null);
	const drag = useRef<"start" | "end" | null>(null);
	const percentage = (value: number) => `${(value / duration) * 100}%`;
	return (
		<div className={styles.timeline}>
			<div className={styles.ruler} aria-hidden="true">
				{Array.from({ length: 5 }, (_, index) => (
					<span key={index}>{preciseTime((duration * index) / 4)}</span>
				))}
			</div>
			<div
				ref={track}
				className={styles.track}
				onPointerMove={(event) => {
					if (!drag.current || disabled) return;
					const bounds = track.current!.getBoundingClientRect();
					const value = Math.max(
						0,
						Math.min(
							duration,
							((event.clientX - bounds.left) / bounds.width) * duration,
						),
					);
					onRange(
						drag.current === "start" ? value : start,
						drag.current === "end" ? value : end,
					);
				}}
				onPointerUp={() => {
					if (drag.current) onCommit();
					drag.current = null;
				}}
				onPointerCancel={() => {
					drag.current = null;
					onCommit();
				}}
			>
				<Storyboard token={token} duration={duration} />
				<div
					className={styles.selection}
					style={{ left: percentage(start), right: percentage(duration - end) }}
				/>
				<input
					className={styles.playheadSeek}
					type="range"
					aria-label="Timeline playhead"
					min={0}
					max={duration}
					step={0.001}
					value={Math.min(position, duration)}
					disabled={disabled}
					onChange={(event) => onSeek(Number(event.target.value))}
				/>
				<div
					className={styles.playhead}
					style={{ left: percentage(Math.min(duration, position)) }}
				/>
				{(["start", "end"] as const).map((edge) => (
					<button
						key={edge}
						className={styles.handle}
						style={{ left: percentage(edge === "start" ? start : end) }}
						role="slider"
						aria-label={edge === "start" ? "Trim start" : "Trim end"}
						aria-valuemin={edge === "start" ? 0 : start + 0.001}
						aria-valuemax={edge === "start" ? end - 0.001 : duration}
						aria-valuenow={edge === "start" ? start : end}
						aria-valuetext={preciseTime(edge === "start" ? start : end)}
						disabled={disabled}
						onPointerDown={(event) => {
							event.currentTarget.setPointerCapture(event.pointerId);
							drag.current = edge;
						}}
						onKeyDown={(event) => {
							if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
								return;
							event.preventDefault();
							const current = edge === "start" ? start : end;
							const value =
								event.key === "Home"
									? 0
									: event.key === "End"
										? duration
										: current +
											(event.key === "ArrowLeft" ? -1 : 1) *
												(event.shiftKey ? 1 : 0.001);
							onRange(edge === "start" ? value : start, edge === "end" ? value : end);
						}}
						onKeyUp={(event) => {
							if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key))
								onCommit();
						}}
					>
						Ⅱ
					</button>
				))}
			</div>
		</div>
	);
}
