// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import Inspector from "./Inspector";
import type { ImportedMetadata } from "../services/medal";
import type { MediaInfo } from "../services/media";
import { localizedDate } from "../lib/inspector";
import { CopyValue } from "./InspectorValues";

const entry: ImportedMetadata = {
	provider: "medal",
	externalId: "synthetic-id",
	relation: "primary",
	importedMs: 1_700_000_000_000,
	sourcePath: "/synthetic/clips.json",
	selectedValues: { title: "Custom synthetic title", game: "Synthetic game" },
	data: {
		title: "Custom synthetic title",
		titleField: "Content.contentTitle",
		localTitle: "Local synthetic title",
		contentTitle: "Custom synthetic title",
		titleDifference: true,
		description: "Synthetic description",
		game: "Synthetic game",
		gameId: "synthetic-game",
		gameConflict: false,
		gameLookupAmbiguous: false,
		createdMs: 1_690_000_000_000,
		contentCreatedMs: 1_690_000_100_000,
		lastEditMs: null,
		duration: 8,
		tags: ["Synthetic tag"],
		filePath: "/synthetic/clip.mp4",
		image: "/synthetic/preview.jpg",
		originalFilePath: null,
		originalImage: null,
		extra: {
			trim: { trimStartTimeInSeconds: 2.123456789, trimDurationInSeconds: 5 },
			bookmarks: [{ time: 3, name: "Synthetic moment" }],
		},
	},
};
const recording = { id: 1, name: "clip.mp4", path: "/synthetic/clip.mp4" };
const info: MediaInfo = {
	container: "mov,mp4,m4a,3gp,3g2,mj2",
	videoCodec: "hevc",
	width: 1920,
	height: 1080,
	frameRate: 59.94005994,
	duration: 65,
	fileSize: 1048576,
	bitrate: 3500123,
	audio: Array.from({ length: 4 }, (_, index) => ({
		index,
		codec: "aac",
		channels: 2,
		channelLayout: "stereo",
		tags: { language: "eng", title: `Track ${index + 1}`, encoder: "Synthetic encoder" },
	})),
};
afterEach(() => {
	cleanup();
	Reflect.deleteProperty(navigator, "clipboard");
	vi.restoreAllMocks();
});
function inspect(entries: ImportedMetadata[] = [entry], media: MediaInfo | null = info) {
	return render(
		<Inspector
			recording={recording}
			info={media}
			metadata={entries}
			importedTitle={
				entries.some((e) => e.relation === "primary") ? "Custom synthetic title" : null
			}
			loading={false}
			error=""
			onRetry={() => {}}
		/>,
	);
}
function disclose(label: string, root: HTMLElement = document.body) {
	const summary = within(root).getByText(label, { selector: "summary" });
	fireEvent.click(summary);
	return summary.closest("details")!;
}

it("prioritizes a single title and useful summary with three independently collapsed sections", () => {
	inspect();
	const aside = screen.getByLabelText("Recording details");
	const header = aside.querySelector("header")!;
	expect(within(header).getByRole("heading", { name: "Custom synthetic title" })).toBeTruthy();
	expect(header.textContent).toContain("Synthetic game");
	expect(header.textContent).toContain("01:05");
	expect(header.textContent).toContain("1920 × 1080 · HEVC");
	expect(header.textContent).toContain("Metadata imported");
	expect(header.textContent).toContain(localizedDate(entry.importedMs));
	expect(header.textContent).not.toContain("Content.contentTitle");
	expect(header.textContent).not.toContain("synthetic-id");
	expect(header.textContent).not.toContain("2023-");
	const sections = Array.from(aside.querySelectorAll(":scope > details"));
	expect(sections.map((s) => s.querySelector("summary")!.textContent)).toEqual([
		"Technical details",
		"Medal metadata",
		"Advanced & source data",
	]);
	expect(sections.every((s) => !s.hasAttribute("open"))).toBe(true);
	const technical = disclose("Technical details");
	expect(technical.open).toBe(true);
	expect(sections[1].hasAttribute("open")).toBe(false);
	const medal = disclose("Medal metadata");
	expect(medal.open).toBe(true);
	expect(technical.open).toBe(true);
	expect(sections[2].hasAttribute("open")).toBe(false);
});

it("keeps plain recordings useful without inventing a game, provider dates or a Medal section", () => {
	inspect([]);
	expect(screen.getByRole("heading", { name: "clip" })).toBeTruthy();
	expect(screen.queryByText("Medal metadata", { selector: "summary" })).toBeNull();
	const header = screen.getByLabelText("Recording details").querySelector("header")!;
	expect(header.textContent).toContain("Local file");
	expect(header.textContent).not.toContain("Game");
	expect(header.textContent).not.toContain("imported");
	const technical = disclose("Technical details");
	expect(within(technical).getByText("1.0 MiB")).toBeTruthy();
	expect(within(technical).getByText("clip.mp4")).toBeTruthy();
});

it("shows all four audio streams above the closed sections while keeping track details expandable", () => {
	inspect();
	const audio = screen.getByRole("region", { name: "Audio streams" });
	const technical = screen
		.getByText("Technical details", { selector: "summary" })
		.closest("details")!;
	expect(audio.closest("details")).toBeNull();
	expect(technical.open).toBe(false);
	expect(
		audio.compareDocumentPosition(technical) & Node.DOCUMENT_POSITION_FOLLOWING,
	).toBeTruthy();
	expect(within(audio).getAllByText("AAC")).toHaveLength(4);
	expect(within(audio).getByText(/Independent mixing is not available/)).toBeTruthy();
	const track = audio.querySelectorAll("details")[3];
	expect(track.open).toBe(false);
	fireEvent.click(track.querySelector("summary")!);
	expect(track.open).toBe(true);
	expect(within(track as HTMLElement).getByText("Track 4")).toBeTruthy();
	const advanced = disclose("Advanced & source data");
	disclose("Audio stream metadata", advanced);
	expect(within(advanced).getAllByText("Synthetic encoder")).toHaveLength(4);
});

it("shows the no-audio state without opening technical details", () => {
	inspect([], { ...info, audio: [] });
	const audio = screen.getByRole("region", { name: "Audio streams" });
	expect(within(audio).getByText("No audio streams detected.")).toBeTruthy();
	expect(audio.closest("details")).toBeNull();
});

it("shows descriptive Medal fields and localized dates without IDs, field paths or duplicated ISO dates", () => {
	inspect();
	const medal = disclose("Medal metadata");
	expect(within(medal).getByText("Synthetic description")).toBeTruthy();
	expect(within(medal).getByText("Synthetic tag")).toBeTruthy();
	expect(medal.textContent).toContain("Local Medal creation");
	expect(medal.textContent).toContain(localizedDate(entry.data.createdMs));
	expect(medal.textContent).not.toContain(new Date(entry.data.createdMs!).toISOString());
	expect(medal.textContent).not.toContain("Content.contentTitle");
	expect(medal.textContent).not.toContain("synthetic-game");
	expect(within(medal).queryByText("Last edit")).toBeNull();
	expect(within(medal).getByText("Synthetic moment")).toBeTruthy();
});

it("preserves exact raw precision and source differences while honoring selected game values", () => {
	const sql: ImportedMetadata = {
		...entry,
		sourcePath: "/synthetic/medal.db",
		selectedValues: {},
		data: {
			...entry.data,
			sourceFormat: "sqlite",
			title: "SQLite title",
			game: "Different game",
			description: "SQLite description",
			remoteTrimStart: 2.987654321,
		},
	};
	const before = JSON.stringify([entry, sql]);
	inspect([entry, sql]);
	const header = screen.getByLabelText("Recording details").querySelector("header")!;
	expect(header.textContent).toContain("Synthetic game");
	expect(header.textContent).not.toContain("Different game");
	const medal = disclose("Medal metadata");
	expect(within(medal).getByText(/Some provider fields differ/)).toBeTruthy();
	expect(within(medal).queryByText("Synthetic description")).toBeNull();
	const advanced = disclose("Advanced & source data");
	const json = disclose("JSON source 1", advanced);
	const sqlite = disclose("SQLite source 2", advanced);
	disclose("Imported source claims", json);
	disclose("Imported source claims", sqlite);
	disclose("Selected library values", json);
	expect(within(sqlite).getByText("SQLite title")).toBeTruthy();
	expect(within(sqlite).getByText("Different game")).toBeTruthy();
	expect(within(json).getByText("Content.contentTitle")).toBeTruthy();
	disclose("Additional source fields", json);
	expect(within(json).getByText("2.123456789")).toBeTruthy();
	expect(JSON.stringify([entry, sql])).toBe(before);
	disclose("Exact timestamps", json);
	expect(within(json).getByText(new Date(entry.data.createdMs!).toISOString())).toBeTruthy();
});

it("does not select an arbitrary game when source claims disagree without a selected value", () => {
	inspect([
		{ ...entry, selectedValues: {} },
		{ ...entry, selectedValues: {}, data: { ...entry.data, game: "Other game" } },
	]);
	expect(
		screen.getByLabelText("Recording details").querySelector("header")!.textContent,
	).not.toContain("Game");
});

it("does not use related edit metadata as the original recording's title or game", () => {
	inspect([{ ...entry, relation: "original" }]);
	const header = screen.getByLabelText("Recording details").querySelector("header")!;
	expect(header.textContent).toContain("clip");
	expect(header.textContent).not.toContain("Synthetic game");
	const medal = disclose("Medal metadata");
	expect(
		within(medal).getByText(/explicitly links this recording to a related edit/),
	).toBeTruthy();
});

it("keeps real analysis failures and retry accessible outside every collapsed section", () => {
	const retry = vi.fn();
	render(
		<Inspector
			recording={recording}
			info={null}
			loading={false}
			error="Recording is unavailable"
			onRetry={retry}
		/>,
	);
	expect(screen.getByRole("alert").closest("details")).toBeNull();
	fireEvent.click(screen.getByRole("button", { name: "Retry analysis" }));
	expect(retry).toHaveBeenCalledOnce();
});

it("handles long titles and paths, sparse metadata and unavailable media without fabricated fields", () => {
	const title = "A very long recording title ".repeat(20);
	render(
		<Inspector
			recording={{
				...recording,
				path: "/synthetic/" + "long-folder/".repeat(30) + "clip.mp4",
			}}
			info={null}
			loading={true}
			error=""
			onRetry={() => {}}
			metadata={[]}
			importedTitle={title}
		/>,
	);
	expect(screen.getByRole("heading").textContent).toBe(title);
	expect(screen.getByRole("status").closest("details")).toBeNull();
	expect(screen.queryByText("Duration")).toBeNull();
	expect(screen.queryByText("Video codec")).toBeNull();
});

it("labels retained timestamps and nonfatal source warnings inside advanced details", () => {
	inspect([
		{
			...entry,
			data: {
				...entry.data,
				timestampWarnings: ["Local creation time unavailable."],
				retainedTimestamps: ["createdMs"],
			},
		},
	]);
	const advanced = disclose("Advanced & source data");
	const source = disclose("JSON source", advanced);
	expect(within(source).getByText("Local creation time unavailable.")).toBeTruthy();
	expect(source.textContent).toContain(
		"Previously imported valid timestamps retained: createdMs",
	);
});

it("copies an exact path and reports clipboard errors without requiring native permissions", async () => {
	const writeText = vi.fn().mockResolvedValue(undefined);
	Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
	inspect();
	disclose("Advanced & source data");
	fireEvent.click(screen.getByRole("button", { name: "Copy file path" }));
	await screen.findByText("Copied");
	expect(writeText).toHaveBeenCalledWith(recording.path);
	writeText.mockRejectedValueOnce(new Error("denied"));
	fireEvent.click(screen.getByRole("button", { name: "Copy file path" }));
	await waitFor(() => expect(screen.getByText(/Could not copy/)).toBeTruthy());
});

it("keeps very long paths compact while providing an accessible full-value disclosure", () => {
	const path = "/synthetic/" + "nested-recordings/".repeat(20) + "clip.mp4";
	render(<CopyValue value={path} label="file path" />);
	const full = screen.getByText("Show full file path").closest("details")!;
	expect(full.open).toBe(false);
	fireEvent.click(screen.getByText("Show full file path"));
	expect(full.open).toBe(true);
	expect(within(full).getByText(path)).toBeTruthy();
});

it("omits absent and invalid dates and supports normalized metadata without a provider-specific inspector", () => {
	const sparse: ImportedMetadata = {
		...entry,
		provider: "synthetic-provider",
		importedMs: 0,
		selectedValues: {},
		data: {
			...entry.data,
			sourceFormat: "synthetic-format",
			game: null,
			gameId: null,
			createdMs: null,
			contentCreatedMs: 9e20,
			description: null,
			tags: [],
			duration: null,
			extra: {},
		},
	};
	inspect([sparse], null);
	const header = screen.getByLabelText("Recording details").querySelector("header")!;
	expect(header.textContent).not.toContain("Metadata imported");
	expect(header.textContent).not.toContain("Game");
	expect(header.textContent).not.toContain("duration");
	expect(screen.queryByText("Medal metadata", { selector: "summary" })).toBeNull();
	const advanced = disclose("Advanced & source data");
	expect(within(advanced).getByText("synthetic-provider · synthetic-format source")).toBeTruthy();
});
