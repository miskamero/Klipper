// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import MedalImport from "./MedalImport";
import * as service from "../services/medal";
vi.mock("../services/medal", () => ({
	selectMedalCatalog: vi.fn(),
	medalPreviewPage: vi.fn(),
	medalImportProgress: vi.fn(),
	commitMedalImport: vi.fn(),
	cancelMedalImport: vi.fn(),
}));
const preview: service.ImportPreview = {
	token: 7,
	catalogEntries: 2,
	total: 2,
	offset: 0,
	counts: {
		matched: 1,
		unchanged: 0,
		updated: 0,
		newFiles: 1,
		missing: 0,
		ambiguous: 0,
		unmatched: 0,
		conflicts: 1,
		warnings: 0,
		skipped: 0,
	},
	rows: [
		{
			index: 0,
			clipId: "synthetic-one",
			relation: "primary",
			title: "Imported synthetic title",
			game: "Synthetic game",
			path: "/synthetic/existing.mp4",
			status: "matched",
			conflict: true,
			warning: false,
		},
		{
			index: 1,
			clipId: "synthetic-new",
			relation: "primary",
			title: null,
			game: null,
			path: "/synthetic/new.mp4",
			status: "new",
			conflict: false,
			warning: false,
		},
	],
};
beforeEach(() => {
	HTMLDialogElement.prototype.showModal = function () {
		this.open = true;
	};
	vi.mocked(service.selectMedalCatalog).mockResolvedValue(preview);
	vi.mocked(service.cancelMedalImport).mockResolvedValue(undefined);
	vi.mocked(service.medalImportProgress).mockResolvedValue({
		phase: "preparing",
		done: 0,
		total: 2,
		linked: 0,
		error: "",
	});
	vi.mocked(service.commitMedalImport).mockResolvedValue({
		phase: "complete",
		done: 2,
		total: 2,
		linked: 1,
		error: "",
	});
});
afterEach(() => {
	cleanup();
	vi.resetAllMocks();
});
it("separates unresolved game lookups from contradictory recording metadata", async () => {
	vi.mocked(service.selectMedalCatalog).mockResolvedValue({
		...preview,
		counts: { ...preview.counts, conflicts: 0, warnings: 1 },
		rows: [{ ...preview.rows[0], conflict: false, warning: true }],
	});
	render(<MedalImport onClose={vi.fn()} onImported={vi.fn()} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	await screen.findByText("Imported synthetic title");
	expect(screen.getByText("Game lookup warnings")).toBeTruthy();
	expect(screen.getByText(/Their game names remain unknown; titles are unaffected/)).toBeTruthy();
	expect(screen.queryByText(/Some references contain contradictory metadata/)).toBeNull();
	expect(screen.getByText(/Game lookup unresolved/)).toBeTruthy();
	expect(screen.queryByText(/Metadata conflict/)).toBeNull();
});
it("previews exact associations without importing and defaults to metadata-only confirmation", async () => {
	const imported = vi.fn();
	render(<MedalImport onClose={vi.fn()} onImported={imported} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	await screen.findByText("Imported synthetic title");
	expect(service.commitMedalImport).not.toHaveBeenCalled();
	expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
	fireEvent.click(screen.getByRole("button", { name: "Import metadata only" }));
	await waitFor(() => expect(imported).toHaveBeenCalledOnce());
	expect(service.commitMedalImport).toHaveBeenCalledWith(7, false);
	expect(await screen.findByText(/Import complete/)).toBeTruthy();
});
it("requires explicit new-reference approval and sends only the preview token and choice", async () => {
	render(<MedalImport onClose={vi.fn()} onImported={vi.fn()} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	await screen.findByText("Imported synthetic title");
	fireEvent.click(screen.getByRole("checkbox"));
	fireEvent.click(
		screen.getByRole("button", { name: "Import metadata and approved references" }),
	);
	await waitFor(() => expect(service.commitMedalImport).toHaveBeenCalledWith(7, true));
});
it("shows malformed catalog failures and keeps native cancellation free of side effects", async () => {
	vi.mocked(service.selectMedalCatalog)
		.mockRejectedValueOnce("Invalid synthetic catalog")
		.mockResolvedValueOnce(null);
	render(<MedalImport onClose={vi.fn()} onImported={vi.fn()} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	expect(await screen.findByRole("alert")).toHaveProperty(
		"textContent",
		"Invalid synthetic catalog",
	);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
	expect(service.commitMedalImport).not.toHaveBeenCalled();
});
it("cancels pending native work and ignores its response after unmount", async () => {
	let finish!: (value: service.ImportPreview) => void;
	vi.mocked(service.selectMedalCatalog).mockReturnValue(
		new Promise((resolve) => {
			finish = resolve;
		}),
	);
	const imported = vi.fn();
	const { unmount } = render(<MedalImport onClose={vi.fn()} onImported={imported} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	fireEvent.click(screen.getByRole("button", { name: "Cancel operation" }));
	await waitFor(() => expect(service.cancelMedalImport).toHaveBeenCalled());
	unmount();
	await act(async () => finish(preview));
	expect(imported).not.toHaveBeenCalled();
	expect(service.commitMedalImport).not.toHaveBeenCalled();
});

it("treats an acknowledged cancellation as a status and discards the expired preview", async () => {
	let reject!: (reason: string) => void;
	vi.mocked(service.selectMedalCatalog).mockReturnValue(
		new Promise((_resolve, fail) => {
			reject = fail;
		}),
	);
	render(<MedalImport onClose={vi.fn()} onImported={vi.fn()} />);
	fireEvent.click(screen.getByRole("button", { name: "Choose catalog" }));
	fireEvent.click(screen.getByRole("button", { name: "Cancel operation" }));
	await waitFor(() => expect(service.cancelMedalImport).toHaveBeenCalled());
	await act(async () => reject("Import cancelled"));
	expect(await screen.findByText("Import cancelled. No changes were saved.")).toBeTruthy();
	expect(screen.queryByRole("alert")).toBeNull();
	expect(screen.queryByRole("button", { name: "Import metadata only" })).toBeNull();
});
