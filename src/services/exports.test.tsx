// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useExports } from "./exports";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
afterEach(() => {
	cleanup();
	vi.clearAllMocks();
});
it("subscribes before initial snapshot, rejects stale snapshots and detaches without cancelling jobs", async () => {
	let receive: (event: { payload: { revision: number; jobs: unknown[] } }) => void = () => {};
	const stop = vi.fn();
	let initialResolve: (value: unknown) => void = () => {};
	vi.mocked(listen).mockImplementation(async (_event, handler) => {
		receive = (event) => handler({ event: "exports-changed", id: 0, payload: event.payload });
		return stop;
	});
	vi.mocked(invoke).mockImplementation(
		() =>
			new Promise((resolve) => {
				initialResolve = resolve;
			}),
	);
	function View() {
		const { jobs } = useExports();
		return <p>{jobs.map((j) => j.phase).join(",")}</p>;
	}
	render(<View />);
	await waitFor(() => expect(invoke).toHaveBeenCalledWith("exports_status"));
	act(() => receive({ payload: { revision: 3, jobs: [{ id: 1, phase: "encoding" }] } }));
	await act(async () => initialResolve({ revision: 2, jobs: [] }));
	expect(screen.getByText("encoding")).toBeTruthy();
	act(() => receive({ payload: { revision: 4, jobs: [{ id: 1, phase: "completed" }] } }));
	expect(screen.getByText("completed")).toBeTruthy();
	cleanup();
	await Promise.resolve();
	expect(stop).toHaveBeenCalled();
	expect(vi.mocked(invoke).mock.calls.every(([name]) => name === "exports_status")).toBe(true);
});
