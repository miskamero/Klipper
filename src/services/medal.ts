import { invoke } from "@tauri-apps/api/core";

export interface MedalMetadata {
	title: string | null;
	titleField: string | null;
	localTitle: string | null;
	contentTitle: string | null;
	titleDifference: boolean;
	description: string | null;
	game: string | null;
	gameId: string | null;
	gameConflict: boolean;
	gameLookupAmbiguous: boolean;
	createdMs: number | null;
	contentCreatedMs: number | null;
	lastEditMs: number | null;
	duration: number | null;
	tags: string[];
	filePath: string | null;
	image: string | null;
	originalFilePath: string | null;
	originalImage: string | null;
	extra: Record<string, unknown>;
}
export interface ImportedMetadata {
	provider: string;
	externalId: string;
	relation: "primary" | "original";
	importedMs: number;
	sourcePath?: string | null;
	data: MedalMetadata;
}
export interface ImportRow {
	index: number;
	clipId: string;
	relation: string;
	title: string | null;
	game: string | null;
	path: string | null;
	status: string;
	conflict: boolean;
	warning: boolean;
}
export interface ImportPreview {
	token: number;
	catalogEntries: number;
	total: number;
	offset: number;
	counts: {
		matched: number;
		unchanged: number;
		updated: number;
		newFiles: number;
		missing: number;
		ambiguous: number;
		unmatched: number;
		conflicts: number;
		warnings: number;
		skipped: number;
	};
	rows: ImportRow[];
}
export interface ImportProgress {
	phase: string;
	done: number;
	total: number;
	linked: number;
	error: string;
}
export const selectMedalCatalog = () => invoke<ImportPreview | null>("medal_select");
export const medalPreviewPage = (token: number, offset: number) =>
	invoke<ImportPreview>("medal_preview", { token, offset });
export const commitMedalImport = (token: number, includeNew: boolean) =>
	invoke<ImportProgress>("medal_commit", { confirmation: { token, additions: [], includeNew } });
export const medalImportProgress = () => invoke<ImportProgress>("medal_status");
export const cancelMedalImport = () => invoke<void>("medal_cancel");
