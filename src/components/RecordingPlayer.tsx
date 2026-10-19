import { lazy, Suspense, useEffect, useState } from "react";
import { getPlaybackBackend, type PlaybackBackend } from "../services/playback";
import { errorMessage, playbackUrl, type MediaInfo, type Recording } from "../services/media";
import Player from "./Player";
import styles from "./Player.module.scss";
import type { PlaybackPosition } from "../services/playback";
import type { RefObject } from "react";
import type { EditorBridge } from "../services/editor";

const NativePlayer = lazy(() => import("./native/NativePlayer"));

export default function RecordingPlayer({
	recording,
	info,
	selecting,
	onFullscreenChange,
	initialPosition,
	onPosition,
	editorBridge,
}: {
	recording: Recording;
	info: MediaInfo | null;
	selecting: boolean;
	onFullscreenChange?: (value: boolean) => void;
	initialPosition?: PlaybackPosition;
	onPosition?: (value: PlaybackPosition) => void;
	editorBridge?: RefObject<EditorBridge>;
}) {
	const [backend, setBackend] = useState<PlaybackBackend | null>(null);
	const [attempt, setAttempt] = useState(0);
	const [systemPlayback, setSystemPlayback] = useState(false);

	useEffect(() => {
		let disposed = false;
		void getPlaybackBackend()
			.then((result) => {
				if (!disposed) setBackend(result);
			})
			.catch((reason) => {
				if (!disposed)
					setBackend({
						engine: "unavailable",
						message: `Playback backend could not be checked: ${errorMessage(reason)}. Restart with npm run desktop.`,
					});
			});
		return () => {
			disposed = true;
		};
	}, [attempt]);

	const hevc = ["hevc", "h265"].includes(info?.videoCodec?.toLowerCase() ?? "");
	if (backend?.engine === "native")
		return (
			<Suspense fallback={<p role="status">Loading native player…</p>}>
				<NativePlayer
					id={recording.id}
					editorBridge={editorBridge}
					selecting={selecting}
					onFullscreenChange={onFullscreenChange}
					initialPosition={initialPosition}
					onPosition={onPosition}
				/>
			</Suspense>
		);
	if (backend?.engine === "html" || (systemPlayback && !hevc))
		return (
			<Player
				url={playbackUrl(recording)}
				name={recording.name}
				hevc={hevc}
				initialPosition={initialPosition}
				onPosition={onPosition}
			/>
		);
	return (
		<div className={styles.player}>
			<div className={styles.stage}>
				{backend ? (
					<div className={styles.error} role="alert">
						<strong>Native playback unavailable</strong>
						<p>{backend.message}</p>
						<button
							onClick={() => {
								setBackend(null);
								setAttempt(attempt + 1);
							}}
						>
							Retry native playback
						</button>
						{info?.videoCodec && !hevc && (
							<button onClick={() => setSystemPlayback(true)}>
								Try system playback
							</button>
						)}
					</div>
				) : (
					<p className={styles.status} role="status">
						Checking playback engine…
					</p>
				)}
			</div>
		</div>
	);
}
