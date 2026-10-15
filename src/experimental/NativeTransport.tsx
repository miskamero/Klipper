import type { Ref } from "react";
import { time } from "../lib/format";
import styles from "../components/Player.module.scss";
import type { Control, Snapshot } from "./nativePlayback";

const rates = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3];

export default function NativeTransport({
	snapshot,
	ready,
	command,
	requestFullscreen,
	hidden = false,
	transportRef,
	fullscreenButtonRef,
}: {
	snapshot: Snapshot | null;
	ready: boolean;
	command: (control: Control) => void;
	requestFullscreen: (value: boolean) => void;
	hidden?: boolean;
	transportRef?: Ref<HTMLDivElement>;
	fullscreenButtonRef?: Ref<HTMLButtonElement>;
}) {
	const position = snapshot?.position ?? 0;
	const duration = snapshot?.duration ?? 0;
	return (
		<div
			ref={transportRef}
			hidden={hidden}
			className={styles.transport}
			role="group"
			aria-label="Playback controls"
		>
			<button
				className={styles.play}
				aria-label={snapshot?.paused ? "Play" : "Pause"}
				disabled={!ready}
				onClick={() => command({ kind: "pause", value: !snapshot?.paused })}
			>
				<svg
					width="16"
					height="16"
					viewBox="0 0 16 16"
					fill="currentColor"
					aria-hidden="true"
				>
					{snapshot?.paused ? (
						<path d="m5 2 9 6-9 6z" />
					) : (
						<path d="M4 3h3v10H4zm5 0h3v10H9z" />
					)}
				</svg>
			</button>
			<span className={styles.time}>
				{time(position)} <span>/ {time(duration)}</span>
			</span>
			<input
				className={styles.seek}
				type="range"
				aria-label="Seek"
				min="0"
				max={duration || 1}
				step="0.1"
				value={Math.min(position, duration)}
				disabled={!ready}
				onChange={(event) => command({ kind: "seek", value: Number(event.target.value) })}
			/>
			<div className={styles.volume}>
				<button
					aria-label={snapshot?.muted ? "Unmute" : "Mute"}
					aria-pressed={snapshot?.muted ?? false}
					disabled={!ready}
					onClick={() => command({ kind: "mute", value: !snapshot?.muted })}
				>
					<svg
						width="16"
						height="16"
						viewBox="0 0 16 16"
						fill="none"
						stroke="currentColor"
						strokeWidth="1.5"
						aria-hidden="true"
					>
						<path d="M2 6h3l3-3v10l-3-3H2z" />
						{snapshot?.muted ? (
							<path d="m11 6 4 4m0-4-4 4" />
						) : (
							<path d="M11 5a4 4 0 0 1 0 6m2-8a7 7 0 0 1 0 10" />
						)}
					</svg>
				</button>
				<input
					id="native-volume"
					type="range"
					aria-label="Volume"
					min="0"
					max="100"
					step="1"
					disabled={!ready}
					value={snapshot?.muted ? 0 : (snapshot?.volume ?? 100)}
					onChange={(event) => {
						command({ kind: "volume", value: Number(event.target.value) });
						if (snapshot?.muted) command({ kind: "mute", value: false });
					}}
				/>
			</div>
			<select
				className={styles.speed}
				aria-label="Playback speed"
				value={snapshot?.speed || 1}
				disabled={!ready}
				onChange={(event) => command({ kind: "speed", value: Number(event.target.value) })}
			>
				{rates.map((value) => (
					<option key={value} value={value}>
						{value}×
					</option>
				))}
			</select>
			<button
				ref={fullscreenButtonRef}
				onClick={() => void requestFullscreen(!snapshot?.fullscreen)}
				aria-label="Toggle fullscreen"
				aria-pressed={snapshot?.fullscreen ?? false}
			>
				<svg
					width="16"
					height="16"
					viewBox="0 0 16 16"
					fill="none"
					stroke="currentColor"
					strokeWidth="1.5"
					aria-hidden="true"
				>
					<path d="M6 2H2v4m8-4h4v4M2 10v4h4m8-4v4h-4" />
				</svg>
			</button>
		</div>
	);
}
