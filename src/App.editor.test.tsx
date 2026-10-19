// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect, type RefObject } from "react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";
import { invoke } from "@tauri-apps/api/core";
import type { EditorBridge } from "./services/editor";
import type { ExportJob } from "./services/exports";

const calls = vi.hoisted(() => ({
	mounts: vi.fn(),
	command: vi.fn(),
	fullscreen: vi.fn(async () => true),
	jobs: [] as ExportJob[],
}));
vi.mock("./services/exports", async (importOriginal) => {
	const actual = await importOriginal<typeof import("./services/exports")>();
	return {
		...actual,
		useExports: () => ({ jobs: calls.jobs, error: "", refresh: vi.fn(), action: vi.fn() }),
	};
});
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => false, invoke: vi.fn() }));
vi.mock("./services/media", () => ({
	selectRecording: vi.fn(async () => ({
		id: 1,
		name: "Standalone.mp4",
		path: "/synthetic/standalone.mp4",
	})),
	analyzeRecording: vi.fn(async () => null),
	deactivateRecording: vi.fn(async () => {}),
	activateRecording: vi.fn(),
	errorMessage: String,
}));
vi.mock("./services/library", () => ({
	openLibraryRecording: vi.fn(async () => ({
		recording: { id: 2, name: "Indexed.mp4", path: "/synthetic/indexed.mp4" },
		info: null,
	})),
}));
vi.mock("./components/Library", () => ({
	default: ({ active, onOpen }: { active: boolean; onOpen: (id: number) => void }) => (
		<div hidden={!active}>
			<input aria-label="Synthetic library search" defaultValue="preserved" />
			<button onClick={() => onOpen(2)}>Open indexed clip</button>
		</div>
	),
}));
vi.mock("./components/RecordingPlayer", () => ({
	default: function MockRecordingPlayer({
		recording,
		editorBridge,
	}: {
		recording: { id: number };
		editorBridge: RefObject<EditorBridge>;
	}) {
		useEffect(() => {
			calls.mounts(recording.id);
			editorBridge.current.command = calls.command;
			editorBridge.current.fullscreen = calls.fullscreen;
		}, [recording.id, editorBridge]);
		return <div>Native session {recording.id}</div>;
	},
}));
afterEach(() => {
	cleanup();
	vi.clearAllMocks();
	localStorage.clear();
	calls.jobs = [];
});

it.each(["standalone", "library"])(
	"enters and leaves the editor without remounting %s playback",
	async (context) => {
		vi.mocked(invoke).mockImplementation(async (name) => {
			if (name === "editor_begin")
				return {
					version: 1,
					token: 10,
					start: 0,
					end: 4,
					info: { duration: 4, audio: [] },
				};
			if (name === "editor_frame") return new ArrayBuffer(0);
			if (name === "editor_range")
				return {
					version: 1,
					token: 10,
					start: 1,
					end: 4,
					info: { duration: 4, audio: [] },
				};
		});
		render(<App />);
		if (context === "standalone") fireEvent.click(screen.getByLabelText("Open recording"));
		else fireEvent.click(screen.getByText("Open indexed clip"));
		await screen.findByText(`Native session ${context === "standalone" ? 1 : 2}`);
		fireEvent.click(screen.getByText("Edit video"));
		await screen.findByRole("region", { name: "Video editor" });
		expect(calls.fullscreen).toHaveBeenCalledWith(false);
		expect(calls.mounts).toHaveBeenCalledTimes(1);
		fireEvent.change(screen.getByLabelText("Trim start time"), { target: { value: "1" } });
		fireEvent.blur(screen.getByLabelText("Trim start time"));
		await waitFor(() => expect(screen.queryByText(/Aligning start\/end/)).toBeNull());
		fireEvent.click(screen.getByText(/Back to player/));
		await screen.findByRole("alertdialog", { name: "Discard unsaved edits?" });
		fireEvent.click(screen.getByText("Keep editing"));
		expect(screen.getByRole("region", { name: "Video editor" })).toBeTruthy();
		fireEvent.click(screen.getByText(/Back to player/));
		fireEvent.click(screen.getByText("Discard edits"));
		await waitFor(() =>
			expect(screen.queryByRole("region", { name: "Video editor" })).toBeNull(),
		);
		expect(invoke).toHaveBeenCalledWith("editor_close", { token: 10 });
		expect(calls.mounts).toHaveBeenCalledTimes(1);
		if (context === "library") {
			fireEvent.click(screen.getByText(/Back to library/));
			expect(
				(screen.getByLabelText("Synthetic library search") as HTMLInputElement).value,
			).toBe("preserved");
		}
	},
);

it.each(["standalone", "library"])(
	"queues an immutable job and restores its %s context without waiting for encoding",
	async (context) => {
		calls.jobs = [
			{
				id: 1,
				name: "Other.mp4",
				path: "C:/synthetic/Other-export.mp4",
				phase: "encoding",
				seconds: 2,
				duration: 30,
				eta: 20,
				encoder: "cpu",
				notice: "",
				error: "",
			},
		];
		vi.mocked(invoke).mockImplementation(async (name) => {
			if (name === "editor_begin")
				return {
					version: 1,
					token: 10,
					start: 0,
					end: 4,
					info: { duration: 4, audio: [] },
				};
			if (name === "editor_frame") return new ArrayBuffer(0);
			if (name === "editor_destination") return "C:/synthetic/Queued.mp4";
			if (name === "editor_export") return 2;
		});
		render(<App />);
		expect(screen.getByRole("complementary", { name: "Export queue" })).toBeTruthy();
		if (context === "standalone") fireEvent.click(screen.getByLabelText("Open recording"));
		else fireEvent.click(screen.getByText("Open indexed clip"));
		await screen.findByText(`Native session ${context === "standalone" ? 1 : 2}`);
		expect(screen.queryByRole("complementary", { name: "Export queue" })).toBeNull();
		fireEvent.click(screen.getByText("Edit video"));
		await screen.findByRole("region", { name: "Video editor" });
		expect(screen.queryByRole("complementary", { name: "Export queue" })).toBeNull();
		fireEvent.click(screen.getByText("Save as…"));
		await screen.findByText(/Destination:/);
		fireEvent.click(screen.getByText("Export new video"));
		await waitFor(() =>
			expect(screen.queryByRole("region", { name: "Video editor" })).toBeNull(),
		);
		expect(invoke).toHaveBeenCalledWith("editor_export", { token: 10, encoder: "auto" });
		expect(invoke).toHaveBeenCalledWith("editor_close", { token: 10 });
		if (context === "library") {
			expect(screen.getByRole("complementary", { name: "Export queue" })).toBeTruthy();
			expect(
				(screen.getByLabelText("Synthetic library search") as HTMLInputElement).value,
			).toBe("preserved");
		} else {
			expect(screen.getByText("Native session 1")).toBeTruthy();
			expect(screen.queryByRole("complementary", { name: "Export queue" })).toBeNull();
		}
		expect(calls.jobs[0].phase).toBe("encoding");
	},
);
