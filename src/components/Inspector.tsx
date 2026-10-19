import type { MediaInfo, Recording } from "../services/media";
import type { ImportedMetadata } from "../services/medal";
import { bytes, time } from "../lib/format";
import { audioLayout, containerLabel } from "../lib/mediaLabels";
import MedalDetails, { ProviderSourceDetails } from "./MedalDetails";
import { commonClaim, localizedDate } from "../lib/inspector";
import { CopyValue, MetadataValues, PropertyRows } from "./InspectorValues";
import styles from "./Inspector.module.scss";

function AudioStreams({ info }: { info: MediaInfo }) {
	return (
		<section className={styles.audio} aria-label="Audio streams">
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
					<PropertyRows
						rows={[
							["Stream index", stream.index],
							["Channels", stream.channels],
							["Channel layout", stream.channelLayout],
							[
								"Language",
								stream.tags.language && stream.tags.language !== "und"
									? stream.tags.language
									: null,
							],
							["Title", stream.tags.title],
						]}
					/>
				</details>
			))}
		</section>
	);
}

export default function Inspector({
	recording,
	info,
	loading,
	error,
	onRetry,
	metadata = [],
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
	const primary = metadata.filter((entry) => entry.relation === "primary");
	const selectedGame = commonClaim(primary.map((entry) => entry.selectedValues?.game));
	const game =
		selectedGame.value ??
		(selectedGame.differs
			? undefined
			: commonClaim(primary.map((entry) => entry.data.game)).value);
	const providerDuration = commonClaim(primary.map((entry) => entry.data.duration)).value;
	const duration =
		info?.duration ?? (typeof providerDuration === "number" ? providerDuration : null);
	const importedDates = metadata
		.map((entry) => entry.importedMs)
		.filter((value) => localizedDate(value));
	const providers = [
		...new Set(
			metadata.map((entry) => (entry.provider === "medal" ? "Medal" : entry.provider)),
		),
	];
	const medal = metadata.filter((entry) => entry.provider === "medal");
	const displayTitle =
		importedTitle ||
		recording?.name
			.replace(/\.[^.]+$/, "")
			.replace(/_+/g, " ")
			.trim() ||
		recording?.name;
	return (
		<aside className={styles.inspector} aria-label="Recording details">
			<header className={styles.identity}>
				<p className={styles.kicker}>Recording</p>
				{recording ? <h1>{displayTitle}</h1> : <h2>No recording open</h2>}
				{recording && (
					<PropertyRows
						rows={[
							["Game", typeof game === "string" ? game : null],
							[
								info?.duration != null ? "Duration" : "Provider duration",
								duration != null ? time(duration) : null,
							],
							[
								"Video",
								info
									? [
											info.width && info.height
												? `${info.width} × ${info.height}`
												: null,
											info.videoCodec?.toUpperCase(),
										]
											.filter(Boolean)
											.join(" · ")
									: null,
							],
							[
								"Source",
								providers.length
									? `Local file · ${providers.join(", ")} metadata`
									: "Local file",
							],
							[
								"Metadata imported",
								importedDates.length
									? localizedDate(Math.max(...importedDates))
									: null,
							],
						]}
					/>
				)}
			</header>
			{loading && <p role="status">Reading container and streams…</p>}
			{error && (
				<div className={styles.error} role="alert">
					<p>{error}</p>
					<button onClick={onRetry} disabled={loading}>
						Retry analysis
					</button>
				</div>
			)}
			{!recording && <p>Select a recording to inspect its video and audio streams.</p>}
			{recording && (
				<>
					{info && <AudioStreams info={info} />}
					<details className={styles.section}>
						<summary>Technical details</summary>
						<PropertyRows
							rows={[
								["Filename", recording.name],
								["Format", info?.container ? containerLabel(info.container) : null],
								["Video codec", info?.videoCodec?.toUpperCase()],
								[
									"Resolution",
									info?.width && info.height
										? `${info.width} × ${info.height}`
										: null,
								],
								[
									"Frame rate",
									info?.frameRate
										? `${info.frameRate.toLocaleString(undefined, { maximumFractionDigits: 3 })} fps`
										: null,
								],
								["File size", info ? bytes(info.fileSize) : null],
								[
									"Overall bitrate",
									info?.bitrate
										? `${(info.bitrate / 1_000_000).toLocaleString(undefined, { maximumFractionDigits: 2 })} Mb/s`
										: null,
								],
							]}
						/>
						{!info && !loading && <p>Inspected media properties are not available.</p>}
					</details>
					{medal.length > 0 && (
						<details className={styles.section}>
							<summary>Medal metadata</summary>
							<MedalDetails entries={medal} />
						</details>
					)}
					<details className={styles.section}>
						<summary>Advanced &amp; source data</summary>
						<PropertyRows
							rows={[
								[
									"File location",
									<CopyValue value={recording.path} label="file path" />,
								],
								["Demuxer names", info?.container],
							]}
						/>
						{info && (
							<details>
								<summary>Exact media values</summary>
								<MetadataValues
									exact
									value={{
										durationSeconds: info.duration,
										frameRate: info.frameRate,
										fileBytes: info.fileSize,
										overallBitrate: info.bitrate,
									}}
								/>
							</details>
						)}
						{info?.audio.some((stream) => Object.keys(stream.tags).length > 0) && (
							<details>
								<summary>Audio stream metadata</summary>
								{info.audio.map((stream, index) =>
									Object.keys(stream.tags).length > 0 ? (
										<div key={stream.index}>
											<h3>Stream {index + 1}</h3>
											<MetadataValues value={stream.tags} exact />
										</div>
									) : null,
								)}
							</details>
						)}
						<ProviderSourceDetails entries={metadata} />
					</details>
				</>
			)}
		</aside>
	);
}
