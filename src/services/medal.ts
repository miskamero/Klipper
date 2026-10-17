import { invoke } from "@tauri-apps/api/core";

export interface MedalMetadata {
	sourceFormat?: string;
	localId?: string | null;
	remoteId?: string | null;
	contentHasTitle?: boolean | null;
	remoteTrimStart?: number | null;
	gameField?: string | null;
	timestampWarnings?: string[];
	retainedTimestamps?: string[];
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
	selectedValues?: Record<string, unknown>;
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
	identityConflict?: boolean;
	timestampWarnings?: string[];
	differences?: { field: string; existing: unknown; incoming: unknown; existingFormat: string }[];
}
export interface ImportPreview {
	sourceFormat?: string;
	sourcePath?: string;
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
		recovered?: number;
		identityConflicts?: number;
		metadataDifferences?: number;
		timestampWarnings?: number;
		invalidIdentities?: number;
	};
	rows: ImportRow[];
}
export interface ImportProgress {
	phase: string;
	done: number;
	total: number;
	linked: number;
	error: string;
	warning?: string;
	committed?: boolean;
}
export const selectMedalCatalog = () => invoke<ImportPreview | null>("medal_select");
export const medalPreviewPage = (token: number, offset: number) =>
	invoke<ImportPreview>("medal_preview", { token, offset });
export const commitMedalImport = (token: number, includeNew: boolean) =>
	invoke<ImportProgress>("medal_commit", { confirmation: { token, additions: [], includeNew } });
export const medalImportProgress = () => invoke<ImportProgress>("medal_status");
export const cancelMedalImport = () => invoke<void>("medal_cancel");
