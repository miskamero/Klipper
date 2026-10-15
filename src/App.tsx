import { useEffect, useRef, useState } from "react";
import Inspector from "./components/Inspector";
import InspectorResize, { useInspectorWidth } from "./components/InspectorResize";
import Player from "./components/Player";
import TitleBar from "./components/TitleBar";
import {
	analyzeRecording,
	errorMessage,
	playbackUrl,
	selectRecording,
	type MediaInfo,
	type Recording,
} from "./services/media";
import styles from "./App.module.scss";

export default function App() {
	const [recording, setRecording] = useState<Recording | null>(null);
	const [info, setInfo] = useState<MediaInfo | null>(null);
	const [selecting, setSelecting] = useState(false);
	const [analyzing, setAnalyzing] = useState(false);
	const [selectionError, setSelectionError] = useState("");
	const [analysisError, setAnalysisError] = useState("");
	const [inspectorOpen, setInspectorOpen] = useState(true);
	const workspace = useRef<HTMLDivElement>(null);
	const details = useRef<HTMLDivElement>(null);
	const inspector = useInspectorWidth(workspace);
	const generation = useRef(0);
	const picking = useRef(false);
	const mounted = useRef(false);

	useEffect(() => {
		mounted.current = true;
		return () => {
			mounted.current = false;
			generation.current += 1;
		};
	}, []);

	useEffect(() => {
		if (details.current) details.current.scrollTop = 0;
	}, [recording?.id]);

	async function inspect(selected: Recording) {
		if (!mounted.current) return;
		const request = ++generation.current;
		setInfo(null);
		setAnalysisError("");
		setAnalyzing(true);
		try {
			const result = await analyzeRecording(selected.id);
			if (request === generation.current) setInfo(result);
		} catch (error) {
			if (request === generation.current) setAnalysisError(errorMessage(error));
		} finally {
			if (request === generation.current) setAnalyzing(false);
		}
	}

	async function open() {
		if (!mounted.current || picking.current) return;
		picking.current = true;
		setSelecting(true);
		setSelectionError("");
		let selected: Recording | null;
		try {
			selected = await selectRecording();
		} catch (error) {
			if (mounted.current) setSelectionError(errorMessage(error));
			return;
		} finally {
			picking.current = false;
			if (mounted.current) setSelecting(false);
		}
		if (!mounted.current || !selected) return;
		setRecording(selected);
		await inspect(selected);
	}

	return (
		<div className={styles.app}>
			<TitleBar
				selecting={selecting}
				hasRecording={!!recording}
				inspectorOpen={inspectorOpen}
				onOpen={() => void open()}
				onToggleInspector={() => setInspectorOpen(!inspectorOpen)}
			/>
			<main className={styles.main}>
				{selectionError && (
					<p className={styles.error} role="alert">
						{selectionError}
					</p>
				)}
				<div
					ref={workspace}
					style={{
						gridTemplateColumns: inspectorOpen
							? `minmax(0, 1fr) 16px ${inspector.width}px`
							: undefined,
					}}
					className={`${styles.workspace} ${!inspectorOpen ? styles.expanded : ""}`}
				>
					<section className={styles.preview} aria-label="Playback">
						{recording ? (
							<Player
								key={recording.id}
								url={playbackUrl(recording)}
								name={recording.name}
							/>
						) : (
							<div className={styles.empty}>
								<svg
									width="32"
									height="32"
									viewBox="0 0 24 24"
									fill="none"
									stroke="currentColor"
									strokeWidth="1.5"
									aria-hidden="true"
								>
									<rect x="3" y="5" width="18" height="14" rx="2" />
									<path d="m10 9 5 3-5 3Z" />
								</svg>
								<h1>No recording selected</h1>
								<p>Open a local recording to play it and inspect its streams.</p>
								<button onClick={() => void open()} disabled={selecting}>
									Open a local recording
								</button>
								<small>
									MP4 &middot; MOV &middot; MKV &middot; WebM
									<br />
									Playback support depends on your system.
								</small>
							</div>
						)}
					</section>
					{inspectorOpen && (
						<InspectorResize
							width={inspector.width}
							maximum={inspector.maximum}
							onResize={inspector.setWidth}
							className={styles.resize}
						/>
					)}
					<div
						ref={details}
						id="recording-inspector"
						hidden={!inspectorOpen}
						className={styles.details}
						tabIndex={0}
						role="region"
						aria-label="Scrollable recording details"
					>
						<Inspector
							key={recording?.id ?? "empty"}
							recording={recording}
							info={info}
							loading={analyzing}
							error={analysisError}
							onRetry={() => {
								if (recording) void inspect(recording);
							}}
						/>
					</div>
				</div>
			</main>
		</div>
	);
}
