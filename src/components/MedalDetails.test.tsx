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
	expect(screen.getByText("Related Medal edit (this file is its original)")).toBeTruthy();
});
