import { invoke, isTauri } from "@tauri-apps/api/core";
import type { MediaInfo, Recording } from "./media";

export interface LibraryRoot {
	id: number;
	path: string;
	scanState: string;
	error: string | null;
	lastScan: number | null;
	recordings: number;
}

export interface ScanProgress {
	rootId: number | null;
	phase: string;
	discovered: number;
	analyzed: number;
	reused: number;
	skippedLinks: number;
	issueCount: number;
	issues: string[];
}

export interface LibraryStatus {
	roots: LibraryRoot[];
	progress: ScanProgress;
}

export interface LibraryRecording {
	id: number;
	name: string;
	path: string;
	size: number;
	modifiedMs: number | null;
	revision?: string | null;
	status: string;
	availability: string;
	duration: number | null;
	videoCodec: string | null;
	width: number | null;
	height: number | null;
	frameRate: number | null;
	audioCount: number | null;
	error: string | null;
}

export interface LibraryQuery {
	search: string;
	rootId: number | null;
	sort: string;
	descending: boolean;
	filter: string;
	codec: string;
	offset: number;
}

export interface LibraryPage {
	items: LibraryRecording[];
	total: number;
	limit: number;
}

export function scanActive(phase: string): boolean {
	return phase === "discovering" || phase === "analyzing";
}

export async function libraryStatus(): Promise<LibraryStatus> {
	if (!isTauri())
		return {
			roots: [],
			progress: {
				rootId: null,
				phase: "",
				discovered: 0,
				analyzed: 0,
				reused: 0,
				skippedLinks: 0,
				issueCount: 0,
				issues: [],
			},
		};
	return invoke("library_status");
}

export async function queryLibrary(query: LibraryQuery): Promise<LibraryPage> {
	if (!isTauri()) return { items: [], total: 0, limit: 40 };
	return invoke("library_query", { query });
}

export function addLibraryFolder(): Promise<LibraryRoot | null> {
	return invoke("library_add_folder");
}

export function rescanLibraryFolder(rootId: number): Promise<void> {
	return invoke("library_rescan", { rootId });
}

export function removeLibraryFolder(rootId: number): Promise<void> {
	return invoke("library_remove_folder", { rootId });
}

export function cancelLibraryScan(): Promise<void> {
	return invoke("library_cancel");
}

export function openLibraryRecording(fileId: number): Promise<{
	recording: Recording;
	info: MediaInfo | null;
}> {
	return invoke("library_open", { fileId });
}
