import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import NativeTransport from "./NativeTransport";
import type { Control, Snapshot } from "./nativePlayback";
import styles from "../components/Player.module.scss";
import useVideoFullscreen from "./useVideoFullscreen";

// Uses the native worker and embedded surface validated by the integration spike.
export default function NativePlayer({
	id,
	selecting,
	onFullscreenChange,
}: {
	id: number;
	selecting: boolean;
	onFullscreenChange?: (value: boolean) => void;
}) {
	const stage = useRef<HTMLDivElement>(null);
	const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
	const [error, setError] = useState("");
	const [controlError, setControlError] = useState("");
	const [attempt, setAttempt] = useState(0);
	const alive = useRef(false);
	const pending = useRef(new Map<string, Control>());
	const hidden = useRef(selecting);
	hidden.current = selecting || !!error;

	const command = useCallback((control: Control) => {
		// Coalesce slider updates while IPC is pending; never accumulate an unbounded queue.
		pending.current.set(control.kind, control);
	}, []);
	const fullscreen = snapshot?.fullscreen ?? false;
	const requestFullscreen = useCallback(
		async (value: boolean) => {
			setControlError("");
			try {
				const result = await invoke<Snapshot>("mpv_control", {
					id,
					control: { kind: "fullscreen", value },
				});
				if (alive.current && result.id === id) setSnapshot(result);
			} catch (reason) {
				if (alive.current)
					setControlError(`Fullscreen could not change: ${String(reason)}`);
			}
		},
		[id],
	);
	const exitFullscreen = useCallback(() => void requestFullscreen(false), [requestFullscreen]);
	const focusControls = useCallback(() => {
		void invoke("mpv_overlay", { action: { kind: "focus", id } }).catch((reason) =>
			setControlError(String(reason)),
		);
	}, [id]);
	const presentation = useVideoFullscreen({
		fullscreen,
		pointer: snapshot?.pointer,
		exit: exitFullscreen,
		onChange: onFullscreenChange,
		focusControls,
	});
	const wakeControls = presentation.wakeControls;
	useEffect(() => {
		let disposed = false;
		const subscription = listen<boolean>("native-controls-activity", (event) => {
			if (!disposed) wakeControls(event.payload);
		});
		return () => {
			disposed = true;
			void subscription.then((unlisten) => unlisten());
		};
	}, [wakeControls]);
	const cursorHidden = fullscreen && !presentation.controlsVisible && !!snapshot?.pointer;
	const cursorHiddenRef = useRef(false);
	cursorHiddenRef.current = cursorHidden;
	const controlsVisible = useRef(true);
	controlsVisible.current = presentation.controlsVisible;

	useEffect(() => {
		alive.current = true;
		let disposed = false;
		let timer: ReturnType<typeof setTimeout>;
		async function poll(load: boolean) {
			try {
				const next = pending.current.values().next().value;
				if (!load && next) pending.current.delete(next.kind);
				const result = await invoke<Snapshot>("mpv_control", {
					id,
					control: load
						? { kind: attempt ? "reload" : "load" }
						: (next ?? { kind: "snapshot" }),
				});
				if (disposed) return;
				if (result.id === id) {
					setSnapshot(result);
					setError(result.error);
					if (result.error) return;
				}
			} catch (reason) {
				if (!disposed) setError(String(reason));
				return;
			}
			if (!disposed) timer = setTimeout(() => void poll(false), 100);
		}
		void poll(true);
		return () => {
			alive.current = false;
			disposed = true;
			clearTimeout(timer);
			// StrictMode replays effects on the same instance; do not stop its replacement load.
			queueMicrotask(() => {
				if (!alive.current)
					void invoke("mpv_control", { id, control: { kind: "stop" } }).catch(() => {});
			});
		};
	}, [id, attempt]);

	useEffect(() => {
		let disposed = false;
		let frame = 0;
		let previous = "";
		async function update() {
			const element = stage.current;
			if (disposed || !element) return;
			const rect = element.getBoundingClientRect();
			const x = Math.max(0, rect.left);
			const y = Math.max(0, rect.top);
			// Send CSS pixels; Rust uses the window's current native scale factor.
			const bounds = {
				x,
				y,
				width: Math.max(0, Math.min(innerWidth, rect.right) - x),
				height: Math.max(0, Math.min(innerHeight, rect.bottom) - y),
				visible: !hidden.current && !document.hidden,
				cursorHidden: cursorHiddenRef.current,
				controlsVisible: controlsVisible.current,
			};
			const key = JSON.stringify([bounds, devicePixelRatio]);
			if (previous !== key) {
				try {
					await invoke("mpv_surface", { id, bounds });
					previous = key;
				} catch (reason) {
					if (!disposed) setError(String(reason));
				}
			}
			if (!disposed) frame = requestAnimationFrame(() => void update());
		}
		void update();
		return () => {
			disposed = true;
			cancelAnimationFrame(frame);
			// Effect replay must not hide the surface after its replacement has shown it.
			queueMicrotask(() => {
				if (!alive.current)
					void invoke("mpv_surface", {
						id,
						bounds: { x: 0, y: 0, width: 0, height: 0, visible: false },
					}).catch(() => {});
			});
		};
	}, [id]);

	const duration = snapshot?.duration ?? 0;
	const ready = !!duration && !error;
	return (
		<div
			className={`${styles.player} ${fullscreen ? styles.videoFullscreen : ""}`}
			data-video-fullscreen={fullscreen}
			data-cursor-hidden={cursorHidden}
			onPointerMove={presentation.onPointerMove}
		>
			<div ref={stage} className={styles.stage} aria-label="Native video surface" />
			<NativeTransport
				snapshot={snapshot}
				ready={ready}
				command={command}
				requestFullscreen={(value) => void requestFullscreen(value)}
				hidden={fullscreen}
				transportRef={presentation.transport}
				fullscreenButtonRef={presentation.fullscreenButton}
			/>
			<div
				hidden={fullscreen && !error && !controlError}
				className={styles.controlError}
				role={error || controlError ? "alert" : "status"}
			>
				{error || controlError || `libmpv · ${snapshot?.hwdec || "Loading recording…"}`}
				{error && (
					<button
						onClick={() => {
							setError("");
							setSnapshot(null);
							pending.current.clear();
							setAttempt(attempt + 1);
						}}
					>
						Retry playback
					</button>
				)}
			</div>
		</div>
	);
}
