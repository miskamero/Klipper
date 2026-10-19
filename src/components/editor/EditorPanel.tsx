import { useEffect, useRef, useState, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";
import { errorMessage } from "../../services/media";
import {
	clampRange,
	parseTime,
	preciseTime,
	resolveRange,
	type EditProject,
	type EditorBridge,
} from "../../services/editor";
import type { Encoder } from "../../services/exports";
import TrimTimeline from "./TrimTimeline";
import styles from "./Editor.module.scss";

export default function EditorPanel({
	project: initial,
	bridge,
	onState,
	onQueued,
}: {
	project: EditProject;
	bridge: RefObject<EditorBridge>;
	onState: (state: { dirty: boolean; busy: boolean }) => void;
	onQueued: (id: number) => void;
}) {
	const [project, setProject] = useState(initial);
	const [selection, setSelection] = useState({ start: initial.start, end: initial.end });
	const [position, setPosition] = useState(0);
	const [texts, setTexts] = useState({
		start: preciseTime(initial.start),
		end: preciseTime(initial.end),
	});
	const [error, setError] = useState("");
	const [resolving, setResolving] = useState(false);
	const [destination, setDestination] = useState("");
	const [encoder, setEncoder] = useState<Encoder>("auto");
	const [support, setSupport] = useState<{ nvenc: boolean; detail: string } | null>(null);
	const [starting, setStarting] = useState(false);
	const saved = `${initial.start}:${initial.end}`;
	const pending = useRef<{ start: number; end: number } | null>(null);
	const latest = useRef(selection);
	const running = useRef(false);
	const alive = useRef(true);
	const exporting = starting;
	const dirty = `${selection.start}:${selection.end}` !== saved;
	useEffect(() => {
		onState({ dirty, busy: exporting || resolving });
	}, [dirty, exporting, resolving, onState]);
	useEffect(() => {
		alive.current = true;
		const port = bridge.current;
		port.command?.({ kind: "previewRange", value: [initial.start, initial.end] });
		port.report = (snapshot) => setPosition(snapshot.position);
		return () => {
			alive.current = false;
			delete port.report;
		};
	}, [bridge, initial.start, initial.end]);
	useEffect(() => {
		let disposed = false;
		void invoke<{ nvenc: boolean; detail: string }>("export_encoders")
			.then((value) => {
				if (!disposed && value) setSupport(value);
			})
			.catch(() => {
				if (!disposed)
					setSupport({
						nvenc: false,
						detail: "Could not verify NVIDIA encoding. CPU encoding remains available.",
					});
			});
		return () => {
			disposed = true;
		};
	}, []);
	function change(start: number, end: number) {
		const next = clampRange(start, end, initial.info.duration);
		const seek = next.start !== latest.current.start ? next.start : next.end;
		latest.current = next;
		setSelection(next);
		setTexts({ start: preciseTime(next.start), end: preciseTime(next.end) });
		bridge.current.command?.({ kind: "previewRange", value: [next.start, next.end] });
		bridge.current.command?.({ kind: "pause", value: true });
		bridge.current.command?.({ kind: "seek", value: seek });
	}
	async function commit() {
		pending.current = latest.current;
		if (running.current) return;
		running.current = true;
		setResolving(true);
		setError("");
		try {
			while (pending.current && alive.current) {
				const next = pending.current;
				pending.current = null;
				let result: EditProject | undefined;
				for (let retry = 0; retry < 75 && alive.current; retry++) {
					try {
						result = await resolveRange(initial.token, next.start, next.end);
						break;
					} catch (reason) {
						if (!String(reason).includes("busy") || retry === 74) throw reason;
						await new Promise((resolve) => setTimeout(resolve, 200));
					}
				}
				if (!alive.current || !result) return;
				setProject(result);
				if (!pending.current) {
					latest.current = { start: result.start, end: result.end };
					setSelection(latest.current);
					setTexts({ start: preciseTime(result.start), end: preciseTime(result.end) });
					bridge.current.command?.({
						kind: "previewRange",
						value: [result.start, result.end],
					});
					bridge.current.command?.({ kind: "seek", value: result.start });
				}
			}
		} catch (reason) {
			if (alive.current) setError(errorMessage(reason));
		} finally {
			running.current = false;
			if (alive.current) setResolving(false);
		}
	}
	async function action(run: () => Promise<void>) {
		setError("");
		try {
			await run();
		} catch (reason) {
			if (alive.current) setError(errorMessage(reason));
		}
	}
	const synced = selection.start === project.start && selection.end === project.end;
	const inputsValid = parseTime(texts.start) !== null && parseTime(texts.end) !== null;
	return (
		<section className={styles.editor} aria-label="Video editor">
			<TrimTimeline
				token={initial.token}
				duration={initial.info.duration}
				start={selection.start}
				end={selection.end}
				position={position}
				disabled={exporting}
				onRange={change}
				onCommit={() => void commit()}
				onSeek={(value) => bridge.current.command?.({ kind: "seek", value })}
			/>
			<div className={styles.rangeControls}>
				{(["start", "end"] as const).map((edge) => (
					<label key={edge}>
						{edge === "start" ? "Start" : "End"}
						<input
							aria-label={edge === "start" ? "Trim start time" : "Trim end time"}
							value={texts[edge]}
							disabled={exporting}
							onChange={(event) => setTexts({ ...texts, [edge]: event.target.value })}
							onBlur={() => {
								if (texts[edge] === preciseTime(selection[edge])) return;
								const value = parseTime(texts[edge]);
								if (value === null) {
									setError("Use HH:MM:SS.mmm or seconds.");
									return;
								}
								change(
									edge === "start" ? value : selection.start,
									edge === "end" ? value : selection.end,
								);
								void commit();
							}}
							onKeyDown={(event) => {
								if (event.key === "Enter") event.currentTarget.blur();
							}}
						/>
					</label>
				))}
				<span className={styles.result}>
					Selection <strong>{preciseTime(selection.end - selection.start)}</strong>
				</span>
				<button
					disabled={exporting}
					onClick={() => bridge.current.command?.({ kind: "frameStep", value: false })}
					title="Previous decoded frame"
				>
					← Frame
				</button>
				<button
					disabled={exporting}
					onClick={() => bridge.current.command?.({ kind: "frameStep", value: true })}
					title="Next decoded frame"
				>
					Frame →
				</button>
				<button
					disabled={exporting || resolving}
					title="Use the current playback position as the start of the exported video"
					onClick={() => {
						change(position, selection.end);
						void commit();
					}}
				>
					Set start here
				</button>
				<button
					disabled={exporting || resolving}
					title="Use the current playback position as the end of the exported video"
					onClick={() => {
						change(selection.start, position);
						void commit();
					}}
				>
					Set end here
				</button>
			</div>
			<div className={styles.exportControls}>
				<label>
					Encoding
					<select
						aria-label="Export encoder"
						value={encoder}
						disabled={exporting}
						onChange={(event) => setEncoder(event.target.value as Encoder)}
						title={support?.detail}
					>
						<option value="auto">
							Automatic
							{support
								? support.nvenc
									? " · NVIDIA"
									: " · CPU fallback"
								: " · checking…"}
						</option>
						<option value="cpu">CPU · H.264</option>
						<option value="nvenc" disabled={!support?.nvenc}>
							NVIDIA · H.264
						</option>
					</select>
				</label>
				<div>
					<strong>H.264 MP4</strong>
					<p>
						Original timing · {initial.info.audio.length} separate audio{" "}
						{initial.info.audio.length === 1 ? "track" : "tracks"} · AAC re-encoded, no
						mixing
					</p>
				</div>
				<button
					disabled={exporting || resolving}
					onClick={() =>
						void action(async () => {
							setStarting(true);
							try {
								const path = await invoke<string | null>("editor_destination", {
									token: initial.token,
								});
								if (path) setDestination(path);
							} finally {
								if (alive.current) setStarting(false);
							}
						})
					}
				>
					Save as…
				</button>
				<button
					className={styles.primary}
					disabled={exporting || resolving || !synced || !inputsValid || !destination}
					onClick={() =>
						void action(async () => {
							setStarting(true);
							try {
								const id = await invoke<number>("editor_export", {
									token: initial.token,
									encoder,
								});
								onQueued(id);
							} finally {
								if (alive.current) setStarting(false);
							}
						})
					}
				>
					Export new video
				</button>
			</div>
			{destination && (
				<p className={styles.path} title={destination}>
					Destination: {destination}
				</p>
			)}
			{resolving && <p role="status">Aligning start/end points to source frames…</p>}
			{error && <p role="alert">{error}</p>}
			<p className={styles.note}>
				Export adds a background job; you can leave the editor and keep using Klipper. The
				start frame is included; the end frame is excluded. Points snap forward to decoded
				frame boundaries. Play stops at the end; press Play again to restart at the start.
				Seeking pauses playback and can go outside the selection to choose boundaries.
				Playback volume and speed do not affect export. Preview uses the player's selected
				audio track; all supported tracks are exported separately.
			</p>
		</section>
	);
}
