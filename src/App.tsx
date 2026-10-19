import { useEffect, useRef, useState } from "react";
import Inspector from "./components/Inspector";
import InspectorResize, { useInspectorWidth } from "./components/InspectorResize";
import RecordingPlayer from "./components/RecordingPlayer";
import Library from "./components/Library";
import TitleBar from "./components/TitleBar";
import useRecordingWorkspaces from "./useRecordingWorkspaces";
import styles from "./App.module.scss";
import EditorPanel from "./components/editor/EditorPanel";
import { beginEdit, closeEdit, type EditProject, type EditorBridge } from "./services/editor";
import { errorMessage } from "./services/media";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useExports, openExport } from "./services/exports";
import ExportQueue from "./components/editor/ExportQueue";

export default function App() {
	const session = useRecordingWorkspaces();
	const exports = useExports();
	const [queueExpanded, setQueueExpanded] = useState(false);
	const [closeRequested, setCloseRequested] = useState(false);
	const { view, selecting } = session;
	const recording = session.current?.recording ?? null;
	const info = session.current?.info ?? null;
	const analyzing = session.current?.analyzing ?? false;
	const analysisError = session.current?.error ?? "";
	const selectionError = session.error;
	const [inspectorOpen, setInspectorOpen] = useState(true);
	const [videoFullscreen, setVideoFullscreen] = useState(false);
	const [edit, setEdit] = useState<EditProject | null>(null);
	const [editError, setEditError] = useState("");
	const [editPreparing, setEditPreparing] = useState(false);
	const [leaveAction, setLeaveAction] = useState<(() => void) | null>(null);
	const editorState = useRef({ dirty: false, busy: false });
	const bridge = useRef<EditorBridge>({});
	const originalPosition = useRef(0);
	const mounted = useRef(true);
	const editToken = useRef<number | null>(null);
	editToken.current = edit?.token ?? null;
	useEffect(() => {
		mounted.current = true;
		return () => {
			mounted.current = false;
			queueMicrotask(() => {
				if (!mounted.current && editToken.current !== null)
					void closeEdit(editToken.current, true).catch(() => {});
			});
		};
	}, []);
	const updateEditorState = useRef((state: { dirty: boolean; busy: boolean }) => {
		editorState.current = state;
	}).current;
	async function leaveEditor(action: () => void) {
		if (editPreparing) return;
		if (edit && editorState.current.busy) {
			setEditError("Finish or cancel the editor operation before leaving.");
			return;
		}
		if (edit && editorState.current.dirty) {
			setLeaveAction(() => action);
			return;
		}
		await finishEditor(action);
	}
	async function finishEditor(action: () => void) {
		try {
			if (edit) {
				await closeEdit(edit.token);
				bridge.current.command?.({ kind: "clearPreview" });
				bridge.current.command?.({ kind: "pause", value: true });
				bridge.current.command?.({ kind: "seek", value: originalPosition.current });
			}
			setEdit(null);
			setLeaveAction(null);
			setEditError("");
			editorState.current = { dirty: false, busy: false };
			action();
		} catch (reason) {
			setEditError(errorMessage(reason));
		}
	}
	async function enterEditor() {
		if (!recording) return;
		setEditError("");
		if (!bridge.current.command) {
			setEditError(
				"The editor requires native playback. Open this recording in Windows Klipper with libmpv available.",
			);
			return;
		}
		setEditPreparing(true);
		originalPosition.current = bridge.current.snapshot?.position ?? 0;
		try {
			if ((await bridge.current.fullscreen?.(false)) === false)
				throw new Error("Exit fullscreen before opening the editor.");
			bridge.current.command({ kind: "pause", value: true });
			const project = await beginEdit(recording.id);
			if (!mounted.current) {
				await closeEdit(project.token, true);
				return;
			}
			setEdit(project);
		} catch (reason) {
			setEditError(errorMessage(reason));
		} finally {
			setEditPreparing(false);
		}
	}
	const workspace = useRef<HTMLDivElement>(null);
	const details = useRef<HTMLDivElement>(null);
	const inspector = useInspectorWidth(workspace, view !== "library");
	useEffect(() => {
		if (!isTauri()) return;
		const subscription = listen("editor-window-close-requested", () => setCloseRequested(true));
		return () => {
			void subscription.then((stop) => stop());
		};
	}, []);
	useEffect(() => {
		if (details.current) details.current.scrollTop = 0;
	}, [recording?.id]);
	return (
		<div
			data-workspace={view}
			className={`${styles.app} ${videoFullscreen ? styles.videoFullscreen : ""}`}
		>
			<TitleBar
				exportSummary={exports.jobs.length ? String(exports.jobs.length) : undefined}
				onShowExports={() =>
					void leaveEditor(() => {
						session.navigate("library");
						setQueueExpanded(true);
					})
				}
				workspace={view === "player" ? "player" : "library"}
				showInspector={view !== "library" && !edit}
				onWorkspaceChange={(next) => void leaveEditor(() => session.navigate(next))}
				selecting={selecting || editPreparing}
				hasRecording={session.hasStandalone}
				inspectorOpen={inspectorOpen}
				onOpen={() =>
					void leaveEditor(() => {
						void session.open();
					})
				}
				onToggleInspector={() => setInspectorOpen(!inspectorOpen)}
			/>
			<main className={styles.main} data-workspace={view}>
				{closeRequested && (
					<div
						className={styles.closeDialog}
						role="alertdialog"
						aria-modal="true"
						aria-label="Close Klipper?"
						onKeyDown={(event) => {
							if (event.key === "Escape") setCloseRequested(false);
						}}
					>
						<strong>Close Klipper?</strong>
						<p>
							Closing cancels unfinished exports. Unsaved edits will be discarded.
							Original recordings remain unchanged.
						</p>
						<button autoFocus onClick={() => setCloseRequested(false)}>
							Keep Klipper open
						</button>
						<button
							onClick={() =>
								void invoke("exports_close").catch((reason) =>
									setEditError(String(reason)),
								)
							}
						>
							Close and cancel unfinished work
						</button>
					</div>
				)}
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
				{view !== "library" && (
					<div className={styles.libraryBack}>
						{edit ? (
							<button onClick={() => void leaveEditor(() => {})}>
								← Back to player
							</button>
						) : (
							<>
								{view === "libraryPlayback" && (
									<button
										onClick={() =>
											void leaveEditor(() => session.navigate("library"))
										}
									>
										<span aria-hidden="true">←</span> Back to library
									</button>
								)}
								{recording && (
									<button
										disabled={selecting || editPreparing}
										onClick={() => void enterEditor()}
									>
										{editPreparing ? "Preparing editor…" : "Edit video"}
									</button>
								)}
							</>
						)}
						{edit && <span>Video editor · non-destructive trim</span>}
					</div>
				)}
				{editError && (
					<p className={styles.error} role="alert">
						{editError}
					</p>
				)}
				{view !== "library" && (
					<div
						ref={workspace}
						style={{
							gridTemplateColumns:
								inspectorOpen && !edit
									? `minmax(0, 1fr) 16px ${inspector.width}px`
									: undefined,
						}}
						className={`${styles.workspace} ${!inspectorOpen || edit ? styles.expanded : ""}`}
					>
						<section className={styles.preview} aria-label="Playback">
							{recording ? (
								<RecordingPlayer
									key={recording.id}
									editorBridge={bridge}
									recording={recording}
									info={info}
									selecting={selecting || closeRequested}
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
							{leaveAction && (
								<div
									className={styles.discard}
									role="alertdialog"
									aria-label="Discard unsaved edits?"
								>
									<p>Discard unsaved edits? The source recording is unchanged.</p>
									<button autoFocus onClick={() => setLeaveAction(null)}>
										Keep editing
									</button>
									<button onClick={() => void finishEditor(leaveAction)}>
										Discard edits
									</button>
								</div>
							)}
							{edit && (
								<EditorPanel
									project={edit}
									bridge={bridge}
									onState={updateEditorState}
									onQueued={() => {
										void exports.refresh();
										void finishEditor(() => {
											setQueueExpanded(true);
											if (view === "libraryPlayback")
												session.navigate("library");
										});
									}}
								/>
							)}
						</section>
						{inspectorOpen && !edit && (
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
							hidden={!inspectorOpen || !!edit}
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
			{view === "library" && !edit && !videoFullscreen && exports.jobs.length > 0 && (
				<ExportQueue
					jobs={exports.jobs}
					expanded={queueExpanded}
					onToggle={() => setQueueExpanded(!queueExpanded)}
					error={exports.error}
					onAction={(command, id) => void exports.action(command, id)}
					onOpen={(id) =>
						void openExport(id)
							.then((output) => session.openExport(output))
							.catch((reason) => setEditError(String(reason)))
					}
				/>
			)}
		</div>
	);
}
