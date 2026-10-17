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
	navigation?: LibraryNavigation;
}

export interface LibraryFolder {
	id: string;
	parentId: string | null;
	rootId: number;
	name: string;
	path: string;
	recordings: number;
}
export interface LibraryNavigation {
	total: number;
	sources: { kind: string; recordings: number }[];
	folders: LibraryFolder[];
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
	title?: string | null;
	game?: string | null;
	medalCreatedMs?: number | null;
	metadataConflict?: boolean;
	previewRevision?: string | null;
}

export interface LibraryQuery {
	home?: boolean;
	unknownGame?: boolean;
	search: string;
	rootId: number | null;
	folderId?: string | null;
	provider?: string | null;
	sort: string;
	descending: boolean;
	filter: string;
	codec: string;
	offset: number;
	game?: string | null;
}

export interface LibraryPage {
	items: LibraryRecording[];
	total: number;
	limit: number;
	games?: string[];
	unknownGameAvailable?: boolean;
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
	const nativeQuery = { ...query };
	delete nativeQuery.home;
	return invoke("library_query", { query: nativeQuery });
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

export function resetLibrary(): Promise<{ cacheWarning: string | null }> {
	return invoke("library_reset", { confirmed: true });
}

export function openLibraryRecording(fileId: number): Promise<{
	recording: Recording;
	info: MediaInfo | null;
	metadata?: import("./medal").ImportedMetadata[];
	display_title?: string | null;
}> {
	return invoke("library_open", { fileId });
}
