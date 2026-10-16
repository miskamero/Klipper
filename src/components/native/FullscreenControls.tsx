import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import NativeTransport from "./NativeTransport";
import type { Control, Snapshot } from "./nativePlayback";
import styles from "../Player.module.scss";

interface Presentation {
	snapshot: Snapshot;
	visible: boolean;
}

// Only the transport lives here. The main window retains the video, worker and idle timer.
export default function FullscreenControls() {
	const [state, setState] = useState<Presentation | null>(null);
	const [error, setError] = useState("");
	const transport = useRef<HTMLDivElement>(null);
	const snapshot = state?.snapshot ?? null;
	const visible = state?.visible ?? false;
	const id = snapshot?.id;

	useEffect(() => {
		let disposed = false;
		const removers: (() => void)[] = [];
		async function connect() {
			const unlisten = await listen<Presentation>("native-controls-state", (event) => {
				if (!disposed) setState(event.payload);
			});
			removers.push(unlisten);
			const unfocus = await listen("native-controls-focus", () => {
				if (disposed) return;
				setState((current) => current && { ...current, visible: true });
				requestAnimationFrame(() => transport.current?.querySelector("button")?.focus());
			});
			removers.push(unfocus);
			if (disposed) removers.forEach((remove) => remove());
			else await invoke("mpv_overlay", { action: { kind: "ready" } });
		}
		void connect().catch((reason) => {
			if (!disposed) setError(String(reason));
		});
		return () => {
			disposed = true;
			removers.forEach((remove) => remove());
		};
	}, []);

	useEffect(() => {
		if (visible || id === undefined) return;
		// Keep the owned window present until its CSS fade completes. Reveal has no transition.
		const delay = matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 180;
		const timer = setTimeout(() => {
			void invoke("mpv_overlay", { action: { kind: "hidden", id } }).catch(() => {});
		}, delay);
		return () => clearTimeout(timer);
	}, [visible, id]);

	async function command(control: Control | { kind: "fullscreen"; value: boolean }) {
		if (id === undefined) return;
		try {
			await invoke("mpv_control", { id, control });
			setError("");
		} catch (reason) {
			setError(String(reason));
		}
	}

	function activity(keyboard: boolean) {
		if (id !== undefined)
			void invoke("mpv_overlay", { action: { kind: "activity", id, keyboard } }).catch(
				() => {},
			);
	}

	return (
		<div
			className={styles.controlsOverlay}
			data-visible={visible}
			aria-hidden={!visible}
			inert={!visible}
			onPointerMove={() => activity(false)}
			onKeyDown={(event) => {
				activity(true);
				if (event.key === "Escape") {
					event.preventDefault();
					void command({ kind: "fullscreen", value: false });
				}
			}}
		>
			<NativeTransport
				transportRef={transport}
				snapshot={snapshot}
				ready={!!snapshot?.duration && !snapshot.error}
				command={(control) => void command(control)}
				requestFullscreen={() => void command({ kind: "fullscreen", value: false })}
			/>
			{error && <div role="alert">{error}</div>}
		</div>
	);
}
