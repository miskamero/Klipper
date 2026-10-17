import { useEffect, useRef, useState } from "react";
import Inspector from "./components/Inspector";
import InspectorResize, { useInspectorWidth } from "./components/InspectorResize";
import RecordingPlayer from "./components/RecordingPlayer";
import Library from "./components/Library";
import TitleBar from "./components/TitleBar";
import useRecordingWorkspaces from "./useRecordingWorkspaces";
import styles from "./App.module.scss";

export default function App() {
	const session = useRecordingWorkspaces();
	const { view, selecting } = session;
	const recording = session.current?.recording ?? null;
	const info = session.current?.info ?? null;
	const analyzing = session.current?.analyzing ?? false;
	const analysisError = session.current?.error ?? "";
	const selectionError = session.error;
	const [inspectorOpen, setInspectorOpen] = useState(true);
	const [videoFullscreen, setVideoFullscreen] = useState(false);
	const workspace = useRef<HTMLDivElement>(null);
	const details = useRef<HTMLDivElement>(null);
	const inspector = useInspectorWidth(workspace, view !== "library");
	useEffect(() => {
		if (details.current) details.current.scrollTop = 0;
	}, [recording?.id]);
	return (
		<div
			data-workspace={view}
			className={`${styles.app} ${videoFullscreen ? styles.videoFullscreen : ""}`}
		>
			<TitleBar
				workspace={view === "player" ? "player" : "library"}
				showInspector={view !== "library"}
				onWorkspaceChange={session.navigate}
				selecting={selecting}
				hasRecording={session.hasStandalone}
				inspectorOpen={inspectorOpen}
				onOpen={() => void session.open()}
				onToggleInspector={() => setInspectorOpen(!inspectorOpen)}
			/>
			<main className={styles.main} data-workspace={view}>
				{selectionError && (
					<p className={styles.error} role="alert">
						{selectionError}
					</p>
				)}
				<Library
					active={view === "library"}
					selecting={selecting}
					onOpen={(id) => void session.open(id)}
					onReset={session.forgetLibrary}
				/>
				{view === "libraryPlayback" && (
					<div className={styles.libraryBack}>
						<button onClick={() => session.navigate("library")}>
							<span aria-hidden="true">←</span> Back to library
						</button>
					</div>
				)}
				{view !== "library" && (
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
								<RecordingPlayer
									key={recording.id}
									recording={recording}
									info={info}
									selecting={selecting}
									initialPosition={session.current?.resume}
									onPosition={session.playbackChanged}
									onFullscreenChange={setVideoFullscreen}
								/>
							) : !session.ready ? (
								<p className={styles.empty} role="status">
									Switching recording…
								</p>
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
									<p>
										Open a local recording to play it and inspect its streams.
									</p>
									<button
										onClick={() => void session.open()}
										disabled={selecting}
									>
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
								metadata={session.current?.metadata}
								importedTitle={session.current?.displayTitle}
								info={info}
								loading={analyzing}
								error={analysisError}
								onRetry={session.retry}
							/>
						</div>
					</div>
				)}
			</main>
		</div>
	);
}
