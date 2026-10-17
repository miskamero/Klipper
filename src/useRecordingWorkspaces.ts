import { useEffect, useRef, useState } from "react";
import { openLibraryRecording } from "./services/library";
import {
	activateRecording,
	analyzeRecording,
	deactivateRecording,
	errorMessage,
	selectRecording,
	type Recording,
	type MediaInfo,
} from "./services/media";
import type { PlaybackPosition } from "./services/playback";
import type { ImportedMetadata } from "./services/medal";

type View = "library" | "libraryPlayback" | "player";
type Context = "library" | "standalone";
interface Selection {
	recording: Recording;
	info: MediaInfo | null;
	analyzing: boolean;
	error: string;
	resume?: PlaybackPosition;
	metadata?: ImportedMetadata[];
	displayTitle?: string | null;
}

// Two session selections, one mounted player. All ownership transfers await native stop/hide.
export default function useRecordingWorkspaces() {
	const [view, setView] = useState<View>("library");
	const [selections, setSelections] = useState<Record<Context, Selection | null>>({
		library: null,
		standalone: null,
	});
	const [ready, setReady] = useState(true);
	const [selecting, setSelecting] = useState(false);
	const [error, setError] = useState("");
	const mounted = useRef(false);
	const picking = useRef(false);
	const sequence = useRef(0);
	const libraryEpoch = useRef(0);
	const owner = useRef<Recording | null>(null);
	const transitions = useRef<Promise<void>>(Promise.resolve());
	const positions = useRef<Partial<Record<Context, PlaybackPosition>>>({});

	useEffect(() => {
		mounted.current = true;
		return () => {
			mounted.current = false;
			// This request counter deliberately invalidates work at cleanup; it is not a DOM ref.
			// eslint-disable-next-line react-hooks/exhaustive-deps
			sequence.current++;
			void transitions.current.finally(async () => {
				if (owner.current) await deactivateRecording(owner.current.id).catch(() => {});
			});
		};
	}, []);

	const valid = (token: number) => mounted.current && token === sequence.current;
	function save(context: Context, selection: Selection) {
		setSelections((previous) => ({ ...previous, [context]: selection }));
	}
	function transfer(token: number, run: () => Promise<void>) {
		transitions.current = transitions.current
			.catch(() => {})
			.then(async () => {
				// Releasing the owner is necessary even if this particular navigation was superseded.
				if (owner.current) {
					await deactivateRecording(owner.current.id);
					owner.current = null;
				}
				if (valid(token)) await run();
			})
			.catch((failure) => {
				if (valid(token)) {
					setError(errorMessage(failure));
					setReady(false);
				}
			});
	}
	async function inspect(context: Context, selection: Selection, token: number) {
		if (!valid(token)) return;
		save(context, { ...selection, info: null, analyzing: true, error: "" });
		try {
			const info = await analyzeRecording(selection.recording.id);
			if (valid(token)) save(context, { ...selection, info, analyzing: false, error: "" });
		} catch (failure) {
			if (valid(token))
				save(context, { ...selection, analyzing: false, error: errorMessage(failure) });
		}
	}
	function navigate(next: "library" | "player") {
		if (picking.current || (view === next && ready)) return;
		const token = ++sequence.current;
		setError("");
		setReady(false);
		setView(next);
		transfer(token, async () => {
			const previous = selections.standalone;
			if (next === "player" && previous) {
				const recording = await activateRecording(previous.recording.id);
				owner.current = recording;
				if (!mounted.current) return;
				const selection = {
					...previous,
					recording,
					resume: positions.current.standalone ?? {
						position: 0,
						speed: 1,
						volume: 100,
						muted: false,
					},
					analyzing: false,
				};
				save("standalone", selection);
				// Authorization IDs rotate even when a navigation is superseded mid-activation.
				if (!valid(token)) return;
				setReady(true);
				// The one-off file may have changed while this workspace was inactive.
				void inspect("standalone", selection, token);
			} else setReady(true);
		});
	}
	async function open(fileId?: number) {
		if (!mounted.current || picking.current) return;
		picking.current = true;
		setSelecting(true);
		setError("");
		let recording: Recording | null;
		const epoch = libraryEpoch.current;
		let info: MediaInfo | null = null;
		let metadata: ImportedMetadata[] | undefined;
		let displayTitle: string | null | undefined;
		try {
			if (fileId === undefined) recording = await selectRecording();
			else {
				const result = await openLibraryRecording(fileId);
				recording = result.recording;
				info = result.info;
				metadata = result.metadata;
				displayTitle = result.display_title;
			}
		} catch (failure) {
			if (mounted.current && (fileId === undefined || epoch === libraryEpoch.current))
				setError(errorMessage(failure));
			return;
		} finally {
			picking.current = false;
			if (mounted.current) setSelecting(false);
		}
		if (!recording) return;
		if (fileId !== undefined && epoch !== libraryEpoch.current) return;
		if (!mounted.current) {
			void deactivateRecording(recording.id).catch(() => {});
			return;
		}
		const selected = recording;
		const context: Context = fileId === undefined ? "standalone" : "library";
		const token = ++sequence.current;
		positions.current[context] = undefined;
		setView(context === "standalone" ? "player" : "libraryPlayback");
		setReady(false);
		// Selection already authorized this ID. Remember it even if a later navigation supersedes
		// the transfer, so its authorization is released before another context is activated.
		const previousOwner = owner.current;
		transitions.current = transitions.current
			.catch(() => {})
			.then(async () => {
				if (previousOwner) await deactivateRecording(previousOwner.id);
				owner.current = selected;
				if (!mounted.current) return;
				const selection = {
					recording: selected,
					info,
					analyzing: false,
					error: "",
					metadata,
					displayTitle,
				};
				save(context, selection);
				if (!valid(token)) return;
				setReady(true);
				if (!info) void inspect(context, selection, token);
			})
			.catch((failure) => {
				if (valid(token)) setError(errorMessage(failure));
			});
	}
	const context: Context = view === "player" ? "standalone" : "library";
	const current = ready && view !== "library" ? selections[context] : null;
	return {
		forgetLibrary: () => {
			libraryEpoch.current++;
			setSelections((previous) => ({ ...previous, library: null }));
			positions.current.library = undefined;
			if (view !== "player") {
				// Native reset already stopped and forgot the library owner.
				sequence.current++;
				owner.current = null;
				setView("library");
				setReady(true);
				setError("");
			}
		},
		view,
		navigate,
		open,
		selecting,
		ready,
		error,
		current,
		hasStandalone: !!selections.standalone,
		retry: () => {
			if (current) void inspect(context, current, sequence.current);
		},
		playbackChanged: (position: PlaybackPosition) => {
			positions.current[context] = position;
		},
	};
}
