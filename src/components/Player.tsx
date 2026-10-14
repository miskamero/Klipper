import { useRef, useState } from "react";
import { playbackError, time } from "../lib/format";
import styles from "./Player.module.scss";

const playbackRates = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3];

export default function Player({ url, name }: { url: string; name: string }) {
	const videoRef = useRef<HTMLVideoElement>(null);
	const playerRef = useRef<HTMLDivElement>(null);
	const [status, setStatus] = useState("Loading recording...");
	const [error, setError] = useState("");
	const [controlError, setControlError] = useState("");
	const [playing, setPlaying] = useState(false);
	const [duration, setDuration] = useState(0);
	const [position, setPosition] = useState(0);
	const [volume, setVolume] = useState(1);
	const [muted, setMuted] = useState(false);
	const [rate, setRate] = useState(1);
	const [rateError, setRateError] = useState("");

	function changeRate(requested: number) {
		const video = videoRef.current;
		if (!video) return;
		setRateError("");
		try {
			video.playbackRate = requested;
			if (video.playbackRate !== requested)
				setRateError(
					"This playback engine could not apply that speed. The active speed is shown.",
				);
		} catch {
			setRateError(
				"This playback engine does not support that speed. Choose another speed or reset to 1×.",
			);
		}
		setRate(video.playbackRate);
	}

	function updateDuration(video: HTMLVideoElement) {
		setDuration(Number.isFinite(video.duration) ? video.duration : 0);
	}

	async function togglePlayback() {
		const video = videoRef.current;
		if (!video) return;
		setControlError("");
		if (!video.paused) {
			video.pause();
			return;
		}
		try {
			await video.play();
		} catch {
			setControlError("Playback could not start. Try Play again or open another recording.");
		}
	}

	async function fullscreen() {
		setControlError("");
		try {
			if (document.fullscreenElement) await document.exitFullscreen();
			else await playerRef.current?.requestFullscreen();
		} catch {
			setControlError("Fullscreen is unavailable in this window.");
		}
	}

	return (
		<div className={styles.player} ref={playerRef}>
			<div className={styles.stage}>
				<video
					ref={videoRef}
					src={url}
					preload="metadata"
					playsInline
					aria-label={name}
					onLoadedMetadata={(event) => {
						updateDuration(event.currentTarget);
						setStatus("");
					}}
					onDurationChange={(event) => updateDuration(event.currentTarget)}
					onTimeUpdate={(event) => setPosition(event.currentTarget.currentTime)}
					onPlay={() => setPlaying(true)}
					onPause={() => setPlaying(false)}
					onEnded={() => setPlaying(false)}
					onVolumeChange={(event) => {
						setVolume(event.currentTarget.volume);
						setMuted(event.currentTarget.muted);
					}}
					onRateChange={(event) => setRate(event.currentTarget.playbackRate)}
					onCanPlay={() => setStatus("")}
					onPlaying={() => setStatus("")}
					onWaiting={() => setStatus("Buffering...")}
					onSeeked={() => setStatus("")}
					onError={(event) => {
						setError(playbackError(event.currentTarget.error?.code));
						setStatus("");
						setPlaying(false);
					}}
				/>
				{status && !error && (
					<p className={styles.status} role="status">
						{status}
					</p>
				)}
				{error && (
					<div className={styles.error} role="alert">
						<strong>Playback unavailable</strong>
						<p>{error}</p>
					</div>
				)}
			</div>
			<div className={styles.transport} role="group" aria-label="Playback controls">
				<button
					className={styles.play}
					onClick={() => void togglePlayback()}
					disabled={!!error}
					aria-label={playing ? "Pause" : "Play"}
				>
					<svg
						width="16"
						height="16"
						viewBox="0 0 16 16"
						fill="currentColor"
						aria-hidden="true"
					>
						{playing ? (
							<path d="M4 3h3v10H4zm5 0h3v10H9z" />
						) : (
							<path d="m5 2 9 6-9 6z" />
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
					disabled={!duration || !!error}
					aria-valuetext={`${time(position)} of ${time(duration)}`}
					onChange={(event) => {
						const value = Number(event.target.value);
						if (videoRef.current) videoRef.current.currentTime = value;
						setPosition(value);
					}}
				/>
				<div className={styles.volume}>
					<button
						aria-label={muted ? "Unmute" : "Mute"}
						aria-pressed={muted}
						onClick={() => {
							if (videoRef.current) videoRef.current.muted = !videoRef.current.muted;
						}}
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
							{muted ? (
								<path d="m11 6 4 4m0-4-4 4" />
							) : (
								<path d="M11 5a4 4 0 0 1 0 6m2-8a7 7 0 0 1 0 10" />
							)}
						</svg>
					</button>
					<input
						type="range"
						aria-label="Volume"
						min="0"
						max="1"
						step="0.01"
						value={muted ? 0 : volume}
						aria-valuetext={`${Math.round((muted ? 0 : volume) * 100)} percent`}
						onChange={(event) => {
							if (videoRef.current) {
								videoRef.current.volume = Number(event.target.value);
								videoRef.current.muted = false;
							}
						}}
					/>
				</div>
				<select
					className={styles.speed}
					aria-label="Playback speed"
					title="Playback speed — select 1× to reset"
					value={rate}
					disabled={!!error}
					onChange={(event) => changeRate(Number(event.target.value))}
				>
					{!playbackRates.includes(rate) && <option value={rate}>{rate}×</option>}
					{playbackRates.map((value) => (
						<option key={value} value={value}>
							{value}×
						</option>
					))}
				</select>
				{document.fullscreenEnabled && (
					<button onClick={() => void fullscreen()} aria-label="Toggle fullscreen">
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
				)}
			</div>
			{controlError && (
				<p className={styles.controlError} role="alert">
					{controlError}
				</p>
			)}
			{rateError && (
				<p className={styles.controlError} role="alert">
					{rateError}
				</p>
			)}
		</div>
	);
}
