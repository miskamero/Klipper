// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import MedalDetails from "./MedalDetails";
import Inspector from "./Inspector";
import type { ImportedMetadata } from "../services/medal";
const entry: ImportedMetadata = {
	provider: "medal",
	externalId: "synthetic-id",
	relation: "primary",
	importedMs: 1_700_000_000_000,
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
		createdMs: 1_700_000_000_000,
		contentCreatedMs: 1_700_000_100_000,
		lastEditMs: null,
		duration: 8,
		tags: ["Synthetic tag"],
		filePath: "/synthetic/clip.mp4",
		image: null,
		originalFilePath: null,
		originalImage: null,
		extra: { trim: { trimStartTimeInSeconds: 2, trimDurationInSeconds: 5 } },
	},
};
afterEach(cleanup);
it("shows provider provenance and timestamp meanings without raw JSON or absent fields", () => {
	render(<MedalDetails entries={[entry]} />);
	expect(screen.getByText("Medal created")).toBeTruthy();
	expect(screen.getByText("Content created")).toBeTruthy();
	expect(screen.queryByText("Last edit")).toBeNull();
	expect(screen.getByText("Trim Start Time In Seconds")).toBeTruthy();
	expect(screen.getByText("Medal title fields")).toBeTruthy();
});
it("uses an imported primary title while retaining the actual source filename", () => {
	render(
		<Inspector
			recording={{ id: 1, name: "clip.mp4", path: "/synthetic/clip.mp4" }}
			info={null}
			metadata={[entry]}
			importedTitle={entry.data.title}
			loading={false}
			error=""
			onRetry={() => {}}
		/>,
	);
	expect(screen.getByRole("heading", { name: "Custom synthetic title" })).toBeTruthy();
	expect(screen.getByText("clip.mp4")).toBeTruthy();
});
it("labels original references as related edits instead of claiming their title belongs to this file", () => {
	render(<MedalDetails entries={[{ ...entry, relation: "original" }]} />);
	expect(screen.getByText(/Related Medal edit \(this file is its original\)/)).toBeTruthy();
});

it("shows SQLite provenance, retained selections and sanitized extra metadata", () => {
	render(
		<MedalDetails
			entries={[
				{
					...entry,
					selectedValues: { title: "Retained title" },
					data: {
						...entry.data,
						sourceFormat: "sqlite",
						title: "SQLite title",
						remoteTrimStart: 2,
						extra: { bookmarks: [{ time: 3, name: "Synthetic moment" }] },
					},
				},
			]}
		/>,
	);
	expect(screen.getByText(/Imported clip information.*SQLite/)).toBeTruthy();
	expect(screen.getByText("Local Medal created")).toBeTruthy();
	expect(screen.getByText("Selected library values from this source")).toBeTruthy();
	expect(screen.getByText("Retained title")).toBeTruthy();
	expect(screen.getByText("SQLite title")).toBeTruthy();
	expect(screen.getByText("Synthetic moment")).toBeTruthy();
	expect(screen.getByText("Remote trim start")).toBeTruthy();
});

it("labels retained provider dates instead of presenting an invalid new timestamp as current", () => {
	render(
		<MedalDetails
			entries={[
				{
					...entry,
					data: {
						...entry.data,
						sourceFormat: "sqlite",
						timestampWarnings: ["Local creation time unavailable."],
						retainedTimestamps: ["createdMs", "contentCreatedMs"],
					},
				},
			]}
		/>,
	);
	expect(screen.getByText("Unavailable source timestamps")).toBeTruthy();
	expect(screen.getByText("Local creation time unavailable.")).toBeTruthy();
	expect(
		screen.getByText(
			/Using previously imported valid timestamps: local creation, remote content creation/,
		),
	).toBeTruthy();
});
