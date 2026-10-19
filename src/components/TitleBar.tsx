import { useCallback, useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import styles from "./TitleBar.module.scss";

export default function TitleBar({
	selecting,
	hasRecording,
	inspectorOpen,
	onOpen,
	onToggleInspector,
	workspace = "player",
	onWorkspaceChange,
	showInspector = workspace === "player",
	exportSummary,
	onShowExports,
}: {
	selecting: boolean;
	hasRecording: boolean;
	inspectorOpen: boolean;
	onOpen: () => void;
	onToggleInspector: () => void;
	workspace?: "library" | "player";
	onWorkspaceChange?: (workspace: "library" | "player") => void;
	showInspector?: boolean;
	exportSummary?: string;
	onShowExports?: () => void;
}) {
	const [nativeWindow] = useState(() => (isTauri() ? getCurrentWindow() : null));
	const [maximized, setMaximized] = useState(false);
	const [error, setError] = useState("");
	const stateRequest = useRef(0);
	const mounted = useRef(true);

	const refreshState = useCallback(async () => {
		if (!nativeWindow) return;
		const request = ++stateRequest.current;
		try {
			const value = await nativeWindow.isMaximized();
			if (mounted.current && request === stateRequest.current) setMaximized(value);
		} catch {
			if (mounted.current && request === stateRequest.current)
				setError("Window state could not be read. Window controls are still available.");
		}
	}, [nativeWindow]);

	useEffect(() => {
		if (!nativeWindow) return;
		mounted.current = true;
		let disposed = false;
		let unlisten: (() => void) | undefined;
		let timer: ReturnType<typeof setTimeout> | undefined;
		void refreshState();
		void nativeWindow
			.onResized(() => {
				// Windows emits intermediate resize events during maximize/restore.
				++stateRequest.current;
				clearTimeout(timer);
				timer = setTimeout(() => void refreshState(), 80);
			})
			.then((stop) => {
				if (disposed) stop();
				else unlisten = stop;
			})
			.catch(() => {
				if (!disposed) setError("Window state updates are unavailable.");
			});
		return () => {
			disposed = true;
			mounted.current = false;
			// This numeric counter intentionally invalidates pending queries during cleanup.
			// eslint-disable-next-line react-hooks/exhaustive-deps
			++stateRequest.current;
			clearTimeout(timer);
			unlisten?.();
		};
	}, [nativeWindow, refreshState]);

	async function windowAction(action: "minimize" | "toggleMaximize" | "close" | "startDragging") {
		if (!nativeWindow) return;
		setError("");
		try {
			await nativeWindow[action]();
			if (action === "toggleMaximize") await refreshState();
		} catch {
			setError(
				"The window action failed. Try again; Alt+F4 can also close the desktop window.",
			);
		}
	}

	return (
		<>
			<header className={styles.titlebar}>
				<div
					className={styles.dragRegion}
					data-window-drag-region={nativeWindow ? true : undefined}
					onMouseDown={(event) => {
						if (event.button === 0 && nativeWindow) {
							event.preventDefault();
							void windowAction(
								event.detail === 2 ? "toggleMaximize" : "startDragging",
							);
						}
					}}
				>
					<svg
						className={styles.logo}
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						strokeWidth="2"
						aria-hidden="true"
					>
						<rect x="3" y="3" width="18" height="18" rx="4" />
						<path d="M8 7v10m8-10-6 5 6 5" />
					</svg>
					<span className={styles.brand}>Klipper</span>
					{!onWorkspaceChange && <span className={styles.workspaceName}>Player</span>}
				</div>
				{onWorkspaceChange && (
					<nav className={styles.navigation} aria-label="Workspace">
						<button
							aria-current={workspace === "library" ? "page" : undefined}
							disabled={selecting}
							onClick={() => onWorkspaceChange("library")}
						>
							Library
						</button>
						<button
							aria-current={workspace === "player" ? "page" : undefined}
							disabled={selecting}
							onClick={() => onWorkspaceChange("player")}
						>
							Player
						</button>
					</nav>
				)}
				<div className={styles.actions}>
					{exportSummary && (
						<button
							onClick={onShowExports}
							aria-label="Show exports"
							title="View background exports in Library"
						>
							Exports · {exportSummary}
						</button>
					)}
					<button
						onClick={onOpen}
						disabled={selecting}
						aria-label={hasRecording ? "Open another recording" : "Open recording"}
						title="Open a local recording in the standalone Player"
					>
						<svg viewBox="0 0 18 18" aria-hidden="true">
							<path d="M2 5h5l2 2h7v8H2zM2 5V3h5l2 2h6v2" />
						</svg>
						<span>{selecting ? "Choosing..." : "Open recording"}</span>
					</button>
					{showInspector && (
						<button
							aria-label={inspectorOpen ? "Hide details" : "Show details"}
							title={
								inspectorOpen ? "Hide recording details" : "Show recording details"
							}
							aria-expanded={inspectorOpen}
							aria-controls="recording-inspector"
							onClick={onToggleInspector}
						>
							<svg viewBox="0 0 18 18" aria-hidden="true">
								<rect x="2" y="3" width="14" height="12" rx="1" />
								<path d="M11 3v12" />
							</svg>
						</button>
					)}
				</div>
				{nativeWindow && (
					<div
						className={styles.windowControls}
						aria-label="Window controls"
						role="group"
					>
						<button
							aria-label="Minimize window"
							title="Minimize"
							onClick={() => void windowAction("minimize")}
						>
							<svg viewBox="0 0 18 18" aria-hidden="true">
								<path d="M4 9h10" />
							</svg>
						</button>
						<button
							aria-label={maximized ? "Restore window" : "Maximize window"}
							title={maximized ? "Restore" : "Maximize"}
							onClick={() => void windowAction("toggleMaximize")}
						>
							<svg viewBox="0 0 18 18" aria-hidden="true">
								{maximized ? (
									<>
										<path d="M6 5V3h9v9h-2" />
										<rect x="3" y="6" width="9" height="9" />
									</>
								) : (
									<rect x="4" y="4" width="10" height="10" />
								)}
							</svg>
						</button>
						<button
							className={styles.close}
							aria-label="Close window"
							title="Close"
							onClick={() => void windowAction("close")}
						>
							<svg viewBox="0 0 18 18" aria-hidden="true">
								<path d="m4 4 10 10M14 4 4 14" />
							</svg>
						</button>
					</div>
				)}
			</header>
			{error && (
				<p className={styles.error} role="alert">
					{error}
				</p>
			)}
		</>
	);
}
