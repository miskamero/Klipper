import type { MediaInfo, Recording } from "../services/media";
import { bytes, time } from "../lib/format";
import { audioLayout, containerLabel } from "../lib/mediaLabels";
import styles from "./Inspector.module.scss";
import MedalDetails from "./MedalDetails";
import type { ImportedMetadata } from "../services/medal";

export default function Inspector({
	recording,
	info,
	loading,
	error,
	onRetry,
	metadata,
	importedTitle,
}: {
	recording: Recording | null;
	info: MediaInfo | null;
	loading: boolean;
	error: string;
	onRetry: () => void;
	metadata?: ImportedMetadata[];
	importedTitle?: string | null;
}) {
	const displayTitle =
		importedTitle ||
		recording?.name
			.replace(/\.[^.]+$/, "")
			.replace(/_+/g, " ")
			.trim() ||
		recording?.name;
	return (
		<aside className={styles.inspector} aria-label="Recording details">
			<div className={styles.identity}>
				<p className={styles.kicker}>
					{recording ? "LOCAL RECORDING" : "RECORDING DETAILS"}
				</p>
				{recording ? (
					<>
						<h1>{displayTitle}</h1>
						<p>Read-only preview</p>
					</>
				) : (
					<h2>No recording open</h2>
				)}
			</div>
			{metadata && metadata.length > 0 && <MedalDetails entries={metadata} />}
			{recording && (
				<details className={styles.file}>
					<summary>Source file</summary>
					<dl>
						<div>
							<dt>Filename</dt>
							<dd>{recording.name}</dd>
						</div>
						<div>
							<dt>Location</dt>
							<dd>{recording.path}</dd>
						</div>
					</dl>
				</details>
			)}
			{recording && <h2 className={styles.propertiesHeading}>Recording properties</h2>}
			{loading && <p role="status">Reading container and streams…</p>}
			{error && (
				<p className={styles.error} role="alert">
					{error}
				</p>
			)}
			{error && (
				<button onClick={onRetry} disabled={loading}>
					Retry analysis
				</button>
			)}
			{!info && !loading && !error && (
				<p>Select a recording to inspect its video and audio streams.</p>
			)}
			{info && (
				<>
					<dl>
						{[
							["Format", containerLabel(info.container)],
							["Video codec", info.videoCodec?.toUpperCase() ?? "No video stream"],
							[
								"Resolution",
								info.width && info.height ? `${info.width} × ${info.height}` : "—",
							],
							[
								"Frame rate",
								info.frameRate ? `${info.frameRate.toFixed(3)} fps` : "—",
							],
							["Duration", time(info.duration)],
							["File size", bytes(info.fileSize)],
							[
								"Bitrate",
								info.bitrate
									? `${(info.bitrate / 1_000_000).toFixed(2)} Mb/s`
									: "—",
							],
						].map(([label, value]) => (
							<div key={label}>
								<dt>{label}</dt>
								<dd>{value}</dd>
							</div>
						))}
					</dl>
					{info.container && (
						<details className={styles.technical}>
							<summary>Container technical details</summary>
							<dl>
								<div>
									<dt>Demuxer names</dt>
									<dd>{info.container}</dd>
								</div>
							</dl>
						</details>
					)}
					<div className={styles.audioHeading}>
						<h3>Audio streams</h3>
						<span>{info.audio.length}</span>
					</div>
					<p className={styles.note}>
						System-selected audio. Independent mixing is not available.
					</p>
					{info.audio.length === 0 && <p>No audio streams detected.</p>}
					{info.audio.map((stream, position) => (
						<details className={styles.track} key={stream.index}>
							<summary>
								<span className={styles.number}>
									{String(position + 1).padStart(2, "0")}
								</span>
								<span className={styles.codec}>
									{stream.codec?.toUpperCase() ?? "Unknown codec"}
								</span>
								<span className={styles.layout}>
									{audioLayout(stream.channels, stream.channelLayout)}
								</span>
								{stream.tags.language && stream.tags.language !== "und" && (
									<span className={styles.language}>{stream.tags.language}</span>
								)}
							</summary>
							<dl>
								<div>
									<dt>Stream index</dt>
									<dd>{stream.index}</dd>
								</div>
								<div>
									<dt>Channels</dt>
									<dd>{stream.channels ?? "Unknown"}</dd>
								</div>
								{stream.channelLayout && (
									<div>
										<dt>Channel layout</dt>
										<dd>{stream.channelLayout}</dd>
									</div>
								)}
								{Object.entries(stream.tags).map(([key, value]) => (
									<div key={key}>
										<dt>{key}</dt>
										<dd>{value}</dd>
									</div>
								))}
							</dl>
						</details>
					))}
				</>
			)}
		</aside>
	);
}
